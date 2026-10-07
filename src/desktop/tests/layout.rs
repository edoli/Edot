use super::*;

#[gpui::test]
fn tab_close_button_passes_wheel_to_tab_strip(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, w, cx| {
        for i in 0..12 {
            let id = this.book.add(0, format!("메모 {i}"));
            fs::write(this.book.file(&f.dir, id), "").unwrap();
            assert!(this.open_note(id, true, w, cx));
        }
        this.open_note(f.ids[0], true, w, cx);
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    // GPUI's test selector API requires static strings.
    let selectors: [[&'static str; 2]; 2] = [
        [
            "active-tab-title",
            Box::leak(format!("close-tab-{}", f.ids[0]).into_boxed_str()),
        ],
        [
            Box::leak(format!("tab-title-{}", f.ids[1]).into_boxed_str()),
            Box::leak(format!("close-tab-{}", f.ids[1]).into_boxed_str()),
        ],
    ];
    for (id, targets) in [f.ids[0], f.ids[1]].into_iter().zip(selectors) {
        for delta in [
            gpui::ScrollDelta::Lines(point(0., -1.)),
            gpui::ScrollDelta::Lines(point(-1., 0.)),
            gpui::ScrollDelta::Pixels(point(px(0.), px(-32.))),
            gpui::ScrollDelta::Pixels(point(px(-32.), px(0.))),
        ] {
            let mut offsets = Vec::new();
            for selector in targets {
                f.update(cx, |this, _, cx| {
                    this.tab_scroll.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                });
                visual.update(|w, cx| w.render_frame(cx));
                let position = visual.debug_bounds(selector).unwrap().center();
                visual.simulate_mouse_move(position, None, Default::default());
                visual.simulate_event(gpui::ScrollWheelEvent {
                    position,
                    delta,
                    touch_phase: gpui::TouchPhase::Started,
                    ..Default::default()
                });
                visual.update(|w, cx| w.render_frame(cx));
                offsets.push(f.update(cx, |this, _, _| {
                    assert_eq!(this.open.len(), 14);
                    assert_eq!(this.session.active, Some(f.ids[0]));
                    this.tab_scroll.offset().x
                }));
            }
            assert!(offsets[0] < px(0.));
            assert_eq!(
                offsets[0], offsets[1],
                "title/close wheel mismatch: {id}, {delta:?}"
            );
        }
    }
    f.update(cx, |this, _, cx| {
        this.tab_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    });
    visual.update(|w, cx| w.render_frame(cx));
    let close = visual.debug_bounds(selectors[1][1]).unwrap().center();
    visual.simulate_click(close, Default::default());
    f.update(cx, |this, _, _| {
        assert_eq!(this.open.len(), 13);
        assert!(!this.open.iter().any(|tab| tab.id == f.ids[1]));
        assert_eq!(this.session.active, Some(f.ids[0]));
    });
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
    assert!(tab.size.width >= px(crate::desktop::metrics::TAB_MIN_WIDTH as f32));
    assert!(title.size.width >= px(50.));
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
