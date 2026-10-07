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
        if matches!(stroke.key.as_str(), "enter" | "tab" | "space") {
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
fn wrap_switch_animates_reverses_and_respects_reduced_motion(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| this.show_settings(window, cx));
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let right = visual.debug_bounds("settings-wrap-thumb").unwrap().left();

    f.update(cx, |this, window, cx| {
        let mut settings = this.settings.clone();
        settings.wrap = !settings.wrap;
        this.change_settings(settings, window, cx);
    });
    visual.update(|window, cx| window.render_frame(cx));
    let start = visual.debug_bounds("settings-wrap-thumb").unwrap().left();
    assert!(start <= right && start > right - px(3.));
    visual.update(|window, cx| window.render_frame(cx));
    let middle = visual.debug_bounds("settings-wrap-thumb").unwrap().left();
    assert!(middle < right - px(1.) && middle > right - px(17.));

    f.update(cx, |this, window, cx| {
        let mut settings = this.settings.clone();
        settings.wrap = !settings.wrap;
        this.change_settings(settings, window, cx);
    });
    visual.update(|window, cx| window.render_frame(cx));
    let reversed = visual.debug_bounds("settings-wrap-thumb").unwrap().left();
    assert!(reversed < right && reversed > right - px(18.));
    for _ in 0..60 {
        visual.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        });
    }
    assert!((visual.debug_bounds("settings-wrap-thumb").unwrap().left() - right).abs() < px(0.1));

    cx.update(|cx| cx.set_reduce_motion(true));
    f.update(cx, |this, window, cx| {
        let mut settings = this.settings.clone();
        settings.wrap = !settings.wrap;
        this.change_settings(settings, window, cx);
    });
    visual.update(|window, cx| window.render_frame(cx));
    assert!(
        (visual.debug_bounds("settings-wrap-thumb").unwrap().left() - (right - px(18.))).abs()
            < px(0.1)
    );
    f.update(cx, |this, _, _| assert!(!this.settings.wrap));
}

#[gpui::test]
fn settings_row_units_and_wrap_switch_work_without_desktop_input(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        this.show_settings(window, cx);
        let size = this.preferences.as_ref().unwrap().size.clone();
        size.update(cx, |state, cx| state.focus(window, cx));
        this.active_editor().unwrap()
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let font = visual.debug_bounds("settings-font").unwrap();
    let size = visual.debug_bounds("settings-size").unwrap();
    let panel = visual.debug_bounds("settings-panel").unwrap();
    assert_eq!(font.top(), size.top());
    assert!(font.right() < size.left());
    assert!(size.right() < panel.right());
    f.press(cx, "enter");
    f.press(cx, "down");
    f.press(cx, "enter");
    f.update(cx, |this, _, _| {
        assert_eq!(this.settings.font_size, 19);
        assert_eq!(Settings::load(&f.dir).unwrap().font_size, 19);
    });
    f.press(cx, "tab");
    f.update(cx, |this, window, _| {
        assert!(
            this.preferences
                .as_ref()
                .unwrap()
                .wrap_focus
                .is_focused(window)
        );
    });
    f.press(cx, "space");
    f.update(cx, |this, _, cx| {
        assert!(!this.settings.wrap);
        assert!(!Settings::load(&f.dir).unwrap().wrap);
        assert_eq!(this.active_editor().unwrap(), editor);
        assert_eq!(editor.read(cx).value(), "🙂 한글12\r\n한글3\rkeep\n");
    });
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert!(this.settings.wrap));
    visual.update(|window, cx| window.render_frame(cx));
    let wrap = visual.debug_bounds("settings-wrap").unwrap();
    visual.simulate_click(wrap.center(), gpui::Modifiers::default());
    f.update(cx, |this, _, _| {
        assert!(!this.settings.wrap);
        assert!(!Settings::load(&f.dir).unwrap().wrap);
    });
}

#[gpui::test]
fn system_theme_uses_gpui_appearance_and_keeps_editor_state(cx: &mut TestAppContext) {
    use crate::widgets::ActiveTheme;
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        assert_eq!(window.appearance(), gpui::WindowAppearance::Light);
        let editor = this.active_editor().unwrap();
        let content = editor.read(cx).value();
        let selection = editor.read(cx).selected_range();

        let mut settings = this.settings.clone();
        settings.theme = ThemeMode::Dark;
        this.change_settings(settings, window, cx);
        assert_eq!(cx.theme().background, gpui::Hsla::from(gpui::rgb(0x1e2127)));
        assert_eq!(cx.theme().bar, gpui::Hsla::from(gpui::rgb(0x15181d)));

        let mut settings = this.settings.clone();
        settings.theme = ThemeMode::System;
        this.change_settings(settings, window, cx);
        assert_eq!(cx.theme().background, gpui::Hsla::from(gpui::rgb(0xffffff)));
        assert_eq!(cx.theme().bar, gpui::Hsla::from(gpui::rgb(0xf1f4f8)));
        assert_eq!(Settings::load(&f.dir).unwrap().theme, ThemeMode::System);
        assert_eq!(this.active_editor().unwrap(), editor);
        assert_eq!(editor.read(cx).value(), content);
        assert_eq!(editor.read(cx).selected_range(), selection);
    });
    cx.update_window(f.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[gpui::test]
fn font_picker_preserves_a_family_missing_from_gpui_fonts(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let missing_font = "Edot Test Missing Font";
    let picker = f.update(cx, |this, window, cx| {
        assert!(this.preferences.is_none());
        assert!(
            !cx.text_system()
                .all_font_names()
                .iter()
                .any(|name| name == missing_font)
        );
        let mut settings = this.settings.clone();
        settings.font = missing_font.into();
        this.change_settings(settings, window, cx);
        this.show_settings(window, cx);
        let picker = this.preferences.as_ref().unwrap().font.clone();
        let mut settings = this.settings.clone();
        settings.font = "Consolas".into();
        this.change_settings(settings, window, cx);
        picker.update(cx, |state, cx| state.focus(window, cx));
        picker
    });
    f.press(cx, "enter");
    f.input(cx, missing_font);
    f.press(cx, "enter");
    f.update(cx, |this, window, cx| {
        assert_eq!(this.settings.font, missing_font);
        assert_eq!(Settings::load(&f.dir).unwrap().font, missing_font);
        this.show_settings(window, cx);
        this.show_settings(window, cx);
        assert_eq!(this.preferences.as_ref().unwrap().font, picker);
    });
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

#[gpui::test]
fn dropdown_toggle_selection_scroll_and_dismissal(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.settings.font_size = 40;
        this.show_settings(window, cx);
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let trigger = visual.debug_bounds("settings-size").unwrap().center();
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    let options = visual.debug_bounds("select-options").unwrap();
    let selected = visual.debug_bounds("select-highlight").unwrap();
    assert!(
        selected.top() >= options.top() && selected.bottom() <= options.bottom(),
        "selected={selected:?} options={options:?}"
    );
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert_eq!(this.settings.font_size, 40));
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_some());
    visual.simulate_mouse_down(trigger, gpui::MouseButton::Left, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_some());
    visual.simulate_mouse_up(trigger, gpui::MouseButton::Left, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_none());
    f.press(cx, "enter");
    f.press(cx, "escape");
    f.update(cx, |this, _, _| assert!(this.settings_visible));
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_none());
    f.press(cx, "enter");
    f.press(cx, "tab");
    f.update(cx, |this, w, _| {
        assert!(this.preferences.as_ref().unwrap().wrap_focus.is_focused(w))
    });
}

#[gpui::test]
fn splitter_edges_capture_drag_and_release(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    for edge in [0.5, 5.5] {
        visual.update(|w, cx| w.render_frame(cx));
        let bounds = visual.debug_bounds("sidebar-resize").unwrap();
        let line = visual.debug_bounds("sidebar-resize-line").unwrap();
        assert_eq!(bounds.center().x, line.center().x);
        let start = point(bounds.left() + px(edge), bounds.center().y);
        let width = f.update(cx, |this, _, _| this.session.sidebar_width);
        visual.simulate_mouse_move(start, None, gpui::Modifiers::default());
        visual.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
        visual.update(|w, cx| w.render_frame(cx));
        assert!(visual.debug_bounds("sidebar-resize-capture").is_some());
        let end = start + point(px(80.), px(100.));
        visual.simulate_mouse_move(
            end,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        visual.update(|w, cx| w.render_frame(cx));
        f.update(cx, |this, _, _| {
            assert_eq!(this.session.sidebar_width, width + 80);
            assert!(this.resizing.is_some());
        });
        visual.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
        visual.update(|w, cx| w.render_frame(cx));
        assert!(visual.debug_bounds("sidebar-resize-capture").is_none());
        visual.simulate_mouse_move(
            end + point(px(50.), px(0.)),
            None,
            gpui::Modifiers::default(),
        );
        f.update(cx, |this, _, _| {
            assert_eq!(this.session.sidebar_width, width + 80)
        });
    }
}

#[gpui::test]
fn tab_scrollbar_does_not_activate_tabs_and_titles_keep_space(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, w, cx| {
        for i in 0..12 {
            let id = this.book.add(0, format!("아주 긴 탭 제목 여러 단어 {i}"));
            fs::write(this.book.file(&f.dir, id), "").unwrap();
            assert!(this.open_note(id, true, w, cx));
        }
        this.open_note(f.ids[0], true, w, cx);
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    for _ in 0..2 {
        visual.update(|w, cx| w.render_frame(cx));
    }
    let tab = visual.debug_bounds("active-tab").unwrap();
    let title = visual.debug_bounds("active-tab-title").unwrap();
    assert!(tab.size.width >= px(140.));
    assert!(title.size.width >= px(60.));
    assert!(title.size.height < px(30.));
    let thumb = visual.debug_bounds("h-scrollbar").unwrap();
    let start = point(thumb.left() + px(3.), thumb.center().y);
    let before = f.update(cx, |this, _, _| {
        assert!(
            this.tab_scroll.max_offset().x > px(0.),
            "tabs={} max={:?} bounds={:?}",
            this.open.len(),
            this.tab_scroll.max_offset(),
            thumb
        );
        this.tab_scroll.offset()
    });
    visual.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| {
        assert_eq!(this.tab_scroll.offset(), before)
    });
    let end = start + point(px(90.), px(120.));
    visual.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| {
        assert!(this.tab_scroll.offset().x < before.x);
        assert_eq!(this.session.active, Some(f.ids[0]));
    });
    visual.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    let after = f.update(cx, |this, _, _| this.tab_scroll.offset());
    visual.simulate_mouse_move(
        end + point(px(100.), px(0.)),
        None,
        gpui::Modifiers::default(),
    );
    f.update(cx, |this, _, _| assert_eq!(this.tab_scroll.offset(), after));
}

#[gpui::test]
fn editor_scrollbar_preserves_grab_point_and_text_selection(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value("long document\n".repeat(300), w, cx);
            s.set_selected_range(0..4, cx);
        });
        editor
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let bounds = visual.debug_bounds("text-input").unwrap();
    let start = point(bounds.right() - px(5.), bounds.top() + px(3.));
    visual.simulate_mouse_move(start, None, gpui::Modifiers::default());
    visual.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).scroll_offset().y, px(0.))
    });
    let end = start + point(px(-140.), px(100.));
    visual.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |_, _, cx| {
        assert!(editor.read(cx).scroll_offset().y < px(0.));
        assert_eq!(editor.read(cx).selected_range(), 0..4);
    });
    visual.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    let offset = f.update(cx, |_, _, cx| editor.read(cx).scroll_offset());
    visual.simulate_mouse_move(
        end + point(px(0.), px(100.)),
        None,
        gpui::Modifiers::default(),
    );
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).scroll_offset(), offset)
    });
}

#[gpui::test]
fn dropdown_empty_search_keeps_value_and_reopen_resets_filter(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let font = f.update(cx, |this, w, cx| {
        this.show_settings(w, cx);
        this.settings.font.clone()
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let trigger = visual.debug_bounds("settings-font").unwrap().center();
    visual.simulate_click(trigger, gpui::Modifiers::default());
    f.input(cx, "edot nonexistent font qwerty");
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-highlight").is_none());
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert_eq!(this.settings.font, font));
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_some());
    f.press(cx, "escape");
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-highlight").is_some());
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert_eq!(this.settings.font, font));
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    let outside = visual.debug_bounds("settings-panel").unwrap().origin + point(px(8.), px(8.));
    visual.simulate_click(outside, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_none());
    f.update(cx, |this, _, _| assert!(this.settings_visible));
}

#[gpui::test]
fn body_padding_scrolls_inside_the_full_editor_clip(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_soft_wrap(false, w, cx);
            s.set_value(
                format!("{}\n", "가나다 ABC 123 ".repeat(100)).repeat(100),
                w,
                cx,
            );
        });
        editor
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    for scale in [1., 1.25, 1.5, 2.] {
        visual.simulate_scale_factor_change(scale);
        f.update(cx, |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_soft_wrap(false, w, cx);
                s.set_scroll_offset(point(px(0.), px(0.)), cx);
            })
        });
        visual.update(|w, cx| w.render_frame(cx));
        let body = visual.debug_bounds("body-editor").unwrap();
        f.update(cx, |_, w, cx| {
            editor.update(cx, |s, cx| {
                let initial = s.bounds_for_range(0..0, Bounds::default(), w, cx).unwrap();
                assert_eq!(initial.left(), body.left() + px(16.));
                assert_eq!(initial.top(), body.top() + px(6.));
                // Inspect the effective paint mask, not only layout coordinates.
                assert_eq!(s.paint_clip(), body);
            })
        });
        // Move the first character INTO what used to be the blank margins.
        for (delta, expected) in [
            (point(px(-12.), px(0.)), point(px(4.), px(6.))),
            (point(px(0.), px(-3.)), point(px(4.), px(3.))),
        ] {
            visual.simulate_event(gpui::ScrollWheelEvent {
                position: body.center(),
                delta: gpui::ScrollDelta::Pixels(delta),
                touch_phase: gpui::TouchPhase::Started,
                ..Default::default()
            });
            visual.update(|w, cx| w.render_frame(cx));
            f.update(cx, |_, w, cx| {
                editor.update(cx, |s, cx| {
                    let caret = s.bounds_for_range(0..0, Bounds::default(), w, cx).unwrap();
                    assert_eq!(caret.origin, body.origin + expected);
                    assert!(s.paint_clip().contains(&caret.origin));
                    assert_eq!(s.character_index_for_point(caret.origin, w, cx), Some(0));
                    assert_eq!(s.paint_clip(), body);
                })
            });
        }
        f.press(cx, "ctrl-home");
        f.update(cx, |_, _, cx| {
            assert_eq!(editor.read(cx).scroll_offset(), point(px(0.), px(0.)))
        });
        f.update(cx, |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_soft_wrap(true, w, cx);
                s.set_scroll_offset(point(px(0.), px(-3.)), cx);
            })
        });
        visual.update(|w, cx| w.render_frame(cx));
        f.update(cx, |_, w, cx| {
            editor.update(cx, |s, cx| {
                let caret = s.bounds_for_range(0..0, Bounds::default(), w, cx).unwrap();
                assert_eq!(caret.top(), body.top() + px(3.));
                assert!(s.paint_clip().contains(&caret.origin));
            })
        });
    }
}

#[gpui::test]
fn settings_close_only_when_focus_leaves_the_panel(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    cx.update_window(f.window, |_, w, _| w.activate_window())
        .unwrap();
    f.update(cx, |this, w, cx| this.show_settings(w, cx));
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    f.press(cx, "tab");
    f.update(cx, |this, _, _| assert!(this.settings_visible));
    let trigger = visual.debug_bounds("settings-font").unwrap().center();
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| assert!(this.settings_visible));
    assert!(visual.debug_bounds("select-popup").is_some());
    f.input(cx, "con");
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert!(this.settings_visible));
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    visual.simulate_click(point(px(440.), px(480.)), gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, w, cx| {
        use gpui::Focusable;
        assert!(
            this.active_editor()
                .unwrap()
                .read(cx)
                .focus_handle(cx)
                .is_focused(w),
            "focus did not reach body: {:?}",
            w.focused(cx)
        );
        assert!(!this.settings_visible);
        this.show_settings(w, cx);
    });
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_none());
    f.update(cx, |this, w, cx| this.tree_focus.focus(w, cx));
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, w, _| {
        assert!(!this.settings_visible);
        assert!(this.tree_focus.is_focused(w));
    });
    f.update(cx, |this, w, cx| this.show_settings(w, cx));
    visual.update(|w, cx| w.render_frame(cx));
    // Leaving the final setting by keyboard also dismisses the panel.
    f.update(cx, |this, w, cx| {
        this.preferences.as_ref().unwrap().wrap_focus.focus(w, cx)
    });
    f.press(cx, "tab");
    f.update(cx, |this, _, _| assert!(!this.settings_visible));
}

#[gpui::test]
fn settings_close_when_window_deactivates(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, _| w.activate_window());
    f.update(cx, |this, w, cx| this.show_settings(w, cx));
    visual.update(|w, cx| w.render_frame(cx));
    visual.deactivate_window();
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| assert!(!this.settings_visible));
}

#[gpui::test]
fn splitter_painted_hover_cursor_and_click_share_hitbox_at_each_scale(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    for scale in [1., 1.25, 1.5, 2.] {
        visual.simulate_scale_factor_change(scale);
        visual.update(|w, cx| w.render_frame(cx));
        let bounds = visual.debug_bounds("sidebar-resize").unwrap();
        for dx in [-2., -0.2, 0.2, 1., 2.9, 3.1, 5., 5.8, 6.8, 8.] {
            let position = point(bounds.left() + px(dx), bounds.center().y);
            visual.simulate_mouse_move(position, None, gpui::Modifiers::default());
            visual.update(|w, cx| w.render_frame(cx));
            let expected = bounds.contains(&position);
            f.update(cx, |this, w, _| {
                let probe = this.splitter_probe.borrow();
                let (hitbox, highlighted, cursor) = probe.as_ref().unwrap();
                assert_eq!(*highlighted, expected, "scale={scale} dx={dx}");
                assert_eq!(hitbox.is_hovered_at(position, w), expected);
                assert_eq!(*cursor, gpui::CursorStyle::ResizeColumn);
            });
            visual.simulate_mouse_down(
                position,
                gpui::MouseButton::Left,
                gpui::Modifiers::default(),
            );
            f.update(cx, |this, _, _| {
                assert_eq!(this.resizing.is_some(), expected)
            });
            visual.simulate_mouse_up(
                position,
                gpui::MouseButton::Left,
                gpui::Modifiers::default(),
            );
            visual.update(|w, cx| w.render_frame(cx));
        }
    }
}

#[gpui::test]
fn vertical_scroll_noise_does_not_cut_the_first_character(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_soft_wrap(false, w, cx);
            s.set_value(
                format!("{}\n", "왼쪽 시작 text ".repeat(80)).repeat(100),
                w,
                cx,
            );
        });
        editor
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let viewport = visual.debug_bounds("text-input").unwrap();
    for _ in 0..20 {
        visual.simulate_event(gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(-0.75), px(-48.))),
            ..Default::default()
        });
        visual.update(|w, cx| w.render_frame(cx));
    }
    f.update(cx, |_, _, cx| {
        assert!(editor.read(cx).scroll_offset().y < px(0.));
        assert_eq!(editor.read(cx).scroll_offset().x, px(0.));
    });
}

#[gpui::test]
fn windows_shift_wheel_is_not_rotated_twice(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_soft_wrap(false, w, cx);
            s.set_value(format!("{}\n", "long text ".repeat(100)).repeat(100), w, cx);
        });
        editor
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let viewport = visual.debug_bounds("text-input").unwrap();
    visual.simulate_event(gpui::ScrollWheelEvent {
        position: viewport.center(),
        delta: gpui::ScrollDelta::Lines(point(-3., 0.)),
        modifiers: gpui::Modifiers {
            shift: true,
            ..Default::default()
        },
        ..Default::default()
    });
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |_, _, cx| {
        assert!(editor.read(cx).scroll_offset().x < px(0.));
        assert_eq!(editor.read(cx).scroll_offset().y, px(0.));
    });
}

fn settle_search(cx: &mut TestAppContext) {
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
}

#[gpui::test]
fn search_fields_center_text_and_paint_focus_border(cx: &mut TestAppContext) {
    use crate::widgets::ActiveTheme;
    use gpui::EntityInputHandler;
    let f = Fixture::new(cx);
    f.update(cx, |this, _, _| {
        this.settings.font_size = 40;
        this.zoom = 200;
    });
    for global in [false, true] {
        let (query, replacement) = f.update(cx, |this, w, cx| {
            this.show_find(global, true, w, cx);
            let panel = if global { &this.global } else { &this.local };
            (panel.query.clone(), panel.replacement.clone())
        });
        let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
        for scale in [1., 1.25, 1.5, 2.] {
            visual.simulate_scale_factor_change(scale);
            for (entity, label) in [
                (
                    &query,
                    if global {
                        "전체 메모 검색어"
                    } else {
                        "현재 메모 검색어"
                    },
                ),
                (
                    &replacement,
                    if global {
                        "전체 메모 바꿀 내용"
                    } else {
                        "바꿀 내용"
                    },
                ),
            ] {
                f.update(cx, |_, w, cx| entity.update(cx, |s, cx| s.focus(w, cx)));
                visual.update(|w, cx| w.render_frame(cx));
                let field = visual.debug_bounds(label).unwrap();
                assert_eq!(field.size.height, px(32.));
                f.update(cx, |_, w, cx| {
                    let caret = entity.update(cx, |s, cx| {
                        s.bounds_for_range(0..0, Bounds::default(), w, cx).unwrap()
                    });
                    assert!(
                        (f32::from(caret.center().y - field.center().y)).abs() < 0.6,
                        "field={field:?} caret={caret:?}"
                    );
                    assert!(caret.top() > field.top() && caret.bottom() < field.bottom());
                    let border = w
                        .painted_quads()
                        .into_iter()
                        .find(|q| {
                            q.bounds == field.scale(scale)
                                && q.border_widths.top > gpui::ScaledPixels(0.)
                        })
                        .unwrap();
                    assert_eq!(border.border_color, cx.theme().primary);
                });
                f.update(cx, |this, w, cx| this.focus_editor(w, cx));
                visual.update(|w, cx| w.render_frame(cx));
                f.update(cx, |_, w, cx| {
                    let border = w
                        .painted_quads()
                        .into_iter()
                        .find(|q| {
                            q.bounds == field.scale(scale)
                                && q.border_widths.top > gpui::ScaledPixels(0.)
                        })
                        .unwrap();
                    assert_eq!(border.border_color, cx.theme().border);
                });
            }
        }
        if global {
            let field = visual.debug_bounds("전체 메모 검색어").unwrap();
            let replace = visual.debug_bounds("전체 메모 바꿀 내용").unwrap();
            assert_eq!(field.left(), replace.left());
            assert!(field.bottom() < replace.top());
            // Escape returns focus to the body without closing global search.
            f.press(cx, "escape");
            f.update(cx, |this, _, _| assert!(this.search_mode));
        }
    }
}

#[gpui::test]
fn search_result_first_click_survives_ime_confirmation(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let query = f.update(cx, |this, w, cx| {
        this.open_note(f.ids[1], true, w, cx);
        this.show_find(true, false, w, cx);
        this.global.query.clone()
    });
    cx.update_window(f.window, |_, w, cx| {
        let mut handler = ElementInputHandler::new(Bounds::default(), query.clone());
        handler.replace_and_mark_text_in_range(None, "한글", Some(2..2), w, cx);
    })
    .unwrap();
    settle_search(cx);
    let generation = f.update(cx, |this, _, _| {
        assert!(!this.scanning);
        assert_eq!(this.rows[0].count, 2);
        this.search_generation
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let result = visual.debug_bounds("result-1").unwrap().center();
    visual.simulate_mouse_down(result, gpui::MouseButton::Left, gpui::Modifiers::default());
    // Windows may confirm the composition as focus leaves the query.
    visual.update(|w, cx| {
        let mut handler = ElementInputHandler::new(Bounds::default(), query.clone());
        handler.replace_text_in_range(None, "한글", w, cx);
        w.render_frame(cx);
    });
    visual.simulate_mouse_up(result, gpui::MouseButton::Left, gpui::Modifiers::default());
    f.update(cx, |this, _, cx| {
        assert_eq!(this.search_generation, generation);
        assert!(!this.scanning);
        assert_eq!(this.session.active, Some(f.ids[0]));
        assert_eq!(
            this.active_editor().unwrap().read(cx).selected_value(),
            "한글"
        );
    });
}

#[gpui::test]
fn search_edit_updates_one_note_and_preserves_other_results(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.set_value("needle first\n".repeat(20), w, cx));
        fs::write(
            this.book.file(&this.dir, f.ids[1]),
            "needle other\n".repeat(80),
        )
        .unwrap();
        this.show_find(true, false, w, cx);
        this.global
            .query
            .update(cx, |s, cx| s.set_value("needle", w, cx));
        editor
    });
    settle_search(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let (generation, other_preview, anchor, selected, offset) = f.update(cx, |this, _, _| {
        this.result_collapsed.insert(f.ids[2]);
        this.rebuild_results();
        let group = this.rows.iter().position(|r| r.id == f.ids[1]).unwrap();
        let index = this
            .result_rows
            .iter()
            .position(|r| r.group == group && r.hit == Some(10))
            .unwrap();
        this.result_cursor = Some(index);
        let selected = this.result_identity(index).unwrap();
        let top = index - 2;
        let offset = point(
            px(0.),
            -px(top as f32 * crate::global_search::ROW_HEIGHT as f32 + 3.),
        );
        this.result_scroll.0.borrow().base_handle.set_offset(offset);
        (
            this.search_generation,
            this.rows[group].matches[0].preview.clone(),
            this.result_identity(top).unwrap(),
            selected,
            offset,
        )
    });
    f.update(cx, |_, w, cx| {
        editor.update(cx, |s, cx| {
            s.replace_all("needle changed\n".repeat(25), w, cx)
        })
    });
    f.update(cx, |this, _, _| {
        assert!(!this.scanning);
        assert_eq!(this.rows[0].count, 20, "keep old results during debounce");
        assert_eq!(this.search_generation, generation);
    });
    settle_search(cx);
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| {
        assert_eq!(this.rows[0].count, 25);
        let other = this.rows.iter().find(|r| r.id == f.ids[1]).unwrap();
        assert!(
            std::sync::Arc::ptr_eq(&other.matches[0].preview, &other_preview),
            "unmodified note was rescanned"
        );
        assert_eq!(
            this.result_cursor.and_then(|i| this.result_identity(i)),
            Some(selected)
        );
        assert!(this.result_collapsed.contains(&f.ids[2]));
        let new_offset = this.result_scroll.0.borrow().base_handle.offset();
        assert_eq!(
            new_offset.y,
            offset.y - px(5. * crate::global_search::ROW_HEIGHT as f32)
        );
        let top =
            (f32::from(-new_offset.y) / crate::global_search::ROW_HEIGHT as f32).floor() as usize;
        assert_eq!(this.result_identity(top), Some(anchor));
    });
    // Rapid edits use the latest revision; removing/re-adding a group preserves document order.
    f.update(cx, |_, w, cx| {
        editor.update(cx, |s, cx| s.replace_all("none", w, cx))
    });
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert!(!this.rows.iter().any(|r| r.id == f.ids[0]))
    });
    f.update(cx, |_, w, cx| {
        editor.update(cx, |s, cx| s.replace_all("needle stale needle", w, cx))
    });
    f.update(cx, |_, w, cx| {
        editor.update(cx, |s, cx| s.replace_all("needle latest", w, cx))
    });
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert_eq!(this.rows[0].id, f.ids[0]);
        assert_eq!(this.rows[0].count, 1);
        assert!(this.rows[0].matches[0].preview.contains("latest"));
        assert_eq!(this.search_generation, generation);
    });
    // A new query cancels any old per-note job.
    f.update(cx, |this, w, cx| {
        editor.update(cx, |s, cx| s.replace_all("needle queued", w, cx));
        this.global
            .query
            .update(cx, |s, cx| s.set_value("other", w, cx));
    });
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert_eq!(this.rows.len(), 1);
        assert_eq!(this.rows[0].id, f.ids[1]);
    });
}

#[gpui::test]
fn search_edits_during_full_scan_and_in_multiple_notes_use_latest_text(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let (first, second) = f.update(cx, |this, w, cx| {
        let first = this.active_editor().unwrap();
        this.open_note(f.ids[1], true, w, cx);
        let second = this.active_editor().unwrap();
        this.show_find(true, false, w, cx);
        this.global
            .query
            .update(cx, |s, cx| s.set_value("needle", w, cx));
        (first, second)
    });
    f.update(cx, |this, w, cx| {
        assert!(this.scanning);
        first.update(cx, |s, cx| s.replace_all("needle during scan", w, cx));
        second.update(cx, |s, cx| s.replace_all("needle second", w, cx));
    });
    settle_search(cx);
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert!(!this.scanning);
        assert!(this.pending_search_notes.is_empty());
        assert_eq!(this.rows.len(), 3);
        assert!(this.rows[0].matches[0].preview.contains("during scan"));
    });
    f.update(cx, |_, w, cx| {
        first.update(cx, |s, cx| s.replace_all("needle one needle two", w, cx));
        second.update(cx, |s, cx| s.replace_all("no match", w, cx));
    });
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert_eq!(this.rows.len(), 2);
        assert_eq!(this.rows[0].id, f.ids[0]);
        assert_eq!(this.rows[0].count, 2);
        assert_eq!(this.rows[1].id, f.ids[2]);
    });
    // An externally changed, unopened note is reconciled without clearing other groups.
    f.update(cx, |this, w, cx| {
        fs::write(
            this.book.file(&this.dir, f.ids[2]),
            "updated needle twice needle",
        )
        .unwrap();
        this.open_match(
            MatchKey {
                id: f.ids[2],
                range: CharRange { start: 6, end: 12 },
            },
            w,
            cx,
        );
        assert_eq!(this.rows.len(), 2);
    });
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert_eq!(this.rows[1].count, 2);
        assert!(this.rows[1].matches[0].preview.contains("updated"));
    });
}

#[gpui::test]
fn search_preview_limit_refills_after_a_large_group_is_removed(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value(
                "needle\n".repeat(crate::global_search::MAX_RESULTS + 1),
                w,
                cx,
            )
        });
        this.show_find(true, false, w, cx);
        this.global
            .query
            .update(cx, |s, cx| s.set_value("needle", w, cx));
        editor
    });
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert_eq!(
            this.rows.iter().map(|r| r.matches.len()).sum::<usize>(),
            crate::global_search::MAX_RESULTS
        );
        assert_eq!(this.rows[1].matches.len(), 0);
    });
    f.update(cx, |_, w, cx| {
        editor.update(cx, |s, cx| s.replace_all("removed", w, cx))
    });
    for _ in 0..3 {
        settle_search(cx);
    }
    f.update(cx, |this, _, _| {
        assert_eq!(this.rows.len(), 2);
        assert!(
            this.rows
                .iter()
                .all(|r| r.count == 1 && r.matches.len() == 1)
        );
        assert!(this.note_search_tasks.is_empty());
    });
}
