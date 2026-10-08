use super::*;

#[gpui::test]
fn global_settings_reload_on_activation_keeps_edits_and_rejects_broken_ini(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let (editor, selection) = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| {
            state.replace_all("global settings retain unsaved edits", window, cx)
        });
        this.changed(f.ids[0], window, cx);
        let selection = editor.read(cx).selected_range();
        (editor, selection)
    });
    let external = Settings {
        theme: ThemeMode::Dark,
        font_size: 29,
        wrap: false,
        ..Default::default()
    };
    visual.deactivate_window();
    external.save(&f.dir.join("test-global-config")).unwrap();
    visual.update(|window, _| window.activate_window());
    cx.run_until_parked();
    f.update(cx, |this, _, cx| {
        assert_eq!(this.settings, external);
        assert_eq!(this.active_editor().unwrap(), editor);
        assert_eq!(editor.read(cx).selected_range(), selection);
        assert_eq!(
            editor.read(cx).value(),
            "global settings retain unsaved edits"
        );
        assert!(this.open.iter().any(|tab| tab.dirty));
    });
    visual.deactivate_window();
    let path = f.dir.join("test-global-config/settings.ini");
    fs::write(&path, "theme = [").unwrap();
    visual.update(|window, _| window.activate_window());
    cx.run_until_parked();
    f.update(cx, |this, _, _| {
        assert_eq!(this.settings, external);
        assert!(this.error.is_some());
    });
    assert_eq!(fs::read_to_string(path).unwrap(), "theme = [");
}

#[gpui::test]
fn wrap_switch_only_toggles_near_status_and_track(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, w, cx| this.show_settings(w, cx));
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let label = visual.debug_bounds("settings-wrap-label").unwrap();
    let control = visual.debug_bounds("settings-wrap").unwrap();
    let track = visual.debug_bounds("settings-wrap-track").unwrap();
    for x in [
        label.left() + px(10.),
        label.right() - px(10.),
        control.left() - px(2.),
    ] {
        visual.simulate_click(point(x, control.center().y), Default::default());
        visual.update(|w, cx| w.render_frame(cx));
        f.update(cx, |this, _, _| {
            assert!(this.settings_visible);
            assert!(this.settings.wrap);
        });
    }
    // The status text and the track are both interactive.
    visual.simulate_click(
        point(control.left() + px(14.), control.center().y),
        Default::default(),
    );
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| assert!(!this.settings.wrap));
    visual.simulate_click(track.center(), Default::default());
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| assert!(this.settings.wrap));
}

#[gpui::test]
fn wrap_switch_animates_reverses_and_respects_reduced_motion(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| this.show_settings(window, cx));
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let right = visual.debug_bounds("settings-wrap-thumb").unwrap().left();

    f.update(cx, |this, window, cx| {
        let mut settings = this.settings.clone();
        settings.wrap = !settings.wrap;
        this.change_settings(settings, window, cx);
    });
    visual.update(|window, cx| window.render_frame(cx));
    let start = visual.debug_bounds("settings-wrap-thumb").unwrap().left();
    assert!(start <= right && start > right - px(3.));
    visual.update(|window, cx| window.render_frame(cx));
    let middle = visual.debug_bounds("settings-wrap-thumb").unwrap().left();
    assert!(middle < right - px(1.) && middle > right - px(17.));

    f.update(cx, |this, window, cx| {
        let mut settings = this.settings.clone();
        settings.wrap = !settings.wrap;
        this.change_settings(settings, window, cx);
    });
    visual.update(|window, cx| window.render_frame(cx));
    let reversed = visual.debug_bounds("settings-wrap-thumb").unwrap().left();
    assert!(reversed < right && reversed > right - px(18.));
    for _ in 0..60 {
        visual.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        });
    }
    assert!((visual.debug_bounds("settings-wrap-thumb").unwrap().left() - right).abs() < px(0.1));

    cx.update(|cx| cx.set_reduce_motion(true));
    f.update(cx, |this, window, cx| {
        let mut settings = this.settings.clone();
        settings.wrap = !settings.wrap;
        this.change_settings(settings, window, cx);
    });
    visual.update(|window, cx| window.render_frame(cx));
    assert!(
        (visual.debug_bounds("settings-wrap-thumb").unwrap().left() - (right - px(18.))).abs()
            < px(0.1)
    );
    f.update(cx, |this, _, _| assert!(!this.settings.wrap));
}

#[gpui::test]
fn settings_row_units_and_wrap_switch_work_without_desktop_input(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        this.show_settings(window, cx);
        let size = this.preferences.as_ref().unwrap().size.clone();
        size.update(cx, |state, cx| state.focus(window, cx));
        this.active_editor().unwrap()
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let font = visual.debug_bounds("settings-font").unwrap();
    let size = visual.debug_bounds("settings-size").unwrap();
    let panel = visual.debug_bounds("settings-panel").unwrap();
    assert_eq!(font.top(), size.top());
    assert!(font.right() < size.left());
    assert!(size.right() < panel.right());
    f.press(cx, "enter");
    f.press(cx, "down");
    f.press(cx, "enter");
    f.update(cx, |this, _, _| {
        assert_eq!(this.settings.font_size, 19);
        assert_eq!(
            Settings::load(&f.dir.join("test-global-config"))
                .unwrap()
                .font_size,
            19
        );
    });
    f.press(cx, "tab");
    f.update(cx, |this, window, _| {
        assert!(
            this.preferences
                .as_ref()
                .unwrap()
                .wrap_focus
                .is_focused(window)
        );
    });
    f.press(cx, "space");
    f.update(cx, |this, _, cx| {
        assert!(!this.settings.wrap);
        assert!(
            !Settings::load(&f.dir.join("test-global-config"))
                .unwrap()
                .wrap
        );
        assert_eq!(this.active_editor().unwrap(), editor);
        assert_eq!(editor.read(cx).value(), "🙂 한글12\r\n한글3\rkeep\n");
    });
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert!(this.settings.wrap));
    visual.update(|window, cx| window.render_frame(cx));
    let wrap = visual.debug_bounds("settings-wrap").unwrap();
    visual.simulate_click(wrap.center(), gpui::Modifiers::default());
    f.update(cx, |this, _, _| {
        assert!(!this.settings.wrap);
        assert!(
            !Settings::load(&f.dir.join("test-global-config"))
                .unwrap()
                .wrap
        );
    });
}

#[gpui::test]
fn system_theme_uses_gpui_appearance_and_keeps_editor_state(cx: &mut TestAppContext) {
    use crate::widgets::ActiveTheme;
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        assert_eq!(window.appearance(), gpui::WindowAppearance::Light);
        let editor = this.active_editor().unwrap();
        let content = editor.read(cx).value();
        let selection = editor.read(cx).selected_range();

        let mut settings = this.settings.clone();
        settings.theme = ThemeMode::Dark;
        this.change_settings(settings, window, cx);
        assert_eq!(cx.theme().background, gpui::Hsla::from(gpui::rgb(0x1e2127)));
        assert_eq!(cx.theme().bar, gpui::Hsla::from(gpui::rgb(0x15181d)));

        let mut settings = this.settings.clone();
        settings.theme = ThemeMode::System;
        this.change_settings(settings, window, cx);
        assert_eq!(cx.theme().background, gpui::Hsla::from(gpui::rgb(0xffffff)));
        assert_eq!(cx.theme().bar, gpui::Hsla::from(gpui::rgb(0xf1f4f8)));
        assert_eq!(
            Settings::load(&f.dir.join("test-global-config"))
                .unwrap()
                .theme,
            ThemeMode::System
        );
        assert_eq!(this.active_editor().unwrap(), editor);
        assert_eq!(editor.read(cx).value(), content);
        assert_eq!(editor.read(cx).selected_range(), selection);
    });
    cx.update_window(f.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[gpui::test]
fn font_picker_preserves_a_family_missing_from_gpui_fonts(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let missing_font = "Edot Test Missing Font";
    let picker = f.update(cx, |this, window, cx| {
        assert!(this.preferences.is_none());
        assert!(
            !cx.text_system()
                .all_font_names()
                .iter()
                .any(|name| name == missing_font)
        );
        let mut settings = this.settings.clone();
        settings.font = missing_font.into();
        this.change_settings(settings, window, cx);
        this.show_settings(window, cx);
        let picker = this.preferences.as_ref().unwrap().font.clone();
        let mut settings = this.settings.clone();
        settings.font = "Consolas".into();
        this.change_settings(settings, window, cx);
        picker.update(cx, |state, cx| state.focus(window, cx));
        picker
    });
    f.press(cx, "enter");
    f.input(cx, missing_font);
    f.press(cx, "enter");
    f.update(cx, |this, window, cx| {
        assert_eq!(this.settings.font, missing_font);
        assert_eq!(
            Settings::load(&f.dir.join("test-global-config"))
                .unwrap()
                .font,
            missing_font
        );
        this.show_settings(window, cx);
        this.show_settings(window, cx);
        assert_eq!(this.preferences.as_ref().unwrap().font, picker);
    });
}

#[gpui::test]
fn dropdown_toggle_selection_scroll_and_dismissal(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.settings.font_size = 40;
        this.show_settings(window, cx);
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let trigger = visual.debug_bounds("settings-size").unwrap().center();
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    let options = visual.debug_bounds("select-options").unwrap();
    let selected = visual.debug_bounds("select-highlight").unwrap();
    assert!(
        selected.top() >= options.top() && selected.bottom() <= options.bottom(),
        "selected={selected:?} options={options:?}"
    );
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert_eq!(this.settings.font_size, 40));
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_some());
    visual.simulate_mouse_down(trigger, gpui::MouseButton::Left, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_some());
    visual.simulate_mouse_up(trigger, gpui::MouseButton::Left, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_none());
    f.press(cx, "enter");
    f.press(cx, "escape");
    f.update(cx, |this, _, _| assert!(this.settings_visible));
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_none());
    f.press(cx, "enter");
    f.press(cx, "tab");
    f.update(cx, |this, w, _| {
        assert!(this.preferences.as_ref().unwrap().wrap_focus.is_focused(w))
    });
}

#[gpui::test]
fn settings_close_only_when_focus_leaves_the_panel(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    cx.update_window(f.window, |_, w, _| w.activate_window())
        .unwrap();
    f.update(cx, |this, w, cx| this.show_settings(w, cx));
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    f.press(cx, "tab");
    f.update(cx, |this, _, _| assert!(this.settings_visible));
    let trigger = visual.debug_bounds("settings-font").unwrap().center();
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| assert!(this.settings_visible));
    assert!(visual.debug_bounds("select-popup").is_some());
    f.input(cx, "con");
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert!(this.settings_visible));
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    visual.simulate_click(point(px(440.), px(480.)), gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, w, cx| {
        use gpui::Focusable;
        assert!(
            this.active_editor()
                .unwrap()
                .read(cx)
                .focus_handle(cx)
                .is_focused(w),
            "focus did not reach body: {:?}",
            w.focused(cx)
        );
        assert!(!this.settings_visible);
        this.show_settings(w, cx);
    });
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_none());
    f.update(cx, |this, w, cx| this.tree_focus.focus(w, cx));
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, w, _| {
        assert!(!this.settings_visible);
        assert!(this.tree_focus.is_focused(w));
    });
    f.update(cx, |this, w, cx| this.show_settings(w, cx));
    visual.update(|w, cx| w.render_frame(cx));
    // Tab moves from wrap to the vault button, then leaves the panel.
    f.update(cx, |this, w, cx| {
        this.preferences.as_ref().unwrap().wrap_focus.focus(w, cx)
    });
    f.press(cx, "tab");
    f.update(cx, |this, _, _| assert!(this.settings_visible));
    f.press(cx, "tab");
    f.update(cx, |this, _, _| assert!(!this.settings_visible));
}

#[gpui::test]
fn settings_close_when_window_deactivates(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, _| w.activate_window());
    f.update(cx, |this, w, cx| this.show_settings(w, cx));
    visual.update(|w, cx| w.render_frame(cx));
    visual.deactivate_window();
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, _, _| assert!(!this.settings_visible));
}

#[gpui::test]
fn alt_z_toggles_and_saves_wrap_without_changing_editor_history(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, w, cx| {
        this.focus_editor(w, cx);
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.replace_all("changed", w, cx));
        editor
    });
    f.press(cx, "alt-z");
    f.update(cx, |this, _, cx| {
        assert!(!this.settings.wrap);
        assert!(
            !Settings::load(&f.dir.join("test-global-config"))
                .unwrap()
                .wrap
        );
        assert_eq!(editor.read(cx).value(), "changed");
    });
    f.press(cx, "ctrl-z");
    f.update(cx, |this, w, cx| {
        assert_eq!(editor.read(cx).value(), "🙂 한글12\r\n한글3\rkeep\n");
        this.show_settings(w, cx);
    });
    f.press(cx, "alt-z");
    f.update(cx, |this, _, _| {
        assert!(this.settings.wrap);
        assert!(
            Settings::load(&f.dir.join("test-global-config"))
                .unwrap()
                .wrap
        );
    });
}

#[gpui::test]
fn dropdown_empty_search_keeps_value_and_reopen_resets_filter(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let font = f.update(cx, |this, w, cx| {
        this.show_settings(w, cx);
        this.settings.font.clone()
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let trigger = visual.debug_bounds("settings-font").unwrap().center();
    visual.simulate_click(trigger, gpui::Modifiers::default());
    f.input(cx, "edot nonexistent font qwerty");
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-highlight").is_none());
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert_eq!(this.settings.font, font));
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_some());
    f.press(cx, "escape");
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-highlight").is_some());
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert_eq!(this.settings.font, font));
    visual.simulate_click(trigger, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    let outside = visual.debug_bounds("settings-panel").unwrap().origin + point(px(8.), px(8.));
    visual.simulate_click(outside, gpui::Modifiers::default());
    visual.update(|w, cx| w.render_frame(cx));
    assert!(visual.debug_bounds("select-popup").is_none());
    f.update(cx, |this, _, _| assert!(this.settings_visible));
}
