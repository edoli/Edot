use super::*;

impl Edot {
    pub(super) fn show_find(
        &mut self,
        global: bool,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !global && self.active_editor().is_none() {
            return;
        }
        self.settings_visible = false;
        if global {
            self.search_mode = true;
            self.session.sidebar_visible = true;
            self.find_visible = false;
        } else {
            self.find_visible = true;
        }
        let selected = self
            .active_editor()
            .map(|e| e.read(cx).selected_value().to_string())
            .unwrap_or_default();
        let panel = if global {
            &mut self.global
        } else {
            &mut self.local
        };
        panel.replace = replace;
        panel.query.update(cx, |state, cx| {
            if !selected.is_empty()
                && !selected.contains(['\r', '\n'])
                && selected.encode_utf16().count() <= 200
            {
                state.set_value(selected, window, cx);
            }
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        if global {
            self.queue_search(window, cx);
        } else {
            self.find_next(false, true, window, cx);
        }
        cx.notify();
    }
    pub(super) fn navigate(
        &mut self,
        global: bool,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if global {
            self.global_next(backwards, window, cx);
        } else {
            self.find_next(backwards, false, window, cx);
        }
    }
    pub(super) fn find_next(
        &mut self,
        backwards: bool,
        live: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let pattern = match self.local.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            Ok(None) => {
                self.local.last = None;
                self.local.message = "검색어를 입력하세요".into();
                cx.notify();
                return;
            }
            Err(error) => {
                self.local.message = error;
                cx.notify();
                return;
            }
        };
        let text = editor.read(cx).value();
        let selected = editing::paragraph_range(&text, editor.read(cx).selected_range());
        let start = if backwards || live {
            selected.start
        } else {
            selected.end
        };
        let skip = !live && self.local.last.is_some_and(|key| key.range == selected);
        let mut ranges = vec![];
        pattern.visit_text(&text, |range| {
            ranges.push(range);
            true
        });
        let fits = |range: &&CharRange| {
            (if backwards {
                range.end <= start
            } else {
                range.start >= start
            }) && !(skip && range.start == start && range.end == start)
        };
        let found = if backwards {
            ranges.iter().rev().find(fits).or_else(|| ranges.last())
        } else {
            ranges.iter().find(fits).or_else(|| ranges.first())
        }
        .copied();
        self.local.last = found.map(|range| MatchKey {
            id: self.session.active.unwrap(),
            range,
        });
        self.local.message = if let Some(range) = found {
            let index = ranges.iter().position(|r| *r == range).unwrap_or(0) + 1;
            editor.update(cx, |state, cx| {
                state.set_selected_range(editing::byte_range(&text, range), cx)
            });
            format!("{index} / {}곳", ranges.len())
        } else {
            "일치하는 텍스트가 없습니다".into()
        };
        cx.notify();
    }
    pub(super) fn replace_local(&mut self, all: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let pattern = match self.local.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            Ok(None) => return,
            Err(error) => {
                self.local.message = error;
                cx.notify();
                return;
            }
        };
        let text = editor.read(cx).value();
        let replacement = self.local.replacement.read(cx).value();
        let edits = pattern.edits_text(&text, &replacement);
        if all {
            if !edits.is_empty() {
                let after = crate::search::apply_edits(&text, &edits);
                editor.update(cx, |state, cx| state.replace_all(after, window, cx));
                self.changed(self.session.active.unwrap(), window, cx);
            }
            self.local.message = format!("{}곳 바꿨습니다", edits.len());
        } else {
            let selected = editing::paragraph_range(&text, editor.read(cx).selected_range());
            if let Some(edit) = edits.iter().find(|edit| edit.range == selected)
                && (selected.start != selected.end
                    || self.local.last.is_some_and(|key| key.range == selected))
            {
                editor.update(cx, |state, cx| {
                    state.replace(edit.replacement.clone(), window, cx)
                });
                self.changed(self.session.active.unwrap(), window, cx);
            }
            self.find_next(false, false, window, cx);
        }
        cx.notify();
    }
    pub(super) fn queue_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_generation += 1;
        let generation = self.search_generation;
        self.global_task = None;
        self.note_search_tasks.clear();
        self.pending_search_notes.clear();
        self.search_errors = 0;
        self.rows.clear();
        self.result_rows.clear();
        self.result_cursor = None;
        self.global.last = None;
        let pattern = match self.global.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            Ok(None) => {
                self.scanning = false;
                self.global.message = "검색어를 입력하세요".into();
                cx.notify();
                return;
            }
            Err(error) => {
                self.scanning = false;
                self.global.message = error;
                cx.notify();
                return;
            }
        };
        self.scanning = true;
        self.global.message = "전체 메모 검색 중…".into();
        self.global_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(180))
                .await;
            let Ok(ids) = this.update(cx, |this, _| {
                this.book.notes.iter().map(|n| n.id).collect::<Vec<_>>()
            }) else {
                return;
            };
            let mut rows = Vec::new();
            let mut displayed = 0;
            let mut errors = 0;
            for (index, id) in ids.iter().copied().enumerate() {
                let Ok(Some((path, loaded))) = this.update(cx, |this, cx| {
                    if this.search_generation != generation {
                        return None;
                    }
                    let loaded = this
                        .open
                        .iter()
                        .find(|tab| tab.id == id)
                        .and_then(|tab| tab.editor.as_ref())
                        .map(|editor| editor.read(cx).value().to_string());
                    Some((this.book.file(&this.dir, id), loaded))
                }) else {
                    return;
                };
                let pattern = pattern.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let text = loaded.map(Ok).unwrap_or_else(|| fs::read_to_string(path))?;
                        Ok::<_, io::Error>(global_search::scan(
                            id,
                            &text,
                            &pattern,
                            global_search::MAX_RESULTS.saturating_sub(displayed),
                        ))
                    })
                    .await;
                match result {
                    Ok(Some(row)) => {
                        displayed += row.matches.len();
                        rows.push(row);
                    }
                    Err(_) => errors += 1,
                    _ => {}
                }
                if index % 16 == 0 {
                    let _ = this.update(cx, |this, cx| {
                        this.global.message =
                            format!("전체 메모 검색 중… {}/{}", index + 1, ids.len());
                        cx.notify();
                    });
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                if this.search_generation != generation {
                    return;
                }
                this.rows = rows;
                this.search_errors = errors;
                this.update_search_message();
                this.rebuild_results();
                this.scanning = false;
                this.refresh_pending_notes(window, cx);
                if let Some(backwards) = this.pending_navigation.take() {
                    this.global_next(backwards, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn update_search_message(&mut self) {
        let total: usize = self.rows.iter().map(|r| r.count).sum();
        let displayed: usize = self.rows.iter().map(|r| r.matches.len()).sum();
        self.global.message = format!("{total}곳 · {}개 메모", self.rows.len());
        if self.search_errors > 0 {
            self.global
                .message
                .push_str(&format!(" · {}개 읽기 실패", self.search_errors));
        }
        if total > displayed {
            self.global
                .message
                .push_str(&format!(" · {displayed}개까지 표시"));
        }
    }
    fn refresh_pending_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for id in std::mem::take(&mut self.pending_search_notes) {
            self.queue_note_search(id, window, cx);
        }
    }
    pub(super) fn queue_note_search(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A full scan may already have read this note. Reconcile after it finishes.
        if self.scanning {
            self.pending_search_notes.insert(id);
            return;
        }
        let Ok(Some(pattern)) = self.global.pattern(cx) else {
            return;
        };
        let generation = self.search_generation;
        self.note_search_tasks.remove(&id);
        let task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(180))
                .await;
            let Ok(Some((path, loaded, revision))) = this.update(cx, |this, cx| {
                this.book.get(id)?;
                let editor = this
                    .open
                    .iter()
                    .find(|t| t.id == id)
                    .and_then(|t| t.editor.as_ref());
                Some((
                    this.book.file(&this.dir, id),
                    editor.map(|e| e.read(cx).value()),
                    editor.map(|e| e.read(cx).revision),
                ))
            }) else {
                return;
            };
            let row = cx
                .background_executor()
                .spawn(async move {
                    let text = loaded
                        .map(|s| Ok(s.to_string()))
                        .unwrap_or_else(|| fs::read_to_string(path))?;
                    Ok::<_, io::Error>(global_search::scan(
                        id,
                        &text,
                        &pattern,
                        global_search::MAX_RESULTS,
                    ))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.search_generation {
                    return;
                }
                this.note_search_tasks.remove(&id);
                let current = this
                    .open
                    .iter()
                    .find(|t| t.id == id)
                    .and_then(|t| t.editor.as_ref())
                    .map(|e| e.read(cx).revision);
                if current != revision {
                    this.queue_note_search(id, window, cx);
                    return;
                }
                match row {
                    Ok(row) => {
                        this.merge_note_results(id, row);
                        // Only refill previously truncated previews when space becomes available.
                        let displayed: usize = this.rows.iter().map(|r| r.matches.len()).sum();
                        if displayed < global_search::MAX_RESULTS
                            && let Some(next) = this
                                .rows
                                .iter()
                                .find(|r| {
                                    r.count > r.matches.len()
                                        && !this.note_search_tasks.contains_key(&r.id)
                                })
                                .map(|r| r.id)
                        {
                            this.queue_note_search(next, window, cx);
                        }
                    }
                    Err(error) => this.global.message = format!("메모 검색 실패: {error}"),
                }
                cx.notify();
            });
        });
        self.note_search_tasks.insert(id, task);
    }
    pub(super) fn result_identity(&self, index: usize) -> Option<(u64, Option<CharRange>)> {
        let row = self.result_rows.get(index)?;
        let group = &self.rows[row.group];
        Some((group.id, row.hit.map(|hit| group.matches[hit].range)))
    }
    fn merge_note_results(&mut self, id: u64, mut row: Option<ResultRow>) {
        let selected = self.result_cursor.and_then(|i| self.result_identity(i));
        let scroll = self.result_scroll.0.borrow().base_handle.clone();
        let offset = scroll.offset();
        let top = (f32::from(-offset.y) / global_search::ROW_HEIGHT as f32)
            .floor()
            .max(0.) as usize;
        let anchor = self.result_identity(top);
        let other_count: usize = self
            .rows
            .iter()
            .filter(|r| r.id != id)
            .map(|r| r.matches.len())
            .sum();
        if let Some(row) = &mut row {
            row.matches
                .truncate(global_search::MAX_RESULTS.saturating_sub(other_count));
        }
        if let Some(index) = self.rows.iter().position(|r| r.id == id) {
            if let Some(row) = row {
                self.rows[index] = row;
            } else {
                self.rows.remove(index);
            }
        } else if let Some(row) = row {
            let order = self
                .book
                .notes
                .iter()
                .position(|n| n.id == id)
                .unwrap_or(usize::MAX);
            let index = self.rows.partition_point(|r| {
                self.book
                    .notes
                    .iter()
                    .position(|n| n.id == r.id)
                    .unwrap_or(usize::MAX)
                    < order
            });
            self.rows.insert(index, row);
        }
        self.rebuild_results();
        self.result_cursor = selected.and_then(|key| {
            (0..self.result_rows.len()).find(|&i| self.result_identity(i) == Some(key))
        });
        if let Some(key) = anchor
            && let Some(index) =
                (0..self.result_rows.len()).find(|&i| self.result_identity(i) == Some(key))
        {
            scroll.set_offset(point(
                offset.x,
                offset.y - px((index as f32 - top as f32) * global_search::ROW_HEIGHT as f32),
            ));
        }
        if self.global.last.is_some_and(|key| {
            key.id == id
                && !self
                    .rows
                    .iter()
                    .any(|r| r.id == id && r.matches.iter().any(|m| m.range == key.range))
        }) {
            self.global.last = None;
        }
        self.update_search_message();
    }
    pub(super) fn rebuild_results(&mut self) {
        self.result_rows = global_search::visible_rows(&self.rows, &self.result_collapsed);
        self.result_cursor = self
            .result_cursor
            .filter(|index| *index < self.result_rows.len());
    }
    pub(super) fn open_match(
        &mut self,
        key: MatchKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pattern = match self.global.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            _ => return,
        };
        let text = match self.text(key.id, cx) {
            Ok(text) => text,
            Err(error) => {
                self.fail("검색 결과를 열지 못했습니다", error, cx);
                return;
            }
        };
        let mut valid = false;
        pattern.visit_text(&text, |range| {
            if range == key.range {
                valid = true;
                return false;
            }
            range.start <= key.range.start
        });
        if !valid {
            self.global.message = "본문이 변경되었습니다. 결과를 새로 고칩니다".into();
            self.queue_note_search(key.id, window, cx);
            return;
        }
        if self.open_note(key.id, false, window, cx) {
            if let Some(editor) = self.active_editor() {
                editor.update(cx, |state, cx| {
                    state.set_selected_range(editing::byte_range(&text, key.range), cx)
                });
            }
            self.global.last = Some(key);
            cx.notify();
        }
    }
    pub(super) fn activate_result(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.result_rows.get(index).copied() else {
            return;
        };
        self.result_cursor = Some(index);
        let group = &self.rows[row.group];
        if let Some(hit) = row.hit {
            self.open_match(
                MatchKey {
                    id: group.id,
                    range: group.matches[hit].range,
                },
                window,
                cx,
            );
        } else {
            let id = group.id;
            if !self.result_collapsed.insert(id) {
                self.result_collapsed.remove(&id);
            }
            self.rebuild_results();
        }
        self.results_focus.focus(window, cx);
        cx.notify();
    }
    pub(super) fn global_next(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scanning {
            self.pending_navigation = Some(backwards);
            return;
        }
        let pattern = match self.global.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            _ => return,
        };
        let total: usize = self.rows.iter().map(|row| row.count).sum();
        let displayed: usize = self.rows.iter().map(|row| row.matches.len()).sum();
        if displayed == total {
            let keys: Vec<_> = self
                .rows
                .iter()
                .flat_map(|row| {
                    row.matches.iter().map(|hit| MatchKey {
                        id: row.id,
                        range: hit.range,
                    })
                })
                .collect();
            if keys.is_empty() {
                return;
            }
            let current = self
                .global
                .last
                .and_then(|key| keys.iter().position(|hit| *hit == key));
            let index = match (current, backwards) {
                (Some(index), true) => (index + keys.len() - 1) % keys.len(),
                (Some(index), false) => (index + 1) % keys.len(),
                (None, true) => keys.len() - 1,
                (None, false) => 0,
            };
            self.select_global_match(keys[index], window, cx);
            return;
        }
        let current = self.global.last;
        let generation = self.search_generation;
        let documents: Vec<_> = self
            .book
            .notes
            .iter()
            .map(|note| {
                let loaded = self
                    .open
                    .iter()
                    .find(|tab| tab.id == note.id)
                    .and_then(|tab| tab.editor.as_ref())
                    .map(|editor| editor.read(cx).value().to_string());
                (note.id, self.book.file(&self.dir, note.id), loaded)
            })
            .collect();
        self.scanning = true;
        self.global_task = Some(cx.spawn_in(window, async move |this, cx| {
            let key = cx
                .background_executor()
                .spawn(async move {
                    // Navigation uses all matches, including those beyond the preview limit.
                    let mut first = None;
                    let mut last = None;
                    let mut next = None;
                    let mut previous = None;
                    let mut passed = current.is_none();
                    let mut seen_current = false;
                    for (id, path, loaded) in documents {
                        let Ok(text) = loaded.map(Ok).unwrap_or_else(|| fs::read_to_string(path))
                        else {
                            continue;
                        };
                        pattern.visit_text(&text, |range| {
                            let key = MatchKey { id, range };
                            first.get_or_insert(key);
                            if current == Some(key) {
                                previous = last;
                                passed = true;
                                seen_current = true;
                            } else if passed && next.is_none() {
                                next = Some(key);
                            }
                            last = Some(key);
                            true
                        });
                    }
                    if backwards {
                        if seen_current {
                            previous.or(last)
                        } else {
                            last
                        }
                    } else {
                        next.or(first)
                    }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.search_generation != generation {
                    return;
                }
                this.scanning = false;
                this.refresh_pending_notes(window, cx);
                if let Some(key) = key {
                    this.select_global_match(key, window, cx);
                }
                if let Some(backwards) = this.pending_navigation.take() {
                    this.global_next(backwards, window, cx);
                }
                cx.notify();
            });
        }));
    }
    fn select_global_match(&mut self, key: MatchKey, window: &mut Window, cx: &mut Context<Self>) {
        self.result_collapsed.remove(&key.id);
        self.rebuild_results();
        self.open_match(key, window, cx);
        if let Some(index) = self.result_rows.iter().position(|row| {
            row.hit.is_some_and(|hit| {
                self.rows[row.group].id == key.id
                    && self.rows[row.group].matches[hit].range == key.range
            })
        }) {
            self.result_cursor = Some(index);
            self.result_scroll
                .scroll_to_item(index, ScrollStrategy::Nearest);
        }
        cx.notify();
    }
    pub(super) fn replace_result(
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
            && let Some(editor) = self.active_editor()
        {
            editor.update(cx, |state, cx| {
                state.set_selected_range(editing::byte_range(&text, key.range), cx);
                state.replace(edit.replacement, window, cx);
            });
            self.changed(key.id, window, cx);
        }
    }
    pub(super) fn replace_global_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            for note in &self.book.notes {
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
                .title("전체 메모 바꾸기")
                .description(format!(
                    "{}개 메모의 {count}곳을 바꾸고 저장합니다.",
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
                                tab.metrics = editing::TextMetrics::new(&change.after);
                                tab.metrics_revision = editor.read(cx).revision;
                                tab.dirty = false;
                            }
                        }
                        this.status = format!("{}개 메모, {count}곳 바꿈", changes.len());
                        this.queue_search(window, cx);
                        true
                    })
                    .unwrap_or(false)
                })
        });
    }
}
