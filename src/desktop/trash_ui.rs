//! A lazy Space-local recycle bin with a bounded, read-only body preview.
use super::*;
use crate::trash::{self, Entry};
use crate::widgets::scroll::Scrollbar;

pub(super) struct TrashBrowser {
    dir: PathBuf,
    space_name: String,
    owner: WeakEntity<Edot>,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    pub(super) entries: Vec<Entry>,
    pub(super) selected: Option<usize>,
    pub(super) preview: Entity<InputState>,
    scroll: UniformListScrollHandle,
    loading: bool,
    preview_loading: bool,
    preview_truncated: bool,
    message: Option<String>,
    load_task: Option<Task<()>>,
    preview_task: Option<Task<()>>,
}

impl Edot {
    pub(super) fn show_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.startup_failed || !self.finish_rename(true, false, window, cx) {
            return;
        }
        self.close_space_list(window, cx);
        self.dismiss_settings(cx);
        self.shortcut_help = None;
        self.keyboard_menu = None;
        self.context_note = None;
        cx.stop_active_drag(window);
        self.stop_drag();
        self.resizing = None;
        let dir = self.dir.clone();
        let space_name = self.current_space_name();
        let owner = cx.entity().downgrade();
        let previous_focus = window.focused(cx);
        let browser = cx.new(|cx| {
            let preview = cx.new(|cx| {
                let mut input = TextareaState::new(window, cx).soft_wrap(true);
                input.set_text_mode(component::input::TextMode::PlainText, cx);
                input.set_read_only(true, cx);
                input
            });
            TrashBrowser {
                dir,
                space_name,
                owner,
                previous_focus,
                focus: cx.focus_handle(),
                preview,
                entries: vec![],
                selected: None,
                scroll: UniformListScrollHandle::new(),
                loading: false,
                preview_loading: false,
                preview_truncated: false,
                message: None,
                load_task: None,
                preview_task: None,
            }
        });
        browser.update(cx, |browser, cx| {
            browser.focus.focus(window, cx);
            browser.reload(window, cx);
        });
        self.trash_browser = Some(browser);
        cx.notify();
    }

    pub(super) fn restore_trash(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
    ) -> io::Result<trash::Restored> {
        if self.startup_failed || !self.save_all(cx) {
            return Err(io::Error::other("현재 노트를 저장한 뒤 복원하세요."));
        }
        let restored = trash::restore(&mut self.book, &self.dir, key)?;
        self.file_sync.tree_revision = self.file_sync.tree_revision.wrapping_add(1);
        if let Some(id) = restored.ids.first() {
            self.caret = Some(*id);
            self.selection.single(*id);
        }
        self.status = format!("{}개 노트 복원됨", restored.ids.len());
        cx.notify();
        Ok(restored)
    }
}

impl TrashBrowser {
    pub(super) fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loading = true;
        self.preview_task = None;
        self.selected = None;
        self.preview_loading = false;
        self.preview_truncated = false;
        self.preview.update(cx, |s, cx| s.set_value("", window, cx));
        let dir = self.dir.clone();
        self.load_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { trash::list(&dir) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.loading = false;
                this.load_task = None;
                match result {
                    Ok(entries) => {
                        this.entries = entries;
                        if !this.entries.is_empty() {
                            this.select(0, window, cx);
                        }
                    }
                    Err(e) => {
                        this.entries.clear();
                        this.message = Some(format!("휴지통을 읽지 못했습니다 · {e}"));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(super) fn select(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(index).cloned() else {
            return;
        };
        self.selected = Some(index);
        self.preview_loading = true;
        self.preview_truncated = false;
        self.preview.update(cx, |s, cx| s.set_value("", window, cx));
        self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        let dir = self.dir.clone();
        let key = entry.key();
        self.preview_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { trash::preview(&dir, &entry) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this
                    .selected
                    .and_then(|i| this.entries.get(i))
                    .is_none_or(|e| e.key() != key)
                {
                    return;
                }
                this.preview_loading = false;
                this.preview_task = None;
                match result {
                    Ok((text, truncated)) => {
                        this.preview_truncated = truncated;
                        this.preview
                            .update(cx, |s, cx| s.set_value(text, window, cx));
                    }
                    Err(e) => {
                        this.message = Some(format!("본문을 읽지 못했습니다 · {e}"));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(super) fn restore_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected.and_then(|i| self.entries.get(i)).cloned() else {
            return;
        };
        if self.loading {
            return;
        }
        let result = self.owner.update(cx, |owner, cx| {
            if owner.dir != self.dir {
                return Err(io::Error::other("Space가 변경되었습니다."));
            }
            owner.restore_trash(&entry.key(), cx)
        });
        match result {
            Ok(Ok(restored)) => {
                self.message =
                    Some(restored.warning.unwrap_or_else(|| {
                        format!("{}개 노트를 복원했습니다.", restored.ids.len())
                    }));
                self.reload(window, cx);
            }
            Ok(Err(e)) => {
                self.message = Some(format!("복원하지 못했습니다 · {e}"));
            }
            Err(_) => {
                self.message = Some("Space가 닫혔습니다.".into());
            }
        }
        self.focus.focus(window, cx);
        cx.notify();
    }
    pub(super) fn close(&self, window: &mut Window, cx: &mut Context<Self>) {
        let restore = self.previous_focus.clone();
        let _ = self.owner.update(cx, |owner, cx| {
            owner.trash_browser = None;
            if let Some(focus) = restore {
                focus.focus(window, cx);
            } else {
                owner.focus_editor(window, cx);
            }
            cx.notify();
        });
    }
    fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let preview_focus = self.preview.read(cx).focus_handle(cx);
        match e.keystroke.key.as_str() {
            "escape" => self.close(window, cx),
            "f5" if !e.is_held && e.keystroke.modifiers == Modifiers::default() => {
                self.message = None;
                self.reload(window, cx);
            }
            "enter" if e.keystroke.modifiers.control && !e.is_held => {
                self.restore_selected(window, cx)
            }
            "tab" => {
                if self.focus.is_focused(window) {
                    preview_focus.focus(window, cx);
                } else {
                    self.focus.focus(window, cx);
                }
            }
            "enter" if self.focus.is_focused(window) => preview_focus.focus(window, cx),
            key @ ("up" | "down" | "home" | "end") if self.focus.is_focused(window) => {
                if !self.entries.is_empty() && !self.loading {
                    let current = self.selected.unwrap_or(0);
                    let last = self.entries.len() - 1;
                    let index = match key {
                        "up" => current.saturating_sub(1),
                        "down" => (current + 1).min(last),
                        "home" => 0,
                        _ => last,
                    };
                    self.select(index, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}

fn deleted_ago(stamp: u128) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seconds = now.saturating_sub(stamp) / 1_000_000_000;
    match seconds {
        0..60 => "방금 삭제".into(),
        60..3600 => format!("{}분 전 삭제", seconds / 60),
        3600..86400 => format!("{}시간 전 삭제", seconds / 3600),
        _ => format!("{}일 전 삭제", seconds / 86400),
    }
}

impl Render for TrashBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected.and_then(|i| self.entries.get(i));
        let count = selected.map_or(0, |e| trash::subtree(&self.entries, e).len());
        let title = selected.map_or("본문 미리보기".to_owned(), |e| e.note.title.clone());
        let legacy = selected.is_some_and(|e| !e.has_metadata);
        let list = uniform_list(
            "trash-list",
            self.entries.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let entry = &this.entries[index];
                        div()
                            .id(("trash-row", index))
                            .debug_selector(move || format!("trash-row-{index}"))
                            .v_flex()
                            .h(px(58.))
                            .px_2()
                            .py_1()
                            .gap_1()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(if this.selected == Some(index) {
                                cx.theme().selection
                            } else {
                                transparent_black()
                            })
                            .hover(|style| style.bg(cx.theme().accent))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.focus.focus(window, cx);
                                this.select(index, window, cx);
                            }))
                            .child(
                                div()
                                    .text_ellipsis()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(entry.note.title.clone()),
                            )
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(deleted_ago(entry.stamp)),
                            )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .id("trash-overlay")
            .debug_selector(|| "trash-overlay".into())
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(black().opacity(0.35))
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::key))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("trash-dialog")
                    .role(Role::Dialog)
                    .aria_label("휴지통")
                    .v_flex()
                    .gap_3()
                    .p_4()
                    .rounded_lg()
                    .shadow_lg()
                    .w(px(
                        (f32::from(window.viewport_size().width) - 32.).clamp(0., 900.)
                    ))
                    .h(px(
                        (f32::from(window.viewport_size().height) - 32.).clamp(0., 660.)
                    ))
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .flex_shrink_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .child(format!(
                                        "휴지통 · {} · {}개",
                                        self.space_name,
                                        self.entries.len()
                                    )),
                            )
                            .child(
                                Button::new("trash-refresh")
                                    .small()
                                    .icon(IconName::RefreshCw)
                                    .tooltip("새로고침 · F5")
                                    .tab_stop(false)
                                    .on_click(cx.listener(|this, _, w, cx| {
                                        this.message = None;
                                        this.reload(w, cx);
                                    })),
                            )
                            .child(
                                Button::new("trash-close")
                                    .small()
                                    .icon(IconName::Close)
                                    .tooltip("닫기 · Esc")
                                    .tab_stop(false)
                                    .on_click(cx.listener(|this, _, w, cx| this.close(w, cx))),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_stretch()
                            .gap_3()
                            .flex_1()
                            .min_h_0()
                            .child(
                                div()
                                    .relative()
                                    .w(relative(0.36))
                                    .h_full()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, w, cx| this.focus.focus(w, cx)),
                                    )
                                    .child(list)
                                    .child(Scrollbar::vertical(&self.scroll))
                                    .when(self.loading || self.entries.is_empty(), |view| {
                                        view.child(
                                            div()
                                                .absolute()
                                                .inset_0()
                                                .p_3()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(if self.loading {
                                                    "휴지통 확인 중…"
                                                } else {
                                                    "휴지통이 비어 있습니다."
                                                }),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .overflow_hidden()
                                            .child(title),
                                    )
                                    .when(legacy, |v| {
                                        v.child(
                                            div().text_color(cx.theme().muted_foreground).child(
                                                "이름·폴더 정보가 없어 최상위로 복원됩니다.",
                                            ),
                                        )
                                    })
                                    .when(self.preview_loading || self.preview_truncated, |v| {
                                        v.child(
                                            div().text_color(cx.theme().muted_foreground).child(
                                                if self.preview_loading {
                                                    "본문 확인 중…"
                                                } else {
                                                    "본문 앞부분 256KiB까지 표시합니다."
                                                },
                                            ),
                                        )
                                    })
                                    .child(
                                        div()
                                            .debug_selector(|| "trash-preview".into())
                                            .flex_1()
                                            .min_h_0()
                                            .border_1()
                                            .border_color(cx.theme().border)
                                            .rounded_md()
                                            .overflow_hidden()
                                            .child(
                                                Textarea::new(&self.preview)
                                                    .appearance(false)
                                                    .aria_label("삭제된 본문 미리보기")
                                                    .h_full(),
                                            ),
                                    ),
                            ),
                    )
                    .when_some(self.message.clone(), |v, message| {
                        v.child(div().child(message))
                    })
                    .child(div().h_flex().justify_end().flex_shrink_0().when(
                        selected.is_some() && !self.loading,
                        |v| {
                            v.child(
                                Button::new("trash-restore")
                                    .primary()
                                    .tab_stop(false)
                                    .label(if count > 1 {
                                        format!("하위 노트와 함께 복원 · {count}개")
                                    } else {
                                        "복원".into()
                                    })
                                    .tooltip("복원 · Ctrl+Enter")
                                    .on_click(
                                        cx.listener(|this, _, w, cx| this.restore_selected(w, cx)),
                                    ),
                            )
                        },
                    )),
            )
    }
}
