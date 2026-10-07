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

#[gpui::test]
fn clipboard_without_selection_uses_logical_lines_and_preserves_undo(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let cases = [
        ("한글🙂\r\n둘째\n끝", 3, "한글🙂\r\n", "둘째\n끝"),
        ("첫\r둘째\n끝", 7, "둘째\n", "첫\r끝"),
        ("첫\n\r\n끝", 4, "\r\n", "첫\n끝"),
        ("첫\n마지막🙂", 7, "마지막🙂", "첫\n"),
        ("한글🙂", 3, "한글🙂", ""),
    ];
    for (text, cursor, copied, remaining) in cases {
        let editor = f.update(cx, |this, w, cx| {
            let editor = this.active_editor().unwrap();
            editor.update(cx, |s, cx| {
                s.set_value(text, w, cx);
                s.set_selected_range(cursor..cursor, cx);
                s.focus(w, cx);
            });
            editor
        });
        f.press(cx, "ctrl-c");
        f.update(cx, |_, _, cx| {
            assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), copied);
            assert_eq!(editor.read(cx).value(), text);
            assert_eq!(editor.read(cx).selected_range(), cursor..cursor);
        });
        f.press(cx, "ctrl-x");
        f.update(cx, |_, _, cx| {
            assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), copied);
            assert_eq!(editor.read(cx).value(), remaining);
        });
        f.press(cx, "ctrl-z");
        f.update(cx, |_, _, cx| {
            assert_eq!(editor.read(cx).value(), text);
            assert_eq!(editor.read(cx).selected_range(), cursor..cursor);
        });
        f.press(cx, "ctrl-y");
        f.update(cx, |_, _, cx| {
            assert_eq!(editor.read(cx).value(), remaining)
        });
    }
    // Empty document and single-line fields leave the clipboard alone.
    f.update(cx, |this, w, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("keep".into()));
        this.active_editor()
            .unwrap()
            .update(cx, |s, cx| s.set_value("", w, cx));
    });
    f.press(cx, "ctrl-x");
    f.press(cx, "ctrl-c");
    f.update(cx, |this, w, cx| {
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "keep");
        this.show_find(false, false, w, cx);
        this.local
            .query
            .update(cx, |s, cx| s.set_value("query", w, cx));
    });
    f.press(cx, "ctrl-c");
    f.press(cx, "ctrl-x");
    f.update(cx, |this, _, cx| {
        assert_eq!(this.local.query.read(cx).value(), "query");
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "keep");
    });
}
