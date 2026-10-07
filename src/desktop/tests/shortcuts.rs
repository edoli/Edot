use super::*;

#[gpui::test]
fn f1_help_blocks_background_input_and_restores_editor_focus(cx: &mut TestAppContext) {
    use gpui::Focusable;
    let f = Fixture::new(cx);
    let (editor, text) = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_selected_range(0..4, cx);
            s.focus(w, cx);
        });
        let text = editor.read(cx).value();
        (editor, text)
    });
    f.press(cx, "f1");
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("shortcut-help").is_some());
    for key in [
        "ctrl-n",
        "ctrl-f4",
        "ctrl-h",
        "ctrl-a",
        "delete",
        "tab",
        "shift-tab",
    ] {
        f.press(cx, key);
    }
    f.input(cx, "unchanged");
    visual.simulate_click(point(px(20.), px(400.)), Default::default());
    visual.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse("f1").unwrap(),
        is_held: true,
        prefer_character_input: false,
    });
    f.update(cx, |this, _, cx| {
        assert!(this.shortcut_help.is_some());
        assert_eq!(this.open.len(), 2);
        assert!(!this.find_visible);
        assert_eq!(editor.read(cx).value(), text);
        assert_eq!(editor.read(cx).selected_range(), 0..4);
    });
    f.press(cx, "escape");
    f.update(cx, |this, w, cx| {
        assert!(this.shortcut_help.is_none());
        assert!(editor.read(cx).focus_handle(cx).is_focused(w));
        assert_eq!(editor.read(cx).selected_range(), 0..4);
    });
    f.press(cx, "f1");
    f.press(cx, "f1");
    f.update(cx, |this, _, _| assert!(this.shortcut_help.is_none()));
}

#[gpui::test]
fn shortcut_help_restores_search_and_opens_from_settings_or_empty_tabs(cx: &mut TestAppContext) {
    use gpui::Focusable;
    let f = Fixture::new(cx);
    f.press(cx, "ctrl-f");
    f.press(cx, "f1");
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let close = visual.debug_bounds("shortcut-close").unwrap().center();
    visual.simulate_click(close, Default::default());
    f.update(cx, |this, w, cx| {
        assert!(this.shortcut_help.is_none());
        assert!(this.local.query.read(cx).focus_handle(cx).is_focused(w));
        this.show_settings(w, cx);
    });
    f.press(cx, "f1");
    f.update(cx, |this, _, _| {
        assert!(!this.settings_visible);
        assert!(this.shortcut_help.is_some());
    });
    f.press(cx, "enter");
    f.update(cx, |this, w, cx| {
        assert!(this.shortcut_help.is_none());
        assert!(
            this.active_editor()
                .unwrap()
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
    });
    f.press(cx, "ctrl-w");
    f.press(cx, "ctrl-w");
    f.press(cx, "f1");
    f.update(cx, |this, _, _| {
        assert!(this.open.is_empty());
        assert!(this.shortcut_help.is_some());
    });
    f.press(cx, "escape");
    f.press(cx, "ctrl-shift-t");
    f.update(cx, |this, _, _| assert_eq!(this.open.len(), 1));
}

#[gpui::test]
fn shortcut_table_scrolls_and_fits_small_windows_at_hidpi(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.press(cx, "f1");
    let help = f.update(cx, |this, _, _| this.shortcut_help.clone().unwrap());
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    for viewport in [size(px(1100.), px(760.)), size(px(520.), px(420.))] {
        visual.simulate_resize(viewport);
        for scale in [1., 1.5, 2.] {
            visual.simulate_scale_factor_change(scale);
            f.press(cx, "home");
            visual.update(|w, cx| w.render_frame(cx));
            let panel = visual.debug_bounds("shortcut-help").unwrap();
            let rows = visual.debug_bounds("shortcut-rows").unwrap();
            let header = visual.debug_bounds("shortcut-table-header").unwrap();
            assert!(panel.left() >= px(0.) && panel.right() <= viewport.width);
            assert!(panel.top() >= px(0.) && panel.bottom() <= viewport.height);
            assert!(rows.size.height > px(100.) && rows.bottom() < panel.bottom());
            // Converting snapped physical coordinates back to f32 logical pixels
            // can differ by a few millionths at fractional DPI.
            assert!(
                header.bottom() <= rows.top() + px(0.001),
                "scale {scale}: header={header:?}, rows={rows:?}"
            );
            f.press(cx, "pagedown");
            f.update(cx, |_, _, cx| {
                assert!(help.read(cx).scroll.offset().y < px(0.))
            });
            f.press(cx, "end");
            f.update(cx, |_, _, cx| {
                let scroll = &help.read(cx).scroll;
                assert!(scroll.max_offset().y > rows.size.height);
                assert_eq!(scroll.offset().y, -scroll.max_offset().y);
            });
            visual.update(|w, cx| w.render_frame(cx));
            assert_eq!(
                visual.debug_bounds("shortcut-table-header").unwrap(),
                header
            );
            f.press(cx, "home");
            visual.simulate_event(gpui::ScrollWheelEvent {
                position: rows.center(),
                delta: gpui::ScrollDelta::Lines(point(0., -3.)),
                ..Default::default()
            });
            visual.update(|w, cx| w.render_frame(cx));
            f.update(cx, |_, _, cx| {
                assert!(help.read(cx).scroll.offset().y < px(0.))
            });
        }
    }
}
