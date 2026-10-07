// Assertions intentionally compare lists of selection ranges, including a single range.
#![allow(clippy::single_range_in_vec_init)]
use super::*;
use crate::widgets::input::InputState;

fn editor(f: &Fixture, cx: &mut TestAppContext, text: &str, at: usize) -> Entity<InputState> {
    f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value(text.to_owned(), w, cx);
            s.set_selected_range(at..at, cx);
            s.focus(w, cx);
        });
        editor
    })
}

fn assert_state(
    f: &Fixture,
    cx: &mut TestAppContext,
    editor: &Entity<InputState>,
    text: &str,
    ranges: &[std::ops::Range<usize>],
) {
    f.update(cx, |_, _, cx| {
        let s = editor.read(cx);
        assert_eq!(s.value().as_ref(), text);
        assert_eq!(s.selected_ranges().collect::<Vec<_>>(), ranges);
    });
}

#[gpui::test]
fn control_d_occurrences_wrap_skip_duplicates_and_respect_manual_substrings(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let e = editor(&f, cx, "one stone one One one", 11);
    f.press(cx, "ctrl-d");
    assert_state(&f, cx, &e, "one stone one One one", &[10..13]);
    f.press(cx, "ctrl-d");
    f.press(cx, "ctrl-d");
    assert_state(&f, cx, &e, "one stone one One one", &[10..13, 18..21, 0..3]);
    f.press(cx, "ctrl-d");
    assert_state(&f, cx, &e, "one stone one One one", &[10..13, 18..21, 0..3]);
    f.press(cx, "escape");
    assert_state(&f, cx, &e, "one stone one One one", &[10..13]);
    f.update(cx, |_, _, cx| {
        e.update(cx, |s, cx| s.set_selected_range(0..3, cx))
    });
    f.press(cx, "ctrl-d");
    assert_state(&f, cx, &e, "one stone one One one", &[0..3, 6..9]);
    f.press(cx, "ctrl-a");
    assert_state(&f, cx, &e, "one stone one One one", &[0..21]);
}

#[gpui::test]
fn simultaneous_edits_restore_text_and_all_selections_with_undo_redo(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let e = editor(&f, cx, "🙂 a 🙂 a 🙂", 0);
    for _ in 0..3 {
        f.press(cx, "ctrl-d");
    }
    assert_state(&f, cx, &e, "🙂 a 🙂 a 🙂", &[0..4, 7..11, 14..18]);
    f.input(cx, "x");
    assert_state(&f, cx, &e, "x a x a x", &[1..1, 5..5, 9..9]);
    f.input(cx, "y");
    assert_state(&f, cx, &e, "xy a xy a xy", &[2..2, 7..7, 12..12]);
    f.press(cx, "ctrl-z");
    assert_state(&f, cx, &e, "x a x a x", &[1..1, 5..5, 9..9]);
    f.press(cx, "ctrl-z");
    assert_state(&f, cx, &e, "🙂 a 🙂 a 🙂", &[0..4, 7..11, 14..18]);
    f.press(cx, "ctrl-y");
    f.press(cx, "ctrl-y");
    f.press(cx, "shift-left");
    assert_state(&f, cx, &e, "xy a xy a xy", &[1..2, 6..7, 11..12]);
    f.press(cx, "backspace");
    assert_state(&f, cx, &e, "x a x a x", &[1..1, 5..5, 9..9]);
    f.press(cx, "left");
    f.press(cx, "delete");
    assert_state(&f, cx, &e, " a  a ", &[0..0, 3..3, 6..6]);
    f.press(cx, "ctrl-z");
    assert_state(&f, cx, &e, "x a x a x", &[0..0, 4..4, 8..8]);
}

#[gpui::test]
fn multiple_selections_copy_cut_paste_and_merge_coincident_carets(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let e = editor(&f, cx, "aa bb aa", 0);
    f.press(cx, "ctrl-d");
    f.press(cx, "ctrl-d");
    f.press(cx, "ctrl-c");
    cx.update(|cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("aa\naa")
        )
    });
    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("한글\r\n🙂".into())));
    f.press(cx, "ctrl-v");
    assert_state(&f, cx, &e, "한글 bb 🙂", &[6..6, 14..14]);
    f.press(cx, "ctrl-z");
    f.press(cx, "ctrl-x");
    assert_state(&f, cx, &e, " bb ", &[0..0, 4..4]);
    f.press(cx, "ctrl-v");
    assert_state(&f, cx, &e, "aa bb aa", &[2..2, 8..8]);
    f.press(cx, "home");
    assert_state(&f, cx, &e, "aa bb aa", &[0..0]);
    f.input(cx, "!");
    assert_state(&f, cx, &e, "!aa bb aa", &[1..1]);
    let e = editor(&f, cx, "x\r\nx", 0);
    f.press(cx, "ctrl-d");
    f.press(cx, "ctrl-d");
    f.press(cx, "right");
    f.press(cx, "ctrl-c");
    cx.update(|cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("x\r\nx")
        )
    });
    f.press(cx, "ctrl-x");
    assert_state(&f, cx, &e, "", &[0..0]);
    f.press(cx, "ctrl-z");
    assert_state(&f, cx, &e, "x\r\nx", &[1..1, 4..4]);
}

#[gpui::test]
fn multiple_selections_ime_composition_is_mirrored_and_one_undo(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    let f = Fixture::new(cx);
    for explicit in [false, true] {
        let e = editor(&f, cx, "word\r\nword", 0);
        f.press(cx, "ctrl-d");
        f.press(cx, "ctrl-d");
        f.update(cx, |_, w, cx| {
            e.update(cx, |s, cx| {
                for text in ["ㅎ", "하", "한"] {
                    let range = explicit.then(|| {
                        s.marked_text_range(w, cx)
                            .unwrap_or_else(|| s.selected_text_range(false, w, cx).unwrap().range)
                    });
                    s.replace_and_mark_text_in_range(range, text, Some(1..1), w, cx);
                }
                let range = explicit.then(|| s.marked_text_range(w, cx).unwrap());
                s.replace_text_in_range(range, "한", w, cx);
            })
        });
        assert_state(&f, cx, &e, "한\r\n한", &[3..3, 8..8]);
        f.press(cx, "ctrl-z");
        assert_state(&f, cx, &e, "word\r\nword", &[0..4, 6..10]);
        f.press(cx, "ctrl-y");
        assert_state(&f, cx, &e, "한\r\n한", &[3..3, 8..8]);
        f.press(cx, "ctrl-d");
        assert_state(&f, cx, &e, "한\r\n한", &[0..3, 5..8]);
    }
}

#[gpui::test]
fn multiple_carets_merge_typing_history_and_overlapping_selections(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let e = editor(&f, cx, "x x x", 0);
    for _ in 0..3 {
        f.press(cx, "ctrl-d");
    }
    f.press(cx, "right");
    f.input(cx, "abc");
    assert_state(&f, cx, &e, "xabc xabc xabc", &[4..4, 9..9, 14..14]);
    f.press(cx, "ctrl-z");
    assert_state(&f, cx, &e, "x x x", &[1..1, 3..3, 5..5]);
    f.press(cx, "ctrl-y");
    assert_state(&f, cx, &e, "xabc xabc xabc", &[4..4, 9..9, 14..14]);
    f.press(cx, "ctrl-shift-home");
    assert_state(&f, cx, &e, "xabc xabc xabc", &[0..14]);
    f.input(cx, "끝");
    assert_state(&f, cx, &e, "끝", &[3..3]);
    f.press(cx, "ctrl-z");
    assert_state(&f, cx, &e, "xabc xabc xabc", &[0..14]);
}

#[gpui::test]
fn occurrence_highlights_survive_help_tabs_and_reveal_the_next_match(cx: &mut TestAppContext) {
    use crate::widgets::ActiveTheme;
    let f = Fixture::new(cx);
    let e = editor(&f, cx, "한국어 한국어", 0);
    f.press(cx, "ctrl-d");
    f.press(cx, "ctrl-d");
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let body = visual.debug_bounds("body-editor").unwrap().scale(1.);
    f.update(cx, |_, w, cx| {
        let highlights: Vec<_> = w
            .painted_quads()
            .into_iter()
            .filter(|q| {
                q.background == cx.theme().selection.into() && body.contains(&q.bounds.origin)
            })
            .collect();
        assert_eq!(highlights.len(), 2);
        assert!(highlights[0].bounds.right() < highlights[1].bounds.left());
    });
    f.press(cx, "f1");
    f.press(cx, "escape");
    f.press(cx, "ctrl-tab");
    f.press(cx, "ctrl-shift-tab");
    assert_state(&f, cx, &e, "한국어 한국어", &[0..9, 10..19]);
    let long = format!("word\n{}word", "line\n".repeat(100));
    let e = editor(&f, cx, &long, 0);
    f.press(cx, "ctrl-d");
    f.press(cx, "ctrl-d");
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |_, _, cx| {
        assert!(e.read(cx).scroll_offset().y < px(-500.));
    });
    f.press(cx, "escape");
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |_, _, cx| {
        assert_eq!(e.read(cx).scroll_offset().y, px(0.));
    });
}
