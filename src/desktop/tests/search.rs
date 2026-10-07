use super::*;

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
        assert!(!this.search_results.scanning);
        assert_eq!(this.search_results.groups[0].count, 2);
        this.search_results.generation
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
        assert_eq!(this.search_results.generation, generation);
        assert!(!this.search_results.scanning);
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
        this.search_results.collapsed.insert(f.ids[2]);
        this.rebuild_results();
        let group = this
            .search_results
            .groups
            .iter()
            .position(|r| r.id == f.ids[1])
            .unwrap();
        let index = this
            .search_results
            .visible
            .iter()
            .position(|r| r.group == group && r.hit == Some(10))
            .unwrap();
        this.search_results.cursor = Some(index);
        let selected = this.result_identity(index).unwrap();
        let top = index - 2;
        let offset = point(
            px(0.),
            -px(top as f32 * crate::desktop::metrics::SEARCH_ROW_HEIGHT + 3.),
        );
        this.search_results
            .scroll
            .0
            .borrow()
            .base_handle
            .set_offset(offset);
        (
            this.search_results.generation,
            this.search_results.groups[group].matches[0].preview.clone(),
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
        assert!(!this.search_results.scanning);
        assert_eq!(
            this.search_results.groups[0].count, 20,
            "keep old results during debounce"
        );
        assert_eq!(this.search_results.generation, generation);
    });
    settle_search(cx);
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| {
        assert_eq!(this.search_results.groups[0].count, 25);
        let other = this
            .search_results
            .groups
            .iter()
            .find(|r| r.id == f.ids[1])
            .unwrap();
        assert!(
            std::sync::Arc::ptr_eq(&other.matches[0].preview, &other_preview),
            "unmodified note was rescanned"
        );
        assert_eq!(
            this.search_results
                .cursor
                .and_then(|i| this.result_identity(i)),
            Some(selected)
        );
        assert!(this.search_results.collapsed.contains(&f.ids[2]));
        let new_offset = this.search_results.scroll.0.borrow().base_handle.offset();
        assert_eq!(
            new_offset.y,
            offset.y - px(5. * crate::desktop::metrics::SEARCH_ROW_HEIGHT)
        );
        let top = (f32::from(-new_offset.y) / crate::desktop::metrics::SEARCH_ROW_HEIGHT).floor()
            as usize;
        assert_eq!(this.result_identity(top), Some(anchor));
    });
    // Rapid edits use the latest revision; removing/re-adding a group preserves document order.
    f.update(cx, |_, w, cx| {
        editor.update(cx, |s, cx| s.replace_all("none", w, cx))
    });
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert!(!this.search_results.groups.iter().any(|r| r.id == f.ids[0]))
    });
    f.update(cx, |_, w, cx| {
        editor.update(cx, |s, cx| s.replace_all("needle stale needle", w, cx))
    });
    f.update(cx, |_, w, cx| {
        editor.update(cx, |s, cx| s.replace_all("needle latest", w, cx))
    });
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert_eq!(this.search_results.groups[0].id, f.ids[0]);
        assert_eq!(this.search_results.groups[0].count, 1);
        assert!(
            this.search_results.groups[0].matches[0]
                .preview
                .contains("latest")
        );
        assert_eq!(this.search_results.generation, generation);
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
        assert_eq!(this.search_results.groups.len(), 1);
        assert_eq!(this.search_results.groups[0].id, f.ids[1]);
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
        assert!(this.search_results.scanning);
        first.update(cx, |s, cx| s.replace_all("needle during scan", w, cx));
        second.update(cx, |s, cx| s.replace_all("needle second", w, cx));
    });
    settle_search(cx);
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert!(!this.search_results.scanning);
        assert!(this.search_results.pending_notes.is_empty());
        assert_eq!(this.search_results.groups.len(), 3);
        assert!(
            this.search_results.groups[0].matches[0]
                .preview
                .contains("during scan")
        );
    });
    f.update(cx, |_, w, cx| {
        first.update(cx, |s, cx| s.replace_all("needle one needle two", w, cx));
        second.update(cx, |s, cx| s.replace_all("no match", w, cx));
    });
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert_eq!(this.search_results.groups.len(), 2);
        assert_eq!(this.search_results.groups[0].id, f.ids[0]);
        assert_eq!(this.search_results.groups[0].count, 2);
        assert_eq!(this.search_results.groups[1].id, f.ids[2]);
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
        assert_eq!(this.search_results.groups.len(), 2);
    });
    settle_search(cx);
    f.update(cx, |this, _, _| {
        assert_eq!(this.search_results.groups[1].count, 2);
        assert!(
            this.search_results.groups[1].matches[0]
                .preview
                .contains("updated")
        );
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
            this.search_results
                .groups
                .iter()
                .map(|r| r.matches.len())
                .sum::<usize>(),
            crate::global_search::MAX_RESULTS
        );
        assert_eq!(this.search_results.groups[1].matches.len(), 0);
    });
    f.update(cx, |_, w, cx| {
        editor.update(cx, |s, cx| s.replace_all("removed", w, cx))
    });
    for _ in 0..3 {
        settle_search(cx);
    }
    f.update(cx, |this, _, _| {
        assert_eq!(this.search_results.groups.len(), 2);
        assert!(
            this.search_results
                .groups
                .iter()
                .all(|r| r.count == 1 && r.matches.len() == 1)
        );
        assert!(this.search_results.note_tasks.is_empty());
    });
}
