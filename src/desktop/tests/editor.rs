use super::*;

#[gpui::test]
fn control_d_selects_plain_text_words_at_unicode_and_line_boundaries(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.focus(w, cx));
        editor
    });
    for (text, expected) in [
        ("word", "word"),
        ("hello_world", "hello_world"),
        ("한국어", "한국어"),
        ("e\u{301}cole", "e\u{301}cole"),
        ("👩‍💻", "👩‍💻"),
        ("-12.5px", "-12.5px"),
        ("IMPLEMENTATION.md", "IMPLEMENTATION"),
    ] {
        for at in text.char_indices().map(|(i, _)| i).chain([text.len()]) {
            if at > expected.len() {
                continue;
            }
            f.update(cx, |_, w, cx| {
                editor.update(cx, |s, cx| {
                    s.set_value(text, w, cx);
                    s.set_selected_range(at..at, cx);
                });
            });
            f.press(cx, "ctrl-d");
            f.update(cx, |_, _, cx| {
                assert_eq!(
                    editor.read(cx).selected_value(),
                    expected,
                    "{text:?} at {at}"
                );
                assert_eq!(editor.read(cx).value().as_ref(), text);
            });
        }
    }
    for (text, at, expected) in [
        ("", 0, 0..0),
        ("   ", 1, 1..1),
        (";;;", 1, 1..1),
        ("one\r\ntwo", 3, 0..3),
        ("one\r\ntwo", 5, 5..8),
        ("one\n", 4, 4..4),
        ("a\u{3000}한글", 4, 4..10),
        ("file.md", 6, 5..7),
    ] {
        f.update(cx, |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_value(text, w, cx);
                s.set_selected_range(at..at, cx);
            });
        });
        f.press(cx, "ctrl-d");
        f.update(cx, |_, _, cx| {
            assert_eq!(editor.read(cx).selected_range(), expected)
        });
    }
}

#[gpui::test]
fn wrapped_word_selection_paints_only_its_visual_row(cx: &mut TestAppContext) {
    use crate::widgets::ActiveTheme;
    use gpui::EntityInputHandler;
    let f = Fixture::new(cx);
    let prefix = "prefix ".repeat(4);
    let word = "IMPLEMENTATION.md";
    let start = prefix.len();
    let end = start + word.len();
    let editor = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value(format!("{prefix}{word} tail"), w, cx);
            s.focus(w, cx);
        });
        editor
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.simulate_resize(size(px(620.), px(760.)));
    for scale in [1., 1.25, 1.5, 2.] {
        visual.simulate_scale_factor_change(scale);
        visual.update(|w, cx| w.render_frame(cx));
        let body = visual.debug_bounds("body-editor").unwrap();
        let (first, inside, last) = f.update(cx, |_, w, cx| {
            editor.update(cx, |s, cx| {
                (
                    s.bounds_for_range(0..0, Bounds::default(), w, cx).unwrap(),
                    s.bounds_for_range(start + 1..start + 1, Bounds::default(), w, cx)
                        .unwrap(),
                    s.bounds_for_range(end..end, Bounds::default(), w, cx)
                        .unwrap(),
                )
            })
        });
        assert!(inside.top() > first.top(), "the word must wrap");
        let word_start = point(first.left() + px(0.1), inside.center().y);
        f.update(cx, |_, w, cx| {
            editor.update(cx, |s, cx| {
                assert_eq!(s.character_index_for_point(word_start, w, cx), Some(start));
            })
        });
        for range in [start..end, 0..start, start..start] {
            f.update(cx, |_, _, cx| {
                editor.update(cx, |s, cx| s.set_selected_range(range.clone(), cx))
            });
            visual.update(|w, cx| w.render_frame(cx));
            f.update(cx, |_, w, cx| {
                let quads: Vec<_> = w
                    .painted_quads()
                    .into_iter()
                    .filter(|q| {
                        q.background == cx.theme().selection.into()
                            && body.scale(scale).contains(&q.bounds.origin)
                    })
                    .collect();
                if range.is_empty() {
                    assert!(quads.is_empty(), "a caret has no selection background");
                } else {
                    assert_eq!(
                        quads.len(),
                        1,
                        "selection {range:?} must not spill across a wrap"
                    );
                    let expected = if range.start == start { inside } else { first };
                    // Painted quads snap to physical pixels at fractional DPI.
                    assert!((quads[0].bounds.top().0 - expected.scale(scale).top().0).abs() <= 1.);
                    assert!((quads[0].bounds.left().0 - first.scale(scale).left().0).abs() <= 1.);
                    if range.start == start {
                        assert!(
                            (quads[0].bounds.right().0 - last.scale(scale).left().0).abs() <= 1.
                        );
                    }
                    assert_eq!(
                        editor.read(cx).selected_value(),
                        if range.start == start { word } else { &prefix }
                    );
                }
            });
        }
        // Selecting backwards must place the active caret on the word's row too.
        visual.simulate_click(point(last.left(), last.center().y), Default::default());
        visual.simulate_click(
            word_start,
            gpui::Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        visual.update(|w, cx| w.render_frame(cx));
        f.update(cx, |_, w, cx| {
            editor.update(cx, |s, cx| {
                assert_eq!(s.selected_range(), start..end);
                assert_eq!(s.cursor(), start);
                let caret = s
                    .bounds_for_range(start..start, Bounds::default(), w, cx)
                    .unwrap();
                assert_eq!(caret.top(), inside.top());
                assert_eq!(caret.left(), first.left());
            })
        });
        // IME composition uses the same start/end distinction as selection.
        f.update(cx, |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.replace_and_mark_text_in_range(Some(start..end), word, None, w, cx);
            })
        });
        visual.update(|w, cx| w.render_frame(cx));
        f.update(cx, |_, w, cx| {
            let underlines: Vec<_> = w
                .painted_quads()
                .into_iter()
                .filter(|q| {
                    q.background == cx.theme().foreground.into()
                        && q.bounds.size.height.0 <= scale + 1.
                        && q.bounds.size.width.0 > 4. * scale
                        && body.scale(scale).contains(&q.bounds.origin)
                })
                .collect();
            assert_eq!(underlines.len(), 1);
            assert!(underlines[0].bounds.top() >= inside.scale(scale).top());
            assert!(underlines[0].bounds.bottom() <= inside.scale(scale).bottom());
            editor.update(cx, |s, cx| s.unmark_text(w, cx));
        });
    }
}

#[gpui::test]
fn mouse_selection_below_document_anchors_at_end(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    let f = Fixture::new(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    for wrap in [false, true] {
        for text in [
            "first\r\n마지막🙂".to_owned(),
            "first\r\n마지막🙂\r\n".to_owned(),
            "wrapped words ".repeat(25),
            String::new(),
        ] {
            let editor = f.update(cx, |this, w, cx| {
                let editor = this.active_editor().unwrap();
                editor.update(cx, |s, cx| {
                    s.set_soft_wrap(wrap, w, cx);
                    s.set_value(text.clone(), w, cx);
                });
                editor
            });
            visual.update(|w, cx| w.render_frame(cx));
            let body = visual.debug_bounds("body-editor").unwrap();
            let first = f.update(cx, |_, w, cx| {
                editor.update(cx, |s, cx| {
                    s.bounds_for_range(0..0, Bounds::default(), w, cx).unwrap()
                })
            });
            // Left-side whitespace well below the last visual row.
            let below = point(first.left() + px(1.), body.bottom() - px(25.));
            visual.simulate_mouse_down(below, gpui::MouseButton::Left, Default::default());
            f.update(cx, |_, _, cx| {
                assert_eq!(editor.read(cx).selected_range(), text.len()..text.len());
            });
            // A long unwrapped line may have scrolled horizontally to reveal
            // the new caret. The upper margin still reaches document start.
            let start = point(first.left(), body.top() + px(1.));
            visual.simulate_mouse_move(start, Some(gpui::MouseButton::Left), Default::default());
            f.update(cx, |_, _, cx| {
                assert_eq!(editor.read(cx).selected_range(), 0..text.len());
            });
            visual.simulate_mouse_up(start, gpui::MouseButton::Left, Default::default());
            // The upper margin also maps to the document boundary, regardless of x.
            visual.simulate_click(
                point(body.right() - px(30.), body.top() + px(1.)),
                Default::default(),
            );
            f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).cursor(), 0));
            visual.simulate_click(
                below,
                gpui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
            );
            f.update(cx, |_, _, cx| {
                assert_eq!(editor.read(cx).selected_range(), 0..text.len())
            });
        }
    }
}

#[gpui::test]
fn mouse_hit_testing_keeps_graphemes_and_tabs_atomic(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    use unicode_segmentation::UnicodeSegmentation;
    let text = "Wa\u{301}\t한👨‍👩‍👧‍👦W";
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.set_value(text, w, cx));
        editor
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    for (byte, grapheme) in text.grapheme_indices(true) {
        let end = byte + grapheme.len();
        let from = text[..byte].encode_utf16().count();
        let to = text[..end].encode_utf16().count();
        let (left, right) = f.update(cx, |_, w, cx| {
            editor.update(cx, |s, cx| {
                (
                    s.bounds_for_range(from..from, Bounds::default(), w, cx)
                        .unwrap(),
                    s.bounds_for_range(to..to, Bounds::default(), w, cx)
                        .unwrap(),
                )
            })
        });
        for (fraction, expected) in [(0.25, byte), (0.75, end)] {
            visual.simulate_click(
                point(
                    left.left() + (right.left() - left.left()) * fraction,
                    left.center().y,
                ),
                Default::default(),
            );
            f.update(cx, |_, _, cx| {
                assert_eq!(
                    editor.read(cx).cursor(),
                    expected,
                    "{grapheme:?} at {fraction}"
                )
            });
        }
    }
}

#[gpui::test]
fn mouse_uses_nearest_caret_and_resets_keyboard_column(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value("WWWWWW\nWWWWWW\nWWWWWW", w, cx);
            s.set_selected_range(5..5, cx);
            s.focus(w, cx);
        });
        editor
    });
    f.press(cx, "down");
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let (left, right) = f.update(cx, |_, w, cx| {
        editor.update(cx, |s, cx| {
            (
                s.bounds_for_range(7..7, Bounds::default(), w, cx).unwrap(),
                s.bounds_for_range(8..8, Bounds::default(), w, cx).unwrap(),
            )
        })
    });
    for (fraction, expected) in [(0.25, 7), (0.75, 8)] {
        visual.simulate_click(
            point(
                left.left() + (right.left() - left.left()) * fraction,
                left.center().y,
            ),
            Default::default(),
        );
        f.update(cx, |_, _, cx| {
            assert_eq!(editor.read(cx).cursor(), expected)
        });
    }
    f.press(cx, "down");
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).cursor(), 15));
}

#[gpui::test]
fn single_line_vertical_padding_keeps_horizontal_hit_testing(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    let f = Fixture::new(cx);
    let query = f.update(cx, |this, w, cx| {
        this.show_find(false, false, w, cx);
        let query = this.local.query.clone();
        query.update(cx, |s, cx| s.set_value("WWWW", w, cx));
        query
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let field = visual.debug_bounds("현재 메모 검색어").unwrap();
    let caret = f.update(cx, |_, w, cx| {
        query.update(cx, |s, cx| {
            s.bounds_for_range(2..2, Bounds::default(), w, cx).unwrap()
        })
    });
    for y in [field.top() + px(2.), field.bottom() - px(2.)] {
        visual.simulate_click(point(caret.left() + px(0.1), y), Default::default());
        f.update(cx, |_, _, cx| assert_eq!(query.read(cx).cursor(), 2));
    }
    f.update(cx, |_, w, cx| {
        query.update(cx, |s, cx| s.set_value("", w, cx))
    });
    visual.update(|w, cx| w.render_frame(cx));
    for click_count in [2, 3] {
        visual.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: point(field.left() + px(40.), field.center().y),
            click_count,
            ..Default::default()
        });
        visual.simulate_mouse_up(field.center(), gpui::MouseButton::Left, Default::default());
        f.update(cx, |_, _, cx| {
            assert_eq!(query.read(cx).selected_range(), 0..0)
        });
    }
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
