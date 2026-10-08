use super::*;

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
            ("한글.메모", 1, "한글"),
            ("한글.메모", 4, "메모"),
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
