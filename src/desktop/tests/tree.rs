use super::*;

#[gpui::test]
fn rename_pasted_tabs_and_newlines_become_spaces(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.begin_rename(f.ids[0], window, cx);
        this.title_input.update(cx, |state, cx| {
            state.replace_all("  한글\t제목\r\n새 줄\r끝\n😀  ", window, cx);
        });
        assert!(this.finish_rename(true, true, window, cx));
        assert_eq!(
            this.book.get(f.ids[0]).unwrap().title,
            "한글 제목 새 줄 끝 😀"
        );
        assert!(!this.open[0].dirty);
    });
    let index = fs::read_to_string(f.dir.join("tree.tsv")).unwrap();
    assert!(index.starts_with("EDOT2\n"));
    assert!(index.contains("\t한글 제목 새 줄 끝 😀\n"));
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[0]))).unwrap(),
        "🙂 한글12\r\n한글3\rkeep\n"
    );
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

fn add_drag_note(this: &mut Edot, parent: u64, title: &str) -> u64 {
    let id = this.book.add(parent, title.into());
    fs::write(this.book.file(&this.dir, id), title).unwrap();
    this.book.save(&this.dir).unwrap();
    id
}

fn tree_note_selector(id: u64) -> &'static str {
    // GPUI test selectors require a static string. Only used by this test process.
    format!("tree-note-{id}").leak()
}

fn begin_note_drag(visual: &mut gpui::VisualTestContext, id: u64) {
    visual.update(|w, cx| w.render_frame(cx));
    let bounds = visual.debug_bounds(tree_note_selector(id)).unwrap();
    let start = point(bounds.right() - px(30.), bounds.center().y);
    visual.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    visual.simulate_mouse_move(
        start + point(px(8.), px(0.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    visual.update(|w, cx| {
        assert!(
            cx.has_active_drag(),
            "mouse gesture must start a real GPUI drag"
        );
        w.render_frame(cx);
    });
}

fn move_note_drag(visual: &mut gpui::VisualTestContext, position: gpui::Point<gpui::Pixels>) {
    visual.simulate_mouse_move(
        position,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    visual.update(|w, cx| w.render_frame(cx));
}

#[gpui::test]
fn tree_drag_matches_win32_folder_boundaries_markers_and_saved_moves(cx: &mut TestAppContext) {
    use crate::drag::DropZone;
    let f = Fixture::new(cx);
    let (source, original) = f.update(cx, |this, _, cx| {
        let source = add_drag_note(this, 0, "드래그할 메모");
        cx.notify();
        (source, this.book.clone())
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    for scale in [1., 1.25, 1.5, 2.] {
        visual.simulate_scale_factor_change(scale);
        for zone in [
            DropZone::InsideFirst,
            DropZone::InsideLast,
            DropZone::AfterSubtree,
        ] {
            f.update(cx, |this, _, cx| {
                this.book = original.clone();
                this.book.save(&this.dir).unwrap();
                this.collapsed.clear();
                this.selection.single(source);
                cx.notify();
            });
            begin_note_drag(&mut visual, source);
            let folder = visual.debug_bounds(tree_note_selector(f.ids[0])).unwrap();
            let child = visual.debug_bounds(tree_note_selector(f.ids[2])).unwrap();
            let next = visual.debug_bounds(tree_note_selector(f.ids[1])).unwrap();
            let position = point(
                folder.right() - px(40.),
                match zone {
                    DropZone::InsideFirst => folder.bottom() - px(1.),
                    DropZone::InsideLast => child.bottom() - px(1.),
                    _ => next.top() + px(1.),
                },
            );
            move_note_drag(&mut visual, position);
            let marker = visual.debug_bounds("tree-insertion-marker").unwrap();
            let expected_left = folder.left()
                + px(24.
                    + if zone == DropZone::AfterSubtree {
                        0.
                    } else {
                        22.
                    });
            let expected_bottom = if zone == DropZone::InsideFirst {
                folder.bottom()
            } else {
                child.bottom()
            } - px(1.);
            // Layout edges snap independently to physical pixels at fractional DPI.
            assert!(
                (marker.left() - expected_left).abs() <= px(1. / scale),
                "{marker:?}, expected x={expected_left:?}"
            );
            assert!(
                (marker.bottom() - expected_bottom).abs() <= px(1. / scale),
                "{marker:?}, expected y={expected_bottom:?}"
            );
            f.update(cx, |this, _, cx| {
                let target = this.drop_target.unwrap();
                assert_eq!(target.zone, zone);
                assert_eq!(
                    target.anchor,
                    Some(if zone == DropZone::InsideFirst {
                        f.ids[0]
                    } else {
                        f.ids[2]
                    })
                );
                assert_eq!(
                    cx.active_drag_cursor_style(),
                    Some(gpui::CursorStyle::ClosedHand)
                );
                let description = this.tree_drag_description().unwrap();
                assert!(description.contains(match zone {
                    DropZone::InsideFirst => "안의 맨 위",
                    DropZone::InsideLast => "안의 맨 아래",
                    _ => "같은 레벨",
                }));
            });
            visual.simulate_mouse_up(
                position,
                gpui::MouseButton::Left,
                gpui::Modifiers::default(),
            );
            visual.update(|w, cx| w.render_frame(cx));
            f.update(cx, |this, _, cx| {
                assert!(!cx.has_active_drag());
                assert!(this.dragging_notes.is_empty());
                let saved = Notebook::load(&this.dir).unwrap();
                match zone {
                    DropZone::InsideFirst => {
                        assert_eq!(saved.children(f.ids[0]), vec![source, f.ids[2]])
                    }
                    DropZone::InsideLast => {
                        assert_eq!(saved.children(f.ids[0]), vec![f.ids[2], source])
                    }
                    _ => assert_eq!(saved.children(0), vec![f.ids[0], source, f.ids[1]]),
                }
            });
        }
    }
}

#[gpui::test]
fn tree_drag_shared_gaps_and_offscreen_ancestor_feedback(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let (source, last, original) = f.update(cx, |this, _, cx| {
        let mut last = f.ids[2];
        for _ in 0..35 {
            last = add_drag_note(this, f.ids[0], "폴더 안의 메모");
        }
        let source = add_drag_note(this, 0, "이동할 메모");
        cx.notify();
        (source, last, this.book.clone())
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| {
        let bounds = this.tree_scroll.0.borrow().base_handle.bounds();
        // Adjacent quarter-rows must resolve to exactly one shared target.
        for boundary in [1, 2] {
            let y = bounds.top() + px(boundary as f32 * 29.);
            let a = this
                .tree_drop_at(point(bounds.center().x, y - px(1.)), &[source])
                .unwrap();
            let b = this
                .tree_drop_at(point(bounds.center().x, y + px(1.)), &[source])
                .unwrap();
            assert_eq!(a, b);
        }
        this.tree_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(-29. * 20.)));
    });
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds(tree_note_selector(f.ids[0])).is_none());
    begin_note_drag(&mut visual, source);
    let last_row = visual.debug_bounds(tree_note_selector(last)).unwrap();
    let next_row = visual.debug_bounds(tree_note_selector(f.ids[1])).unwrap();
    move_note_drag(
        &mut visual,
        point(last_row.center().x, last_row.bottom() - px(1.)),
    );
    let inside = visual.debug_bounds("tree-insertion-marker").unwrap();
    move_note_drag(
        &mut visual,
        point(next_row.center().x, next_row.top() + px(1.)),
    );
    let outside = visual.debug_bounds("tree-insertion-marker").unwrap();
    assert_eq!(inside.left(), outside.left() + px(22.));
    assert_eq!(inside.top(), outside.top());
    f.press(cx, "escape");
    f.update(cx, |this, _, cx| {
        assert!(!cx.has_active_drag());
        assert!(this.drop_target.is_none());
        assert_eq!(this.book.children(0), original.children(0));
    });
}

#[gpui::test]
fn tree_drag_outside_forbidden_and_root_drop_preserve_selection(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    begin_note_drag(&mut visual, f.ids[0]);
    let child = visual.debug_bounds(tree_note_selector(f.ids[2])).unwrap();
    move_note_drag(&mut visual, child.center());
    f.update(cx, |this, _, cx| {
        assert!(
            this.drop_target.is_none(),
            "cannot move a parent into its own child"
        );
        assert_eq!(
            cx.active_drag_cursor_style(),
            Some(gpui::CursorStyle::OperationNotAllowed)
        );
    });
    let host = visual.debug_bounds("tree-host").unwrap();
    let outside = point(host.right() + px(40.), child.center().y);
    move_note_drag(&mut visual, outside);
    assert!(visual.debug_bounds("tree-insertion-marker").is_none());
    f.update(cx, |this, _, _| assert!(this.drop_target.is_none()));
    visual.simulate_mouse_up(outside, gpui::MouseButton::Left, gpui::Modifiers::default());
    f.update(cx, |this, _, _| {
        assert_eq!(this.book.get(f.ids[2]).unwrap().parent, f.ids[0]);
        assert!(this.dragging_notes.is_empty());
    });
    begin_note_drag(&mut visual, f.ids[2]);
    let blank = point(host.center().x, host.top() + px(240.));
    move_note_drag(&mut visual, blank);
    assert!(visual.debug_bounds("tree-root-drop").is_some());
    visual.simulate_mouse_up(blank, gpui::MouseButton::Left, gpui::Modifiers::default());
    f.update(cx, |this, _, _| {
        assert_eq!(this.book.children(0), vec![f.ids[0], f.ids[1], f.ids[2]]);
        assert_eq!(
            Notebook::load(&this.dir)
                .unwrap()
                .get(f.ids[2])
                .unwrap()
                .parent,
            0
        );
    });
    // Drag a selected group without collapsing the selection at mouse-down.
    f.update(cx, |this, _, cx| {
        this.selection.all(&[f.ids[1], f.ids[2]]);
        cx.notify();
    });
    begin_note_drag(&mut visual, f.ids[1]);
    let folder = visual.debug_bounds(tree_note_selector(f.ids[0])).unwrap();
    move_note_drag(&mut visual, folder.center());
    assert!(visual.debug_bounds("tree-inside-drop").is_some());
    f.update(cx, |this, _, _| {
        assert_eq!(this.dragging_notes, vec![f.ids[1], f.ids[2]])
    });
    visual.simulate_mouse_up(
        folder.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    f.update(cx, |this, _, _| {
        assert_eq!(this.book.children(f.ids[0]), vec![f.ids[1], f.ids[2]]);
        assert_eq!(this.selection.ids.len(), 2);
    });
}

#[gpui::test]
fn tree_drag_autoscroll_uses_win32_interval_and_stops_outside(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, _, cx| {
        for _ in 0..60 {
            add_drag_note(this, 0, "스크롤 메모");
        }
        cx.notify();
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    begin_note_drag(&mut visual, f.ids[0]);
    let host = visual.debug_bounds("tree-host").unwrap();
    move_note_drag(&mut visual, point(host.center().x, host.bottom() - px(10.)));
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(159));
    cx.run_until_parked();
    f.update(cx, |this, _, _| {
        assert_eq!(this.tree_scroll.0.borrow().base_handle.offset().y, px(0.))
    });
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(1));
    cx.run_until_parked();
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| {
        assert_eq!(this.tree_scroll.0.borrow().base_handle.offset().y, px(-58.))
    });
    let outside = point(host.right() + px(20.), host.bottom() - px(10.));
    move_note_drag(&mut visual, outside);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(320));
    cx.run_until_parked();
    f.update(cx, |this, _, _| {
        assert_eq!(this.tree_scroll.0.borrow().base_handle.offset().y, px(-58.));
        assert!(this.drop_target.is_none());
    });
    visual.simulate_mouse_up(outside, gpui::MouseButton::Left, gpui::Modifiers::default());
}

#[gpui::test]
fn tree_drag_release_rechecks_position_and_deactivation_cancels(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, _| w.activate_window());
    begin_note_drag(&mut visual, f.ids[2]);
    let folder = visual.debug_bounds(tree_note_selector(f.ids[0])).unwrap();
    move_note_drag(&mut visual, folder.center());
    let host = visual.debug_bounds("tree-host").unwrap();
    // Release at a different position without an intermediate mouse-move event.
    let blank = point(host.center().x, host.top() + px(240.));
    visual.simulate_mouse_up(blank, gpui::MouseButton::Left, gpui::Modifiers::default());
    f.update(cx, |this, _, _| {
        assert_eq!(this.book.get(f.ids[2]).unwrap().parent, 0)
    });
    begin_note_drag(&mut visual, f.ids[2]);
    move_note_drag(&mut visual, folder.center());
    visual.deactivate_window();
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, cx| {
        assert!(!cx.has_active_drag());
        assert!(this.dragging_notes.is_empty());
        assert!(this.drop_target.is_none());
        assert_eq!(
            Notebook::load(&this.dir)
                .unwrap()
                .get(f.ids[2])
                .unwrap()
                .parent,
            0
        );
    });
}
