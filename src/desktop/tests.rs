//! Headless GPUI event tests. They never send input to the user's desktop.
use super::{Edot, Settings, StoreLock};
use crate::{
    editing::CharRange, global_search::MatchKey, model::Notebook, session::Session,
    settings::ThemeMode,
};
use gpui::{
    AnyWindowHandle, AppContext, Bounds, Context, ElementInputHandler, Entity, InputHandler,
    TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};
use std::{fs, path::PathBuf};

trait TestWindowExt {
    fn render_frame(&mut self, cx: &mut gpui::App);
    fn press(&mut self, key: &str, cx: &mut gpui::App);
    fn input(&mut self, text: &str, cx: &mut gpui::App);
}
impl TestWindowExt for Window {
    fn render_frame(&mut self, cx: &mut gpui::App) {
        self.refresh();
        self.draw(cx).clear(cx);
    }
    fn press(&mut self, key: &str, cx: &mut gpui::App) {
        use gpui::InputEvent;
        let stroke = gpui::Keystroke::parse(key).unwrap();
        if matches!(stroke.key.as_str(), "enter" | "tab") {
            self.dispatch_event(
                gpui::KeyDownEvent {
                    keystroke: stroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                }
                .to_platform_input(),
                cx,
            );
        } else {
            self.dispatch_keystroke(stroke.clone().with_simulated_ime(), cx);
        }
        self.dispatch_event(
            gpui::KeyUpEvent { keystroke: stroke }.to_platform_input(),
            cx,
        );
    }
    fn input(&mut self, text: &str, cx: &mut gpui::App) {
        for ch in text.chars() {
            self.render_frame(cx);
            let mut key = gpui::Keystroke::parse(&ch.to_string()).unwrap();
            key.key_char = Some(ch.to_string());
            self.dispatch_keystroke(key, cx);
        }
        self.render_frame(cx);
    }
}

struct Fixture {
    window: AnyWindowHandle,
    view: Entity<Edot>,
    dir: PathBuf,
    ids: [u64; 3],
}
impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "edot-gpui-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let lock = StoreLock::acquire(&dir).unwrap();
        let mut book = Notebook::load(&dir).unwrap();
        let first = book.add(0, "첫 메모".into());
        let second = book.add(0, "둘째 메모".into());
        let third = book.add(first, "하위 메모".into());
        for (id, text) in [
            (first, "🙂 한글12\r\n한글3\rkeep\n"),
            (second, "closed needle\n"),
            (third, "child needle\n"),
        ] {
            fs::write(book.file(&dir, id), text).unwrap();
        }
        book.save(&dir).unwrap();
        Session {
            tabs: vec![first, second],
            active: Some(first),
            ..Default::default()
        }
        .save(&dir)
        .unwrap();
        let (window, view) = cx.update(|cx| {
            crate::widgets::init(cx);
            crate::widgets::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(1100.), px(760.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        Edot::new(
                            dir.clone(),
                            Ok((lock, book, Settings::default())),
                            window,
                            cx,
                        )
                    })
                },
            )
            .unwrap()
        });
        Self {
            window,
            view,
            dir,
            ids: [first, second, third],
        }
    }
    fn update<R>(
        &self,
        cx: &mut TestAppContext,
        update: impl FnOnce(&mut Edot, &mut Window, &mut Context<Edot>) -> R,
    ) -> R {
        cx.update_window(self.window, |_, window, cx| {
            self.view.update(cx, |this, cx| update(this, window, cx))
        })
        .unwrap()
    }
    fn press(&self, cx: &mut TestAppContext, key: &str) {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.simulate_next_frame(cx);
            window.render_frame(cx);
            window.press(key, cx);
        })
        .unwrap();
    }
    fn input(&self, cx: &mut TestAppContext, text: &str) {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.input(text, cx);
        })
        .unwrap();
    }
}

#[gpui::test]
fn tabs_restore_lazily_and_keep_selection_history_through_settings(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        assert!(this.open[1].editor.is_none());
        this.focus_editor(window, cx);
        this.active_editor().unwrap()
    });
    f.press(cx, "ctrl-end");
    f.input(cx, "추가");
    let range = f.update(cx, |this, window, cx| {
        let range = editor.read(cx).selected_range();
        let mut settings = this.settings.clone();
        settings.wrap = false;
        settings.font_size = 24;
        settings.theme = ThemeMode::Dark;
        this.change_settings(settings, window, cx);
        assert_eq!(editor.read(cx).selected_range(), range);
        this.open_note(f.ids[1], true, window, cx);
        this.open_note(f.ids[0], true, window, cx);
        assert_eq!(
            this.active_editor().unwrap().entity_id(),
            editor.entity_id()
        );
        assert_eq!(editor.read(cx).selected_range(), range);
        range
    });
    assert!(range.start > 0);
    f.press(cx, "ctrl-z");
    f.update(cx, |this, _, cx| {
        assert_eq!(editor.read(cx).value(), "🙂 한글12\r\n한글3\rkeep\n");
        assert!(this.save_all(cx));
    });
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[1]))).unwrap(),
        "closed needle\n"
    );
}

#[gpui::test]
fn local_regex_replacement_is_one_undo_and_preserves_mixed_newlines(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.show_find(false, true, window, cx);
        this.local.regex = true;
        this.local
            .query
            .update(cx, |s, cx| s.set_value("(?<word>한글)(\\d+)", window, cx));
        this.local
            .replacement
            .update(cx, |s, cx| s.set_value("${word}-$2-$$", window, cx));
        this.replace_local(true, window, cx);
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "🙂 한글-12-$\r\n한글-3-$\rkeep\n"
        );
        this.find_visible = false;
        this.focus_editor(window, cx);
    });
    f.press(cx, "ctrl-z");
    f.update(cx, |this, _, cx| {
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "🙂 한글12\r\n한글3\rkeep\n"
        )
    });
}

#[gpui::test]
fn rename_cancel_and_confirm_preserve_editor_and_body(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        this.begin_rename(f.ids[0], window, cx);
        this.active_editor().unwrap()
    });
    f.input(cx, "취소할 제목");
    f.press(cx, "escape");
    f.update(cx, |this, window, cx| {
        assert_eq!(this.book.get(f.ids[0]).unwrap().title, "첫 메모");
        this.begin_rename(f.ids[0], window, cx);
    });
    f.input(cx, "  변경된 제목  ");
    f.press(cx, "enter");
    f.update(cx, |this, _, cx| {
        assert_eq!(this.book.get(f.ids[0]).unwrap().title, "변경된 제목");
        assert_eq!(
            this.active_editor().unwrap().entity_id(),
            editor.entity_id()
        );
        assert_eq!(editor.read(cx).value(), "🙂 한글12\r\n한글3\rkeep\n");
        assert!(!this.open[0].dirty);
    });
    assert_eq!(
        Notebook::load(&f.dir).unwrap().get(f.ids[0]).unwrap().title,
        "변경된 제목"
    );
}

#[gpui::test]
fn tree_multiselect_does_not_steal_editor_focus_or_active_note(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.tree_click(f.ids[0], Default::default(), window, cx);
        this.tree_click(
            f.ids[1],
            gpui::Modifiers {
                control: true,
                ..Default::default()
            },
            window,
            cx,
        );
        assert_eq!(this.selection.ids.len(), 2);
        assert_eq!(this.session.active, Some(f.ids[0]));
        assert!(this.tree_focus.is_focused(window));
    });
    f.press(cx, "enter");
    f.update(cx, |this, window, cx| {
        use gpui::Focusable;
        assert_eq!(this.session.active, Some(f.ids[1]));
        assert!(
            this.active_editor()
                .unwrap()
                .focus_handle(cx)
                .is_focused(window)
        );
    });
}

#[gpui::test]
fn close_reorder_and_delete_preserve_surviving_editors_and_files(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        let first = this.active_editor().unwrap();
        this.open_note(f.ids[1], false, window, cx);
        let second = this.active_editor().unwrap();
        this.open_note(f.ids[2], false, window, cx);
        this.reorder_tab(f.ids[2], 0, cx);
        assert_eq!(
            Session::load(&f.dir, &this.book).tabs,
            vec![f.ids[2], f.ids[0], f.ids[1]]
        );
        this.close_tabs(f.ids[1], 0, window, cx);
        assert!(this.book.file(&f.dir, f.ids[1]).exists());
        assert_eq!(
            this.open
                .iter()
                .find(|tab| tab.id == f.ids[0])
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .entity_id(),
            first.entity_id()
        );
        this.open_note(f.ids[1], false, window, cx);
        assert_ne!(
            this.active_editor().unwrap().entity_id(),
            second.entity_id()
        );
        let surviving = this.active_editor().unwrap();
        assert!(this.delete_confirmed(&[f.ids[0]], window, cx));
        assert!(this.book.get(f.ids[0]).is_none());
        assert!(this.book.get(f.ids[2]).is_none());
        assert_eq!(
            this.active_editor().unwrap().entity_id(),
            surviving.entity_id()
        );
    });
    assert_eq!(fs::read_dir(f.dir.join("trash")).unwrap().count(), 2);
}

#[gpui::test]
fn failed_save_retains_dirty_editor_and_blocks_closing(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.replace_all("미저장 본문", window, cx));
        this.changed(f.ids[0], window, cx);
        fs::create_dir(this.book.file(&f.dir, f.ids[0]).with_extension("tmp")).unwrap();
        assert!(!this.save_all(cx));
        this.close_tabs(f.ids[0], 0, window, cx);
        assert!(this.open[0].dirty);
        assert!(this.error.is_some());
        assert_eq!(editor.read(cx).value(), "미저장 본문");
    });
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[0]))).unwrap(),
        "🙂 한글12\r\n한글3\rkeep\n"
    );
}

#[gpui::test]
fn stale_search_result_is_not_replaced_and_global_open_preserves_tree(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.selection.single(f.ids[0]);
        this.caret = Some(f.ids[0]);
        this.collapsed.insert(f.ids[0]);
        this.search_mode = true;
        this.global
            .query
            .update(cx, |s, cx| s.set_value("needle", window, cx));
        this.global
            .replacement
            .update(cx, |s, cx| s.set_value("changed", window, cx));
        this.open_match(
            MatchKey {
                id: f.ids[1],
                range: CharRange { start: 7, end: 13 },
            },
            window,
            cx,
        );
        assert_eq!(this.session.active, Some(f.ids[1]));
        assert!(this.selection.ids.contains(&f.ids[0]));
        assert!(this.collapsed.contains(&f.ids[0]));
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.replace_all("different needle", window, cx));
        this.replace_result(
            MatchKey {
                id: f.ids[1],
                range: CharRange { start: 7, end: 13 },
            },
            window,
            cx,
        );
        assert_eq!(editor.read(cx).value(), "different needle");
    });
}

#[gpui::test]
fn korean_ime_protocol_and_emoji_selection_are_atomic(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.set_value("A🙂Z", window, cx));
        this.focus_editor(window, cx);
        editor
    });
    cx.update_window(f.window, |_, window, cx| {
        window.render_frame(cx);
        let mut handler = ElementInputHandler::new(
            Bounds::new(point(px(270.), px(40.)), size(px(800.), px(650.))),
            editor.clone(),
        );
        handler.replace_and_mark_text_in_range(Some(1..3), "ㅎ", Some(1..1), window, cx);
        handler.replace_and_mark_text_in_range(None, "한", Some(1..1), window, cx);
        handler.replace_text_in_range(None, "한글", window, cx);
        assert_eq!(editor.read(cx).value(), "A한글Z");
        window.render_frame(cx);
        window.press("ctrl-z", cx);
        assert_eq!(editor.read(cx).value(), "A🙂Z");
    })
    .unwrap();
}

#[gpui::test]
fn delete_confirmation_defaults_to_cancel_and_keyboard_menu_dismisses(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.selection.single(f.ids[0]);
        this.caret = Some(f.ids[0]);
        this.tree_focus.focus(window, cx);
        this.confirm_delete(window, cx);
    });
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert!(this.book.get(f.ids[0]).is_some()));
    f.press(cx, "escape");
    f.update(cx, |this, window, cx| this.tree_focus.focus(window, cx));
    f.press(cx, "shift-f10");
    f.update(cx, |this, _, _| assert!(this.keyboard_menu.is_some()));
    f.press(cx, "escape");
    f.update(cx, |this, _, _| assert!(this.keyboard_menu.is_none()));
}

#[gpui::test]
fn text_metrics_track_utf16_columns_and_all_paragraph_endings(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.replace_all("🙂a\r\nb\rc\n", window, cx));
        this.changed(f.ids[0], window, cx);
        let metrics = &this.open[0].metrics;
        assert_eq!(metrics.total, 8);
        assert_eq!(metrics.line(7), (2, 7));
        assert_eq!(metrics.line(9), (3, 9));
    });
}

#[gpui::test]
fn control_wheel_zooms_instead_of_scrolling_a_long_document(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value("긴 본문\n".repeat(1000), window, cx)
        });
        this.focus_editor(window, cx);
        editor
    });
    cx.update_window(f.window, |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_event(
            gpui::PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                position: point(px(400.), px(200.)),
                delta: gpui::ScrollDelta::Lines(point(0., -3.)),
                modifiers: gpui::Modifiers {
                    control: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
            cx,
        );
    })
    .unwrap();
    f.update(cx, |this, _, cx| {
        assert_eq!(this.zoom, 90);
        assert_eq!(editor.read(cx).scroll_offset().y, px(0.));
    });
}

#[gpui::test]
fn global_replace_updates_open_and_closed_notes_and_preserves_undo(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        this.open_note(f.ids[1], true, window, cx);
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.replace_all("loaded needle", window, cx));
        this.changed(f.ids[1], window, cx);
        this.global
            .query
            .update(cx, |s, cx| s.set_value("needle", window, cx));
        this.global
            .replacement
            .update(cx, |s, cx| s.set_value("바뀜", window, cx));
        this.replace_global_all(window, cx);
        editor
    });
    f.press(cx, "tab");
    f.press(cx, "enter");
    f.update(cx, |this, window, cx| {
        assert_eq!(editor.read(cx).value(), "loaded 바뀜");
        assert!(
            !this
                .open
                .iter()
                .find(|tab| tab.id == f.ids[1])
                .unwrap()
                .dirty
        );
        assert!(!this.open.iter().any(|tab| tab.id == f.ids[2]));
        this.focus_editor(window, cx);
    });
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[1]))).unwrap(),
        "loaded 바뀜"
    );
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[2]))).unwrap(),
        "child 바뀜\n"
    );
    f.press(cx, "ctrl-z");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "loaded needle")
    });
}

#[gpui::test]
fn editor_graphemes_crlf_clipboard_and_redo(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.set_value("A👨‍👩‍👧‍👦e\u{301}\r\n끝", window, cx));
        this.focus_editor(window, cx);
        editor
    });
    f.press(cx, "ctrl-home");
    f.press(cx, "right");
    f.press(cx, "right");
    f.press(cx, "backspace");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "Ae\u{301}\r\n끝")
    });
    f.press(cx, "ctrl-z");
    f.press(cx, "ctrl-y");
    f.press(cx, "right");
    f.press(cx, "delete");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "Ae\u{301}끝")
    });
    f.press(cx, "ctrl-a");
    f.press(cx, "ctrl-c");
    f.press(cx, "ctrl-end");
    f.press(cx, "ctrl-v");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "Ae\u{301}끝Ae\u{301}끝")
    });
    f.press(cx, "ctrl-z");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "Ae\u{301}끝")
    });
}

#[gpui::test]
fn editor_wrapped_navigation_and_scroll_reveal(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value(
                format!(
                    "{}\r\n{}",
                    "가나다라 ".repeat(100),
                    "마지막 줄\n".repeat(300)
                ),
                window,
                cx,
            )
        });
        this.focus_editor(window, cx);
        editor
    });
    f.press(cx, "ctrl-home");
    f.press(cx, "down");
    f.update(cx, |_, _, cx| {
        assert!(editor.read(cx).cursor() > 0);
        assert!(editor.read(cx).cursor() < "가나다라 ".repeat(100).len());
    });
    f.press(cx, "ctrl-end");
    f.press(cx, "left");
    f.update(cx, |_, _, cx| {
        assert!(editor.read(cx).scroll_offset().y < px(0.))
    });
    f.press(cx, "ctrl-home");
    f.press(cx, "right");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).scroll_offset().y, px(0.))
    });
}

#[gpui::test]
fn search_inputs_support_tab_focus_and_plain_text_only(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.show_find(false, true, window, cx)
    });
    f.input(cx, "needle");
    f.press(cx, "tab");
    // Search option buttons are also keyboard reachable; continue to the replacement input.
    for _ in 0..8 {
        let focused = f.update(cx, |this, window, cx| {
            gpui::Focusable::focus_handle(this.local.replacement.read(cx), cx).is_focused(window)
        });
        if focused {
            break;
        }
        f.press(cx, "tab");
    }
    f.update(cx, |this, window, cx| {
        assert!(
            gpui::Focusable::focus_handle(this.local.replacement.read(cx), cx).is_focused(window)
        );
    });
    f.input(cx, "replacement");
    f.update(cx, |this, _, cx| {
        assert_eq!(this.local.query.read(cx).value(), "needle");
        assert_eq!(this.local.replacement.read(cx).value(), "replacement");
    });
}
