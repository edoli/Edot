use super::*;

#[gpui::test]
fn macos_app_commands_preserve_system_tab_switching_and_replace_shortcuts(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let press = |key: &str, cx: &mut TestAppContext| {
        f.update(cx, |this, window, cx| {
            this.key_down_on(
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse(key).unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                true,
                window,
                cx,
            )
        });
    };
    press("cmd-tab", cx);
    f.update(cx, |this, _, _| {
        assert_eq!(this.session.active, Some(f.ids[0]))
    });
    press("ctrl-tab", cx);
    f.update(cx, |this, _, _| {
        assert_eq!(this.session.active, Some(f.ids[1]))
    });
    press("cmd-\\", cx);
    f.update(cx, |this, _, _| assert!(!this.session.sidebar_visible));
    press("cmd-h", cx);
    f.update(cx, |this, _, _| assert!(!this.find_visible));
    press("cmd-alt-f", cx);
    f.update(cx, |this, _, _| {
        assert!(this.find_visible);
        assert!(this.local.replace);
    });
}

#[gpui::test]
fn app_shortcuts_leave_altgr_and_composing_title_input_to_the_platform(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        let count = this.book.notes.len();
        let wrap = cx.global::<AppState>().settings.wrap;
        let mut altgr = gpui::Keystroke::parse("alt-z").unwrap();
        altgr.key_char = Some("ž".into());
        this.key_down(
            &gpui::KeyDownEvent {
                keystroke: altgr,
                is_held: false,
                prefer_character_input: false,
            },
            window,
            cx,
        );
        assert_eq!(cx.global::<AppState>().settings.wrap, wrap);
        for (key, prefer_character_input) in [("ctrl-alt-n", false), ("ctrl-n", true)] {
            let event = gpui::KeyDownEvent {
                keystroke: gpui::Keystroke::parse(key).unwrap(),
                is_held: false,
                prefer_character_input,
            };
            this.key_down(&event, window, cx);
            assert_eq!(this.book.notes.len(), count);
        }
        this.begin_rename_note(f.ids[0], window, cx);
        let input = this.title_input.clone();
        let mut handler = ElementInputHandler::new(Bounds::default(), input);
        handler.replace_and_mark_text_in_range(None, "조합", Some(2..2), window, cx);
        let enter = gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse("enter").unwrap(),
            is_held: false,
            prefer_character_input: false,
        };
        this.key_down(&enter, window, cx);
        assert!(this.rename.is_some());
        handler.replace_text_in_range(None, "완료", window, cx);
        this.key_down(&enter, window, cx);
        assert!(this.rename.is_none());
    });
}

#[gpui::test]
fn markdown_shortcuts_toggle_formats_preserve_history_and_update_app_modes(
    cx: &mut TestAppContext,
) {
    use crate::widgets::TextMode;
    use gpui::InputEvent as _;
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        this.focus_editor(window, cx);
        this.active_editor().unwrap()
    });
    cx.update_window(f.window, |_, window, cx| {
        window.render_frame(cx);
        for (key, marker) in [
            ("ctrl-b", "**"),
            ("ctrl-i", "*"),
            ("ctrl-shift-`", "~~"),
            ("ctrl-~", "~~"), // Windows normalizes Shift+backtick to '~'.
            ("ctrl-`", "`"),
        ] {
            let original = "앞 한글🙂 뒤";
            let start = "앞 ".len();
            let end = start + "한글🙂".len();
            editor.update(cx, |s, cx| {
                s.set_value(original, window, cx);
                s.set_selected_range(start..end, cx);
            });
            let sidebar = f.view.read(cx).session.sidebar_visible;
            window.press(key, cx);
            let formatted = format!("앞 {marker}한글🙂{marker} 뒤");
            assert_eq!(editor.read(cx).value(), formatted);
            assert_eq!(editor.read(cx).selected_value(), "한글🙂");
            assert_eq!(f.view.read(cx).session.sidebar_visible, sidebar);
            window.press("ctrl-z", cx);
            assert_eq!(editor.read(cx).value(), original);
            assert_eq!(editor.read(cx).selected_range(), start..end);
            window.press("ctrl-y", cx);
            assert_eq!(editor.read(cx).value(), formatted);
            assert_eq!(editor.read(cx).selected_value(), "한글🙂");
            window.press(key, cx);
            assert_eq!(editor.read(cx).value(), original);
            assert_eq!(editor.read(cx).selected_range(), start..end);
            editor.update(cx, |s, cx| {
                s.set_value("", window, cx);
            });
            window.press(key, cx);
            assert_eq!(editor.read(cx).value(), format!("{marker}{marker}"));
            assert_eq!(editor.read(cx).selected_range(), marker.len()..marker.len());
            window.press(key, cx);
            assert!(editor.read(cx).value().is_empty());
        }
        for (source, key, expected) in [
            ("**한글**", "ctrl-i", "***한글***"),
            ("***한글***", "ctrl-b", "*한글*"),
            ("~~한글~~", "ctrl-shift-`", "한글"),
            ("`한글`", "ctrl-`", "한글"),
        ] {
            editor.update(cx, |s, cx| {
                s.set_value(source, window, cx);
                let start = source.find("한글").unwrap();
                s.set_selected_range(start..start + "한글".len(), cx);
            });
            window.press(key, cx);
            assert_eq!(editor.read(cx).value(), expected);
            assert_eq!(editor.read(cx).selected_value(), "한글");
        }
        for source in ["a`b", "`앞", "끝`", " 앞 "] {
            editor.update(cx, |s, cx| {
                s.set_value(source, window, cx);
                s.select_all(window, cx);
            });
            window.press("ctrl-`", cx);
            assert_eq!(editor.read(cx).selected_value(), source);
            window.press("ctrl-`", cx);
            assert_eq!(editor.read(cx).value(), source);
        }
        // All selected occurrences are formatted in one undo transaction.
        editor.update(cx, |s, cx| {
            s.set_value("한글 + 한글", window, cx);
            s.set_selected_range(0..0, cx);
        });
        window.press("ctrl-d", cx);
        window.press("ctrl-d", cx);
        window.press("ctrl-b", cx);
        assert_eq!(editor.read(cx).value(), "**한글** + **한글**");
        assert_eq!(editor.read(cx).selected_ranges().count(), 2);
        assert!(
            editor
                .read(cx)
                .selected_ranges()
                .all(|r| &editor.read(cx).value()[r] == "한글")
        );
        window.press("ctrl-z", cx);
        assert_eq!(editor.read(cx).value(), "한글 + 한글");
        assert_eq!(editor.read(cx).selected_ranges().count(), 2);
        editor.update(cx, |s, cx| {
            s.set_value("plain", window, cx);
            s.select_all(window, cx);
            s.set_text_mode(TextMode::PlainText, cx);
        });
        for key in ["ctrl-b", "ctrl-i", "ctrl-shift-`", "ctrl-~", "ctrl-`"] {
            window.press(key, cx);
        }
        assert_eq!(editor.read(cx).value(), "plain");
    })
    .unwrap();
    let sidebar = f.update(cx, |this, _, _| this.session.sidebar_visible);
    f.press(cx, "ctrl-\\");
    f.update(cx, |this, _, _| {
        assert_eq!(this.session.sidebar_visible, !sidebar)
    });
    f.press(cx, "ctrl-\\");
    f.update(cx, |this, _, _| {
        assert_eq!(this.session.sidebar_visible, sidebar)
    });
    f.press(cx, "ctrl-shift-p");
    f.update(cx, |this, _, cx| {
        assert!(cx.global::<AppState>().settings.read_only);
        assert!(
            Settings::load(&cx.global::<AppState>().config_dir)
                .unwrap()
                .read_only
        );
        assert!(
            this.open
                .iter()
                .filter_map(|tab| tab.editor.as_ref())
                .all(|editor| editor.read(cx).is_read_only())
        );
    });
    f.update(cx, |_, _, cx| {
        editor.update(cx, |s, cx| s.set_text_mode(TextMode::Markdown, cx))
    });
    for key in ["ctrl-b", "ctrl-i", "ctrl-shift-`", "ctrl-~", "ctrl-`"] {
        f.press(cx, key);
    }
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), "plain"));
    f.press(cx, "ctrl-shift-p");
    f.update(cx, |_, _, cx| {
        assert!(!cx.global::<AppState>().settings.read_only);
        assert!(!editor.read(cx).is_read_only());
    });
    // Window shortcuts also work while an overlay owns keyboard focus.
    f.press(cx, "f1");
    f.press(cx, "f11");
    f.update(cx, |this, window, _| {
        assert!(window.is_fullscreen());
        assert!(this.shortcut_help.is_some());
    });
    f.press(cx, "ctrl-f11");
    cx.update_window(f.window, |_, window, cx| {
        window.dispatch_event(
            gpui::KeyDownEvent {
                keystroke: gpui::Keystroke::parse("f11").unwrap(),
                is_held: true,
                prefer_character_input: false,
            }
            .to_platform_input(),
            cx,
        );
        assert!(window.is_fullscreen());
    })
    .unwrap();
    f.press(cx, "f11");
    f.update(cx, |_, window, cx| {
        assert!(!window.is_fullscreen());
        assert_eq!(editor.read(cx).value(), "plain");
    });
}

#[gpui::test]
fn shortcut_search_filters_rows_and_preserves_input_and_editor_focus(cx: &mut TestAppContext) {
    use gpui::Focusable as _;
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        this.focus_editor(window, cx);
        this.active_editor().unwrap()
    });
    let original = editor.read_with(cx, |state, _| (state.value(), state.revision));
    f.press(cx, "f1");
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    let draw = |visual: &mut gpui::VisualTestContext| {
        visual.update(|window, cx| window.render_frame(cx));
    };
    draw(&mut visual);
    assert!(visual.debug_bounds("shortcut-search").is_some());
    cx.simulate_input(f.window, "F11");
    draw(&mut visual);
    assert!(visual.debug_bounds("shortcut-row-2").is_some());
    assert!(visual.debug_bounds("shortcut-row-1").is_none());
    // Home edits the query instead of scrolling the table.
    f.press(cx, "home");
    f.press(cx, "delete");
    cx.simulate_input(f.window, "F");
    draw(&mut visual);
    assert!(visual.debug_bounds("shortcut-row-2").is_some());
    f.press(cx, "ctrl-f");
    cx.simulate_input(f.window, "cTrL + ShIfT + p");
    draw(&mut visual);
    assert!(visual.debug_bounds("shortcut-row-12").is_some());
    assert!(visual.debug_bounds("shortcut-row-2").is_none());
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert!(this.shortcut_help.is_some()));
    f.press(cx, "ctrl-f");
    cx.simulate_input(f.window, "전체화면");
    draw(&mut visual);
    assert!(visual.debug_bounds("shortcut-row-2").is_some());
    f.press(cx, "ctrl-a");
    cx.simulate_input(f.window, "없는 기능입니다");
    draw(&mut visual);
    assert!(visual.debug_bounds("shortcut-empty").is_some());
    assert!(visual.debug_bounds("shortcut-row-2").is_none());
    f.press(cx, "ctrl-a");
    f.press(cx, "backspace");
    draw(&mut visual);
    assert!(visual.debug_bounds("shortcut-empty").is_none());
    assert!(visual.debug_bounds("shortcut-row-1").is_some());
    f.press(cx, "tab");
    f.press(cx, "end");
    f.update(cx, |this, _, cx| {
        assert!(
            this.shortcut_help
                .as_ref()
                .unwrap()
                .read(cx)
                .scroll
                .offset()
                .y
                < px(0.)
        );
    });
    f.press(cx, "shift-tab");
    cx.simulate_input(f.window, "F11");
    draw(&mut visual);
    assert!(visual.debug_bounds("shortcut-row-2").is_some());
    f.update(cx, |this, _, cx| {
        assert_eq!(
            this.shortcut_help
                .as_ref()
                .unwrap()
                .read(cx)
                .scroll
                .offset()
                .y,
            px(0.)
        );
    });
    f.press(cx, "escape");
    f.update(cx, |this, window, cx| {
        assert!(this.shortcut_help.is_none());
        assert!(editor.read(cx).focus_handle(cx).is_focused(window));
        assert_eq!(
            (editor.read(cx).value(), editor.read(cx).revision),
            original
        );
    });
    f.press(cx, "f1");
    draw(&mut visual);
    assert!(visual.debug_bounds("shortcut-row-1").is_some());
    f.press(cx, "f1");
    f.update(cx, |this, _, _| assert!(this.shortcut_help.is_none()));
}

#[gpui::test]
fn body_drag_selects_outside_and_suppresses_other_hover_until_release(cx: &mut TestAppContext) {
    use crate::widgets::{input::TextMode, theme::ActiveTheme as _};
    use gpui::{InputEvent as _, MouseButton, MouseMoveEvent};
    let f = Fixture::new(cx);
    let source = ("abcdefghij".repeat(80) + "\n\n").repeat(80);
    let editor = f.update(cx, |this, _, _| this.active_editor().unwrap());
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    let draw = |visual: &mut gpui::VisualTestContext| {
        visual.update(|w, cx| w.render_frame(cx));
    };
    draw(&mut visual);
    let selector: &'static str = format!("tree-note-{}", f.ids[1]).leak();
    let sidebar = visual.debug_bounds(selector).unwrap().center();
    let hover = |visual: &mut gpui::VisualTestContext| {
        visual.update(|w, cx| {
            w.painted_quads().iter().any(|q| {
                q.background == cx.theme().accent.into()
                    && q.bounds.contains(&sidebar.scale(w.scale_factor()))
            })
        })
    };
    for (mode, read_only, column, wrap) in [
        (TextMode::Markdown, false, false, true),
        (TextMode::Markdown, false, false, false),
        (TextMode::PlainText, true, false, false),
        (TextMode::PlainText, false, true, false),
        (TextMode::Markdown, true, false, false),
    ] {
        f.update(cx, |this, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_read_only(false, cx);
                s.set_value(source.clone(), w, cx);
                s.set_text_mode(mode, cx);
                s.set_soft_wrap(wrap, w, cx);
                s.set_read_only(read_only, cx);
            });
            this.focus_editor(w, cx);
        });
        draw(&mut visual);
        visual.simulate_mouse_move(sidebar, None, Default::default());
        draw(&mut visual);
        assert!(hover(&mut visual), "normal sidebar hover must work");
        let reading = read_only && mode == TextMode::Markdown;
        let body = visual.debug_bounds("body-editor").unwrap();
        let start = if reading {
            let block = visual.debug_bounds("reading-block-0").unwrap();
            point(block.left() + px(50.), block.top() + px(14.))
        } else {
            visual.update(|w, cx| {
                let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
                let bounds = handler.bounds_for_range(4..5, w, cx).unwrap();
                point(bounds.left() + px(0.1), bounds.center().y)
            })
        };
        let modifiers = gpui::Modifiers {
            alt: column,
            shift: column,
            ..Default::default()
        };
        visual.simulate_mouse_down(start, MouseButton::Left, modifiers);
        draw(&mut visual);
        visual.simulate_mouse_move(sidebar, MouseButton::Left, modifiers);
        draw(&mut visual);
        assert!(
            !hover(&mut visual),
            "sidebar hover during {mode:?}, reading={read_only}, column={column}"
        );
        visual.update(|w, cx| w.press("ctrl-c", cx));
        cx.update(|cx| assert!(!cx.read_from_clipboard().unwrap().text().unwrap().is_empty()));

        if !reading {
            visual.simulate_mouse_move(point(-px(30.), -px(30.)), MouseButton::Left, modifiers);
            assert_eq!(editor.read_with(cx, |s, _| s.cursor()), 0);
        }
        if !reading && !wrap {
            // Ordinary and rectangular selection both scroll unwrapped lines.
            let right = point(body.right() + px(50.), start.y);
            visual.simulate_mouse_move(right, MouseButton::Left, modifiers);
            let before = editor.read_with(cx, |s, _| s.cursor());
            for _ in 0..12 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(16));
                cx.run_until_parked();
                draw(&mut visual);
            }
            assert!(editor.read_with(cx, |s, _| s.cursor()) > before);
            visual.simulate_mouse_move(start, MouseButton::Left, modifiers);
        }

        // A stationary pointer beyond the viewport must continue scrolling.
        let below = point(start.x + px(70.), body.bottom() + px(50.));
        visual.simulate_mouse_move(below, MouseButton::Left, modifiers);
        draw(&mut visual);
        visual.update(|w, cx| w.press("ctrl-c", cx));
        let before = cx.update(|cx| cx.read_from_clipboard().unwrap().text().unwrap());
        for _ in 0..12 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(16));
            cx.run_until_parked();
            draw(&mut visual);
        }
        visual.update(|w, cx| w.press("ctrl-c", cx));
        let after = cx.update(|cx| cx.read_from_clipboard().unwrap().text().unwrap());
        assert!(
            after.len() > before.len(),
            "stationary drag must extend selection"
        );
        visual.simulate_mouse_up(sidebar, MouseButton::Left, modifiers);
        draw(&mut visual);
        f.update(cx, |this, _, _| {
            assert_eq!(this.session.active, Some(f.ids[0]))
        });
        visual.simulate_mouse_move(sidebar, None, Default::default());
        draw(&mut visual);
        assert!(hover(&mut visual), "hover must resume after release");
        let range = editor.read_with(cx, |s, _| s.selected_range());
        visual.update(|w, cx| {
            // Include positions outside the native client area without desktop input.
            w.dispatch_event(
                MouseMoveEvent {
                    position: point(-px(30.), -px(30.)),
                    pressed_button: Some(MouseButton::Left),
                    modifiers,
                }
                .to_platform_input(),
                cx,
            );
        });
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(64));
        cx.run_until_parked();
        assert_eq!(editor.read_with(cx, |s, _| s.selected_range()), range);
        assert_eq!(editor.read_with(cx, |s, _| s.value().to_owned()), source);
        // Recover if a release outside the native window was missed.
        visual.simulate_mouse_down(start, MouseButton::Left, modifiers);
        draw(&mut visual);
        visual.simulate_mouse_move(sidebar, None, Default::default());
        draw(&mut visual);
        assert!(hover(&mut visual), "missing release must clear capture");
        visual.simulate_mouse_down(start, MouseButton::Left, modifiers);
        draw(&mut visual);
        visual.update(|w, cx| w.press("escape", cx));
        draw(&mut visual);
        visual.simulate_mouse_move(sidebar, None, Default::default());
        draw(&mut visual);
        assert!(hover(&mut visual), "Escape must clear capture");
    }
}

#[gpui::test]
fn markdown_reading_renders_links_and_blocks_edits_without_losing_source_history(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let source = "# 제목\r\n\r\n[문서 링크](https://example.org/docs)\r\n\r\n**굵게** *기울임*\r\n\r\n- 목록\r\n\r\n> 인용\r\n\r\n~~~\r\ncode\r\n~~~\r\n\r\n| A | B |\r\n|---|---|\r\n| a | b |\r\n";
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value(source, window, cx);
            s.set_selected_range(source.len()..source.len(), cx);
            s.replace("추가", window, cx);
            s.set_selected_range(2..8, cx);
        });
        this.focus_editor(window, cx);
        editor
    });
    let original = editor.read_with(cx, |s, _| s.value());
    let revision = editor.read_with(cx, |s, _| s.revision);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let trigger = visual.debug_bounds("reading-mode-control").unwrap();
    visual.simulate_mouse_down(
        trigger.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_up(
        trigger.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.update(|window, cx| window.render_frame(cx));
    assert!(visual.debug_bounds("markdown-reading").is_some());
    let block = visual.debug_bounds("reading-block-1").unwrap();
    let link = point(block.left() + px(32.), block.center().y);
    visual.simulate_mouse_down(link, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(link, gpui::MouseButton::Left, Default::default());
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.org/docs"));
    for key in [
        "backspace",
        "delete",
        "enter",
        "ctrl-enter",
        "tab",
        "ctrl-v",
        "ctrl-x",
        "ctrl-z",
        "ctrl-y",
        "f5",
        "f6",
    ] {
        f.press(cx, key);
    }
    cx.update_window(f.window, |_, window, cx| {
        let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
        handler.replace_text_in_range(None, "키보드 입력", window, cx);
        handler.replace_and_mark_text_in_range(None, "한글 조합", Some(1..1), window, cx);
    })
    .unwrap();
    f.update(cx, |this, window, cx| {
        assert!(editor.read(cx).is_read_only());
        assert_eq!(editor.read(cx).value(), original);
        assert_eq!(editor.read(cx).revision, revision);
        assert_eq!(editor.read(cx).selected_range(), 2..8);
        assert!(!editor.read(cx).is_composing());
        this.replace_local(true, window, cx);
        editor.update(cx, |s, cx| s.set_read_only(false, cx));
        this.focus_editor(window, cx);
    });
    f.press(cx, "ctrl-z");
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), source));
    f.press(cx, "ctrl-y");
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), original));
    f.press(cx, "ctrl-s");
    let external = "# 외부 변경\n\n**새 본문**";
    fs::write(f.dir.join(format!("notes/{}.txt", f.ids[0])), external).unwrap();
    f.update(cx, |this, window, cx| {
        editor.update(cx, |s, cx| s.set_read_only(true, cx));
        this.request_file_sync(
            crate::watch::Changes {
                notes: std::collections::HashSet::from([f.ids[0]]),
                ..Default::default()
            },
            window,
            cx,
        );
    });
    cx.run_until_parked();
    f.update(cx, |this, _, cx| {
        assert_eq!(editor.read(cx).value(), external);
        assert!(editor.read(cx).is_read_only());
        assert!(!this.open[0].dirty);
    });
    f.press(cx, "ctrl-a");
    f.press(cx, "ctrl-c");
    cx.update(|cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            "외부 변경\n\n새 본문"
        )
    });
}

#[gpui::test]
fn status_text_mode_is_per_note_and_reading_mode_is_global(cx: &mut TestAppContext) {
    use crate::widgets::TextMode;
    use gpui::Focusable;
    let f = Fixture::new(cx);
    let (editor, original, revision) = f.update(cx, |this, window, cx| {
        this.focus_editor(window, cx);
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.set_selected_range(0..4, cx));
        let state = editor.read(cx);
        assert_eq!(state.text_mode(), TextMode::Markdown);
        (editor.clone(), state.value(), state.revision)
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let trigger = visual.debug_bounds("text-mode-control").unwrap();
    visual.simulate_mouse_down(
        trigger.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_up(
        trigger.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    f.update(cx, |this, _, _| {
        assert!(this.keyboard_menu.is_some());
        assert!(this.context_note.is_none());
        assert_eq!(this.menu_anchor, gpui::Anchor::BottomRight);
    });
    f.press(cx, "up"); // The selected row initially follows the current Markdown mode.
    f.press(cx, "enter");
    f.update(cx, |this, window, cx| {
        let state = editor.read(cx);
        assert_eq!(state.text_mode(), TextMode::PlainText);
        assert_eq!(state.value(), original);
        assert_eq!(state.revision, revision);
        assert_eq!(state.selected_range(), 0..4);
        assert!(!state.is_read_only());
        assert!(!this.open.iter().find(|t| t.id == f.ids[0]).unwrap().dirty);
        assert!(state.focus_handle(cx).is_focused(window));
        assert!(this.keyboard_menu.is_none());
        assert!(this.open_note(f.ids[1], true, window, cx));
        assert_eq!(
            this.active_editor().unwrap().read(cx).text_mode(),
            TextMode::Markdown
        );
        assert!(this.open_note(f.ids[0], true, window, cx));
        assert_eq!(
            this.active_editor().unwrap().read(cx).text_mode(),
            TextMode::PlainText
        );
    });
    visual.update(|window, cx| window.render_frame(cx));
    let trigger = visual.debug_bounds("reading-mode-control").unwrap();
    visual.simulate_mouse_down(
        trigger.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_up(
        trigger.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.update(|window, cx| window.render_frame(cx));
    assert!(visual.debug_bounds("text-input").is_some());
    assert!(visual.debug_bounds("markdown-reading").is_none());
    for key in [
        "backspace",
        "delete",
        "enter",
        "ctrl-enter",
        "ctrl-v",
        "ctrl-x",
        "ctrl-z",
        "ctrl-y",
        "f5",
        "f6",
    ] {
        f.press(cx, key);
    }
    cx.update_window(f.window, |_, window, cx| {
        let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
        handler.replace_text_in_range(None, "입력", window, cx);
        handler.replace_and_mark_text_in_range(None, "한글", Some(1..1), window, cx);
    })
    .unwrap();
    f.update(cx, |this, window, cx| {
        assert!(editor.read(cx).is_read_only());
        assert_eq!(editor.read(cx).text_mode(), TextMode::PlainText);
        assert_eq!(editor.read(cx).value(), original);
        assert_eq!(editor.read(cx).revision, revision);
        assert_eq!(editor.read(cx).selected_range(), 0..4);
        assert!(!editor.read(cx).is_composing());
        assert!(!this.open.iter().find(|t| t.id == f.ids[0]).unwrap().dirty);
        assert!(this.open_note(f.ids[1], true, window, cx));
        assert!(this.active_editor().unwrap().read(cx).is_read_only());
        assert!(cx.global::<AppState>().settings.read_only);
        assert!(
            Settings::load(&cx.global::<AppState>().config_dir)
                .unwrap()
                .read_only
        );
        assert!(this.open_note(f.ids[2], true, window, cx));
        assert!(this.active_editor().unwrap().read(cx).is_read_only());
        assert!(this.open_note(f.ids[0], true, window, cx));
        assert!(editor.read(cx).is_read_only());
    });
    f.press(cx, "ctrl-home");
    f.press(cx, "shift-right");
    f.press(cx, "ctrl-c");
    cx.update(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "🙂"));
    f.press(cx, "ctrl-a");
    f.press(cx, "ctrl-c");
    cx.update(|cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            original.as_ref()
        )
    });
    visual.update(|window, cx| window.render_frame(cx));
    let trigger = visual.debug_bounds("text-mode-control").unwrap();
    visual.simulate_mouse_down(
        trigger.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_up(
        trigger.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    f.press(cx, "down");
    f.press(cx, "enter");
    f.update(cx, |this, _, cx| {
        assert_eq!(editor.read(cx).text_mode(), TextMode::Markdown);
        assert!(editor.read(cx).is_read_only());
        assert_eq!(editor.read(cx).value(), original);
        assert_eq!(editor.read(cx).revision, revision);
        assert!(this.keyboard_menu.is_none());
    });
    visual.update(|window, cx| window.render_frame(cx));
    assert!(visual.debug_bounds("markdown-reading").is_some());
    let trigger = visual.debug_bounds("reading-mode-control").unwrap();
    visual.simulate_mouse_down(
        trigger.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_up(
        trigger.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    f.update(cx, |this, window, cx| {
        assert!(!editor.read(cx).is_read_only());
        assert_eq!(editor.read(cx).text_mode(), TextMode::Markdown);
        assert!(editor.read(cx).focus_handle(cx).is_focused(window));
        assert!(!cx.global::<AppState>().settings.read_only);
        assert!(
            !Settings::load(&cx.global::<AppState>().config_dir)
                .unwrap()
                .read_only
        );
        for tab in &this.open {
            assert!(!tab.editor.as_ref().unwrap().read(cx).is_read_only());
        }
        // Failed persistence must leave every editor and the saved setting intact.
        fs::remove_file(cx.global::<AppState>().config_dir.join("settings.ini")).unwrap();
        fs::create_dir(cx.global::<AppState>().config_dir.join("settings.ini")).unwrap();
        assert!(!this.set_read_only(true, window, cx));
        assert!(!cx.global::<AppState>().settings.read_only);
        for tab in &this.open {
            assert!(!tab.editor.as_ref().unwrap().read(cx).is_read_only());
        }
    });
}

#[gpui::test]
fn newline_keeps_indentation_and_ctrl_enter_inserts_below(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        this.focus_editor(window, cx);
        this.active_editor().unwrap()
    });
    cx.update_window(f.window, |_, window, cx| {
        window.render_frame(cx);
        for (source, selection, key, expected, cursor) in [
            ("    alpha", 9..9, "enter", "    alpha\n    ", 14),
            ("  hello", 4..4, "enter", "  he\n  llo", 7),
            ("    text", 2..2, "enter", "  \n    text", 5),
            (" \t한글", 8..8, "enter", " \t한글\n \t", 11),
            (" \talpha\n beta", 2..7, "enter", " \t\n \t\n beta", 5),
            ("  hello\nnext", 4..4, "ctrl-enter", "  hello\n  \nnext", 10),
            ("  hello\nnext", 2..5, "ctrl-enter", "  hello\n  \nnext", 10),
            ("  hello", 4..4, "ctrl-enter", "  hello\n  ", 10),
            ("  hi\r\nnext", 3..3, "ctrl-enter", "  hi\n  \r\nnext", 7),
            ("  \t", 1..1, "ctrl-enter", "  \t\n  \t", 7),
            ("", 0..0, "ctrl-enter", "\n", 1),
        ] {
            editor.update(cx, |s, cx| {
                s.set_value(source, window, cx);
                s.set_selected_range(selection.clone(), cx);
            });
            window.press(key, cx);
            assert_eq!(editor.read(cx).value(), expected, "{key}: {source:?}");
            assert_eq!(editor.read(cx).cursor(), cursor);
            window.press("ctrl-z", cx);
            assert_eq!(editor.read(cx).value(), source);
            assert_eq!(editor.read(cx).selected_range(), selection);
            window.press("ctrl-y", cx);
            assert_eq!(editor.read(cx).value(), expected);
            assert_eq!(editor.read(cx).cursor(), cursor);
        }
        for (source, key, expected, cursors) in [
            ("  one\n\tone", "enter", "  \n  \n\t\n\t", vec![5..5, 9..9]),
            (
                "  one\n\tone",
                "ctrl-enter",
                "  one\n  \n\tone\n\t",
                vec![8..8, 15..15],
            ),
            (
                "  one one",
                "ctrl-enter",
                "  one one\n  ",
                std::iter::once(12..12).collect(),
            ),
        ] {
            editor.update(cx, |s, cx| {
                s.set_value(source, window, cx);
                s.set_selected_range(2..5, cx);
            });
            window.press("ctrl-d", cx);
            let before: Vec<_> = editor.read(cx).selected_ranges().collect();
            assert_eq!(before.len(), 2);
            window.press(key, cx);
            assert_eq!(editor.read(cx).value(), expected);
            assert_eq!(
                editor.read(cx).selected_ranges().collect::<Vec<_>>(),
                cursors
            );
            window.press("ctrl-z", cx);
            assert_eq!(editor.read(cx).value(), source);
            assert_eq!(
                editor.read(cx).selected_ranges().collect::<Vec<_>>(),
                before
            );
        }
    })
    .unwrap();
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
fn empty_ime_preedit_allows_held_backspace_and_app_shortcuts(cx: &mut TestAppContext) {
    use gpui::InputEvent as _;

    let f = Fixture::new(cx);
    let original = "a".repeat(80);
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value(original.as_str(), window, cx);
            s.set_selected_range(original.len()..original.len(), cx);
        });
        this.focus_editor(window, cx);
        editor
    });
    cx.update_window(f.window, |_, window, cx| {
        window.render_frame(cx);
        let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
        handler.replace_and_mark_text_in_range(None, "ㄱ", Some(1..1), window, cx);
        handler.replace_and_mark_text_in_range(None, "가", Some(1..1), window, cx);
        assert_eq!(editor.read(cx).value(), format!("{original}가"));
        // Empty preedit must release editing keys without a separate unmark callback.
        handler.replace_and_mark_text_in_range(None, "", None, window, cx);
        window.render_frame(cx);
        for repeat in 0..25 {
            window.dispatch_event(
                gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("backspace").unwrap(),
                    is_held: repeat > 0,
                    prefer_character_input: false,
                }
                .to_platform_input(),
                cx,
            );
        }
        assert_eq!(editor.read(cx).value(), "a".repeat(55));
        assert!(!editor.read(cx).is_composing());
        assert!(handler.marked_text_range(window, cx).is_none());
    })
    .unwrap();
    f.press(cx, "secondary-f");
    f.update(cx, |this, _, _| assert!(this.find_visible));
}

#[gpui::test]
fn empty_ime_preedit_preserves_multicaret_undo_and_redo(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let original = "A🙂Z A🙂Z";
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value(original, window, cx);
            s.set_selected_range(1..5, cx);
        });
        this.focus_editor(window, cx);
        editor
    });
    f.press(cx, "secondary-d");
    let selections = f.update(cx, |_, _, cx| {
        let selections = editor.read(cx).selected_ranges().collect::<Vec<_>>();
        assert_eq!(selections.len(), 2);
        selections
    });
    cx.update_window(f.window, |_, window, cx| {
        let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
        handler.replace_and_mark_text_in_range(None, "한", Some(1..1), window, cx);
        assert_eq!(editor.read(cx).value(), "A한Z A한Z");
        handler.replace_and_mark_text_in_range(None, "", Some(0..0), window, cx);
        assert_eq!(editor.read(cx).value(), "AZ AZ");
        assert!(!editor.read(cx).is_composing());
        assert!(handler.marked_text_range(window, cx).is_none());
        // A later macOS unmarkText callback must not add another undo transaction.
        handler.unmark_text(window, cx);
    })
    .unwrap();
    f.press(cx, "secondary-z");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), original);
        assert_eq!(
            editor.read(cx).selected_ranges().collect::<Vec<_>>(),
            selections
        );
    });
    f.press(cx, "secondary-shift-z");
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), "AZ AZ"));
    f.press(cx, "backspace");
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), "Z Z"));
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
fn web_link_ctrl_click_opens_exact_urls_after_tabs_wrapping_and_scrolling(cx: &mut TestAppContext) {
    use gpui::InputEvent as _;
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        this.focus_editor(window, cx);
        this.active_editor().unwrap()
    });
    let control = gpui::Modifiers {
        control: true,
        ..Default::default()
    };
    let mut click = |text: &str, target: &str, wrap: bool, modifiers: gpui::Modifiers| {
        cx.update_window(f.window, |_, window, cx| {
            let byte = text.find(target).unwrap();
            let character = text[..byte].encode_utf16().count();
            editor.update(cx, |state, cx| {
                state.set_value(text, window, cx);
                state.set_soft_wrap(wrap, window, cx);
                state.set_selected_range(byte..byte, cx);
            });
            window.render_frame(cx);
            let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
            let bounds = handler
                .bounds_for_range(character..character + 1, window, cx)
                .unwrap();
            let position = point(bounds.left() + bounds.size.width * 0.25, bounds.center().y);
            let selection = editor.read(cx).selected_range();
            let revision = editor.read(cx).revision;
            window.dispatch_event(
                gpui::MouseDownEvent {
                    button: gpui::MouseButton::Left,
                    position,
                    modifiers,
                    click_count: 1,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                gpui::MouseUpEvent {
                    button: gpui::MouseButton::Left,
                    position,
                    modifiers,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            assert_eq!(editor.read(cx).value(), text);
            assert_eq!(editor.read(cx).revision, revision);
            if modifiers.control {
                assert_eq!(editor.read(cx).selected_range(), selection);
            }
        })
        .unwrap();
        cx.opened_url()
    };
    let text = "한글\t(https://example.com/a_(b)).\r\n";
    assert!(click(text, "a_(b)", true, Default::default()).is_none());
    assert!(click(text, ").", true, control).is_none());
    assert_eq!(
        click(text, "a_(b)", true, control).as_deref(),
        Some("https://example.com/a_(b)")
    );
    let long = format!("앞\twww.example.org/{}끝", "long-path/".repeat(120));
    let url = &long[long.find("www.").unwrap()..];
    assert_eq!(
        click(&long, "끝", true, control),
        Some(format!("https://{url}"))
    );
    let unwrapped = format!(
        "{}\r\n앞\tHTTP://localhost:3000/경로",
        "빈 줄\r\n".repeat(80)
    );
    assert_eq!(
        click(&unwrapped, "경로", false, control).as_deref(),
        Some("HTTP://localhost:3000/경로")
    );
}

#[gpui::test]
fn double_click_uses_plain_text_word_boundaries(cx: &mut TestAppContext) {
    use gpui::InputEvent as _;
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        this.focus_editor(window, cx);
        editor
    });
    cx.update_window(f.window, |_, window, cx| {
        let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
        for (text, character, expected) in [
            ("foo.bar", 1, "foo"),
            ("foo.bar", 5, "bar"),
            ("alpha-beta", 2, "alpha"),
            ("name_value", 4, "name_value"),
            ("한글.노트", 1, "한글"),
            ("한글.노트", 4, "노트"),
            ("-3.14", 2, "-3.14"),
        ] {
            editor.update(cx, |s, cx| s.set_value(text, window, cx));
            window.render_frame(cx);
            let bounds = handler
                .bounds_for_range(character..character + 1, window, cx)
                .unwrap();
            let position = point(bounds.left() + bounds.size.width * 0.25, bounds.center().y);
            window.dispatch_event(
                gpui::MouseDownEvent {
                    button: gpui::MouseButton::Left,
                    position,
                    click_count: 2,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                gpui::MouseUpEvent {
                    button: gpui::MouseButton::Left,
                    position,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            assert_eq!(editor.read(cx).selected_value(), expected, "{text}");
        }
    })
    .unwrap();
}

#[gpui::test]
fn line_copy_and_cut_paste_above_current_line_and_preserve_cursor(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        this.focus_editor(window, cx);
        editor
    });
    cx.update_window(f.window, |_, window, cx| {
        window.render_frame(cx);
        for newline in ["\n", "\r\n", "\r"] {
            let source = format!("alpha{newline}target{newline}tail");
            let copied = format!("alpha{newline}");
            let cursor = source.find("target").unwrap() + 3;
            editor.update(cx, |s, cx| {
                s.set_value(source.clone(), window, cx);
                s.set_selected_range(2..2, cx);
            });
            window.press("ctrl-c", cx);
            assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), copied);
            editor.update(cx, |s, cx| s.set_selected_range(cursor..cursor, cx));
            window.press("ctrl-v", cx);
            let pasted = format!("alpha{newline}alpha{newline}target{newline}tail");
            assert_eq!(editor.read(cx).value(), pasted);
            assert_eq!(editor.read(cx).cursor(), cursor + copied.len());
            window.press("ctrl-z", cx);
            assert_eq!(editor.read(cx).value(), source);
            assert_eq!(editor.read(cx).cursor(), cursor);
            window.press("ctrl-y", cx);
            assert_eq!(editor.read(cx).value(), pasted);
            assert_eq!(editor.read(cx).cursor(), cursor + copied.len());

            // The final line has no terminator; pasting still inserts a whole line.
            editor.update(cx, |s, cx| {
                s.set_value(source.clone(), window, cx);
                let at = source.len() - 2;
                s.set_selected_range(at..at, cx);
            });
            window.press("ctrl-c", cx);
            editor.update(cx, |s, cx| s.set_selected_range(2..2, cx));
            window.press("ctrl-v", cx);
            assert_eq!(editor.read(cx).value(), format!("tail{newline}{source}"));
            assert_eq!(editor.read(cx).cursor(), 2 + 4 + newline.len());

            editor.update(cx, |s, cx| {
                s.set_value(source.clone(), window, cx);
                s.set_selected_range(2..2, cx);
            });
            window.press("ctrl-x", cx);
            assert_eq!(editor.read(cx).value(), format!("target{newline}tail"));
            editor.update(cx, |s, cx| s.set_selected_range(3..3, cx));
            window.press("ctrl-v", cx);
            assert_eq!(editor.read(cx).value(), source);
            assert_eq!(editor.read(cx).cursor(), 3 + copied.len());

            // A selection uses normal replacement, even with line-copy metadata.
            editor.update(cx, |s, cx| s.set_selected_range(1..3, cx));
            window.press("ctrl-v", cx);
            assert_eq!(
                editor.read(cx).value(),
                format!("a{copied}ha{newline}target{newline}tail")
            );

            // Identical text from another app must not inherit line-copy behavior.
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()));
            editor.update(cx, |s, cx| {
                s.set_value(source.clone(), window, cx);
                s.set_selected_range(2..2, cx);
            });
            window.press("ctrl-v", cx);
            assert_eq!(
                editor.read(cx).value(),
                format!("al{copied}pha{newline}target{newline}tail")
            );
        }

        // A single copied line also pastes correctly at multiple empty carets,
        // with only one insertion when two carets share the same logical line.
        editor.update(cx, |s, cx| s.set_value("line\nkeep", window, cx));
        window.press("ctrl-c", cx);
        for (source, expected, cursors) in [
            ("aa\naa", "line\naa\nline\naa", vec![7..7, 15..15]),
            ("aa aa", "line\naa aa", vec![7..7, 10..10]),
        ] {
            editor.update(cx, |s, cx| {
                s.set_value(source, window, cx);
                s.set_selected_range(0..2, cx);
            });
            window.press("ctrl-d", cx);
            window.press("right", cx);
            let before: Vec<_> = editor.read(cx).selected_ranges().collect();
            assert_eq!(before.len(), 2);
            window.press("ctrl-v", cx);
            assert_eq!(editor.read(cx).value(), expected);
            assert_eq!(
                editor.read(cx).selected_ranges().collect::<Vec<_>>(),
                cursors
            );
            window.press("ctrl-z", cx);
            assert_eq!(editor.read(cx).value(), source);
            assert_eq!(
                editor.read(cx).selected_ranges().collect::<Vec<_>>(),
                before
            );
        }
    })
    .unwrap();
}
