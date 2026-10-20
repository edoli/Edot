use super::*;

impl Edot {
    pub(in crate::desktop) fn replace_result(
        &mut self,
        key: MatchKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pattern = match self.global.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            _ => return,
        };
        let Ok(text) = self.text(key.id, cx) else {
            return;
        };
        let replacement = self.global.replacement.read(cx).value();
        let edits = pattern.edits_text(&text, &replacement);
        let Some(edit) = edits.into_iter().find(|edit| edit.range == key.range) else {
            self.global.message = "본문이 변경되었습니다. 다시 검색하세요".into();
            cx.notify();
            return;
        };
        if self.open_note(key.id, false, window, cx)
            && self.set_read_only(false, window, cx)
            && let Some(editor) = self.active_editor()
        {
            editor.update(cx, |state, cx| {
                state.set_selected_range(editing::byte_range(&text, key.range), cx);
                state.replace(edit.replacement, window, cx);
            });
            self.changed(key.id, window, cx);
        }
    }
    pub(in crate::desktop) fn replace_global_all(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pattern = match self.global.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            Ok(None) => return,
            Err(error) => {
                self.global.message = error;
                cx.notify();
                return;
            }
        };
        let replacement = self.global.replacement.read(cx).value();
        let plan = (|| -> io::Result<Vec<global_search::Change>> {
            let mut changes = vec![];
            for note in self.book.notes.iter().filter(|note| !note.is_folder()) {
                let path = self.book.file(&self.dir, note.id);
                let before_disk = fs::read(&path)?;
                let text = self.text(note.id, cx)?;
                let edits = pattern.edits_text(&text, &replacement);
                let after = crate::search::apply_edits(&text, &edits);
                if after != text {
                    changes.push(global_search::Change {
                        id: note.id,
                        path,
                        before_disk,
                        after,
                        count: edits.len(),
                    });
                }
            }
            Ok(changes)
        })();
        let changes = match plan {
            Ok(changes) => Rc::new(changes),
            Err(error) => {
                self.fail("전체 바꾸기를 준비하지 못했습니다", error, cx);
                return;
            }
        };
        let count: usize = changes.iter().map(|change| change.count).sum();
        if count == 0 {
            self.global.message = "바꿀 내용이 없습니다".into();
            cx.notify();
            return;
        }
        let weak = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let changes = changes.clone();
            let weak = weak.clone();
            dialog
                .title("전체 노트 바꾸기")
                .description(format!(
                    "{}개 노트의 {count}곳을 바꾸고 저장합니다.",
                    changes.len()
                ))
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("모두 바꾸기")
                        .cancel_text("취소"),
                )
                .on_ok(move |_, window, cx| {
                    weak.update(cx, |this, cx| {
                        if let Err(error) = global_search::commit(&changes) {
                            this.fail("전체 바꾸기를 저장하지 못했습니다", error, cx);
                            return false;
                        }
                        for change in changes.iter() {
                            if let Some(tab) = this.open.iter_mut().find(|tab| tab.id == change.id)
                                && let Some(editor) = &tab.editor
                            {
                                editor.update(cx, |state, cx| {
                                    let range = state.selected_range();
                                    state.replace_all(change.after.clone(), window, cx);
                                    state.set_selected_range(
                                        range.start.min(change.after.len())
                                            ..range.end.min(change.after.len()),
                                        cx,
                                    );
                                });
                                tab.saved = change.after.clone();
                                tab.saved_revision = tab.saved_revision.wrapping_add(1);
                                tab.metrics = editing::TextMetrics::new(&change.after);
                                tab.metrics_revision = editor.read(cx).revision;
                                tab.dirty = false;
                            }
                        }
                        this.status = format!("{}개 노트, {count}곳 바꿈", changes.len());
                        this.queue_search(window, cx);
                        true
                    })
                    .unwrap_or(false)
                })
        });
    }
}
