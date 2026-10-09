use super::*;

#[gpui::test]
fn markdown_preview_renders_links_and_blocks_edits_without_losing_source_history(
    cx: &mut TestAppContext,
) {
    use crate::widgets::TextMode;
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
    visual.update(|window, cx| window.render_frame(cx));
    assert!(visual.debug_bounds("markdown-preview").is_some());
    let block = visual.debug_bounds("preview-block-1").unwrap();
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
        editor.update(cx, |s, cx| s.set_text_mode(TextMode::Markdown, cx));
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
        editor.update(cx, |s, cx| s.set_text_mode(TextMode::MarkdownPreview, cx));
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
fn status_text_mode_menu_changes_only_the_current_editor(cx: &mut TestAppContext) {
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
        assert_eq!(editor.read(cx).value(), original);
        assert_eq!(editor.read(cx).revision, revision);
        assert!(this.keyboard_menu.is_none());
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
