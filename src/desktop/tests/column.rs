// These arrays intentionally contain selection ranges, including single ranges.
#![allow(clippy::single_range_in_vec_init)]
use super::*;
use crate::widgets::input::InputState;
use gpui::{EntityInputHandler, Modifiers, MouseButton, VisualTestContext};

fn prepare(f: &Fixture, cx: &mut TestAppContext, text: &str) -> Entity<InputState> {
    f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value(text.to_owned(), w, cx);
            s.focus(w, cx);
        });
        editor
    })
}

fn caret(
    f: &Fixture,
    cx: &mut TestAppContext,
    editor: &Entity<InputState>,
    offset: usize,
) -> gpui::Point<gpui::Pixels> {
    f.update(cx, |_, w, cx| {
        editor.update(cx, |s, cx| {
            let bounds = s
                .bounds_for_range(offset..offset, Bounds::default(), w, cx)
                .unwrap();
            point(bounds.left() + px(0.01), bounds.center().y)
        })
    })
}

fn drag(
    visual: &mut VisualTestContext,
    start: gpui::Point<gpui::Pixels>,
    end: gpui::Point<gpui::Pixels>,
) {
    let modifiers = Modifiers {
        alt: true,
        shift: true,
        ..Default::default()
    };
    visual.simulate_event(gpui::MouseDownEvent {
        button: MouseButton::Left,
        position: start,
        modifiers,
        click_count: 1,
        ..Default::default()
    });
    visual.update(|w, cx| w.render_frame(cx));
    // Releasing the modifier keys during the drag must not change selection mode.
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
    visual.update(|w, cx| w.render_frame(cx));
}

#[gpui::test]
fn column_drag_both_directions_edits_and_restores_all_rows_at_hidpi(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let mut visual = VisualTestContext::from_window(f.window, cx);
    for scale in [1., 1.5, 2.] {
        visual.simulate_scale_factor_change(scale);
        for reverse in [false, true] {
            let e = prepare(&f, cx, "abcdef\nABCDEF\n012345");
            visual.update(|w, cx| w.render_frame(cx));
            let start = caret(&f, cx, &e, 1);
            let end = caret(&f, cx, &e, 18);
            let (start, end) = if reverse { (end, start) } else { (start, end) };
            drag(&mut visual, start, end);
            let expected = if reverse {
                vec![15..18, 8..11, 1..4]
            } else {
                vec![1..4, 8..11, 15..18]
            };
            f.update(cx, |_, _, cx| {
                assert_eq!(e.read(cx).selected_ranges().collect::<Vec<_>>(), expected)
            });
            f.press(cx, "ctrl-c");
            cx.update(|cx| {
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().as_deref(),
                    Some("bcd\nBCD\n123")
                )
            });
            f.input(cx, "X");
            f.update(cx, |_, _, cx| {
                assert_eq!(e.read(cx).value().as_ref(), "aXef\nAXEF\n0X45")
            });
            f.press(cx, "ctrl-z");
            f.update(cx, |_, _, cx| {
                assert_eq!(e.read(cx).value().as_ref(), "abcdef\nABCDEF\n012345");
                assert_eq!(e.read(cx).selected_ranges().collect::<Vec<_>>(), expected);
            });
            f.press(cx, "escape");
            visual.simulate_mouse_move(start, Some(MouseButton::Left), Default::default());
            f.update(cx, |_, _, cx| {
                assert_eq!(e.read(cx).selected_ranges().count(), 1)
            });
        }
    }
}

#[gpui::test]
fn column_drag_skips_short_and_empty_lines_without_selecting_line_endings(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let e = prepare(&f, cx, "abcdef\r\nx\r\n\r\nabcdef");
    let mut visual = VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let start = caret(&f, cx, &e, 2);
    let end = caret(&f, cx, &e, 17);
    drag(&mut visual, start, end);
    f.update(cx, |_, _, cx| {
        assert_eq!(
            e.read(cx).selected_ranges().collect::<Vec<_>>(),
            [2..4, 15..17]
        )
    });
    f.input(cx, "#");
    f.update(cx, |_, _, cx| {
        assert_eq!(e.read(cx).value().as_ref(), "ab#ef\r\nx\r\n\r\nab#ef")
    });
    f.press(cx, "ctrl-z");
    f.press(cx, "ctrl-c");
    cx.update(|cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("cd\ncd")
        )
    });
    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("1\n2".into())));
    f.press(cx, "ctrl-v");
    f.update(cx, |_, _, cx| {
        assert_eq!(e.read(cx).value().as_ref(), "ab1ef\r\nx\r\n\r\nab2ef")
    });
}

#[gpui::test]
fn column_selection_filters_rows_in_all_drag_directions_and_edits_only_overlaps(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let mut visual = VisualTestContext::from_window(f.window, cx);
    for scale in [1., 1.5, 2.] {
        visual.simulate_scale_factor_change(scale);
        for upward in [false, true] {
            for leftward in [false, true] {
                let e = prepare(&f, cx, "abcdef\nabc\nx\n\nabcdef\n");
                visual.update(|w, cx| w.render_frame(cx));
                let left = caret(&f, cx, &e, 2);
                let right = caret(&f, cx, &e, 5);
                let bottom = caret(&f, cx, &e, 21);
                let (x1, x2) = if leftward {
                    (right.x, left.x)
                } else {
                    (left.x, right.x)
                };
                let (y1, y2) = if upward {
                    (bottom.y, left.y)
                } else {
                    (left.y, bottom.y)
                };
                drag(&mut visual, point(x1, y1), point(x2, y2));
                let expected = if upward {
                    vec![16..19, 9..10, 2..5]
                } else {
                    vec![2..5, 9..10, 16..19]
                };
                f.update(cx, |_, _, cx| {
                    assert_eq!(e.read(cx).selected_ranges().collect::<Vec<_>>(), expected)
                });
                f.press(cx, "delete");
                f.update(cx, |_, _, cx| {
                    assert_eq!(e.read(cx).value().as_ref(), "abf\nab\nx\n\nabf\n")
                });
                f.press(cx, "ctrl-z");
                f.update(cx, |_, _, cx| {
                    assert_eq!(e.read(cx).value().as_ref(), "abcdef\nabc\nx\n\nabcdef\n");
                    assert_eq!(e.read(cx).selected_ranges().collect::<Vec<_>>(), expected);
                });
            }
        }
    }
}

#[gpui::test]
fn column_selection_retains_vscode_zero_width_and_all_outside_fallback(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let mut visual = VisualTestContext::from_window(f.window, cx);
    for width in [0., 30.] {
        let e = prepare(&f, cx, "a\n\nbb");
        visual.update(|w, cx| w.render_frame(cx));
        let top = caret(&f, cx, &e, 0);
        let bottom = caret(&f, cx, &e, 5);
        drag(
            &mut visual,
            point(top.x + px(100.), top.y),
            point(top.x + px(100. + width), bottom.y),
        );
        f.update(cx, |_, _, cx| {
            assert_eq!(
                e.read(cx).selected_ranges().collect::<Vec<_>>(),
                [1..1, 2..2, 5..5]
            )
        });
    }
    let e = prepare(&f, cx, "");
    visual.update(|w, cx| w.render_frame(cx));
    let p = caret(&f, cx, &e, 0);
    drag(
        &mut visual,
        p + point(px(100.), px(0.)),
        p + point(px(150.), px(100.)),
    );
    f.update(cx, |_, _, cx| assert_eq!(e.read(cx).selected_range(), 0..0));
}

#[gpui::test]
fn zero_width_column_adds_carets_and_mouse_release_ends_capture(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let e = prepare(&f, cx, "abcd\nabcd\nabcd");
    let mut visual = VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let start = caret(&f, cx, &e, 2);
    let end = caret(&f, cx, &e, 12);
    drag(&mut visual, start, end);
    visual.simulate_mouse_move(point(px(0.), px(0.)), None, Default::default());
    f.update(cx, |_, _, cx| {
        assert_eq!(
            e.read(cx).selected_ranges().collect::<Vec<_>>(),
            [2..2, 7..7, 12..12]
        )
    });
    f.input(cx, "!");
    f.update(cx, |_, _, cx| {
        assert_eq!(e.read(cx).value().as_ref(), "ab!cd\nab!cd\nab!cd")
    });
    visual.simulate_click(start, Default::default());
    f.update(cx, |_, _, cx| {
        assert_eq!(e.read(cx).selected_ranges().count(), 1)
    });
}

#[gpui::test]
fn column_selection_uses_wrapped_visual_rows_and_keeps_carets_at_row_starts(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let text = "word ".repeat(80);
    let e = prepare(&f, cx, &text);
    let mut visual = VisualTestContext::from_window(f.window, cx);
    visual.simulate_resize(size(px(620.), px(760.)));
    visual.update(|w, cx| w.render_frame(cx));
    let rows = f.update(cx, |_, w, cx| {
        e.update(cx, |s, cx| {
            let mut rows: Vec<(usize, Bounds<gpui::Pixels>)> = Vec::new();
            for i in 0..text.len() {
                let b = s
                    .bounds_for_range(i..i + 1, Bounds::default(), w, cx)
                    .unwrap();
                if rows.last().is_none_or(|(_, last)| last.top() != b.top()) {
                    rows.push((i, b));
                }
                if rows.len() == 3 {
                    break;
                }
            }
            rows
        })
    });
    assert_eq!(rows.len(), 3);
    let start = caret(&f, cx, &e, 1);
    let x = caret(&f, cx, &e, 3).x;
    drag(&mut visual, start, point(x, rows[2].1.center().y));
    f.update(cx, |_, _, cx| {
        assert_eq!(
            e.read(cx).selected_ranges().collect::<Vec<_>>(),
            rows.iter().map(|(i, _)| i + 1..i + 3).collect::<Vec<_>>()
        )
    });
    // A zero-width column at x=0 must paint the wrapped caret on the next row.
    let start = point(rows[0].1.left(), rows[0].1.center().y);
    drag(&mut visual, start, point(start.x, rows[2].1.center().y));
    f.update(cx, |_, w, cx| {
        e.update(cx, |s, cx| {
            assert_eq!(
                s.selected_ranges().collect::<Vec<_>>(),
                rows.iter().map(|(i, _)| *i..*i).collect::<Vec<_>>()
            );
            let at = s.cursor();
            let b = s
                .bounds_for_range(at..at, Bounds::default(), w, cx)
                .unwrap();
            assert_eq!(b.top(), rows[2].1.top());
            assert_eq!(b.left(), rows[2].1.left());
        })
    });
    f.input(cx, "!");
    f.press(cx, "ctrl-z");
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |_, w, cx| {
        e.update(cx, |s, cx| {
            let at = s.cursor();
            let b = s
                .bounds_for_range(at..at, Bounds::default(), w, cx)
                .unwrap();
            assert_eq!(b.top(), rows[2].1.top());
        })
    });
}

#[gpui::test]
fn column_selection_preserves_tabs_unicode_and_scroll_coordinates(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let e = prepare(&f, cx, "\t한글🙂e\u{301}Z\n\t한글🙂e\u{301}Z");
    let mut visual = VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let start = caret(&f, cx, &e, 1);
    let end = caret(&f, cx, &e, 16);
    drag(&mut visual, start, end);
    f.update(cx, |_, _, cx| {
        let s = e.read(cx);
        for range in s.selected_ranges() {
            assert_eq!(&s.value()[range], "한글🙂e\u{301}");
        }
    });
    f.input(cx, "X");
    f.update(cx, |_, _, cx| {
        assert_eq!(e.read(cx).value().as_ref(), "\tXZ\n\tXZ")
    });

    let long = "abcdefghij\n".repeat(100);
    let e = prepare(&f, cx, &long);
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |_, _, cx| {
        e.update(cx, |s, cx| {
            s.set_scroll_offset(point(px(0.), px(-270.)), cx)
        })
    });
    visual.update(|w, cx| w.render_frame(cx));
    let start = caret(&f, cx, &e, 10 * 11 + 2);
    let end = caret(&f, cx, &e, 12 * 11 + 5);
    drag(&mut visual, start, end);
    f.update(cx, |_, _, cx| {
        assert_eq!(
            e.read(cx).selected_ranges().collect::<Vec<_>>(),
            [112..115, 123..126, 134..137]
        )
    });
    // Release outside the editor must end capture and preserve the last selection.
    let modifiers = Modifiers {
        alt: true,
        shift: true,
        ..Default::default()
    };
    visual.simulate_event(gpui::MouseDownEvent {
        button: MouseButton::Left,
        position: start,
        modifiers,
        click_count: 1,
        ..Default::default()
    });
    visual.update(|w, cx| w.render_frame(cx));
    let outside = point(px(1100.), px(755.));
    visual.simulate_mouse_move(outside, Some(MouseButton::Left), Default::default());
    visual.simulate_mouse_up(outside, MouseButton::Left, Default::default());
    let ranges = f.update(cx, |_, _, cx| {
        e.read(cx).selected_ranges().collect::<Vec<_>>()
    });
    visual.simulate_mouse_move(start, None, Default::default());
    f.update(cx, |_, _, cx| {
        assert_eq!(e.read(cx).selected_ranges().collect::<Vec<_>>(), ranges)
    });
}
