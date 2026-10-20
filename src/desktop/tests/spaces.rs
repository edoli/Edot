use super::*;
use crate::space;
use crate::widgets::WindowExt;
use std::collections::HashSet;

#[gpui::test]
fn startup_font_refresh_keeps_the_open_picker_selection(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    fixture.update(cx, |this, window, cx| {
        this.apply_startup_font_names(vec!["Consolas".into(), "Segoe UI".into()], window, cx);
        this.show_settings(window, cx);
        this.preferences
            .as_ref()
            .unwrap()
            .font
            .update(cx, |select, cx| select.focus(window, cx));
    });
    fixture.press(cx, "space");
    fixture.press(cx, "down");
    fixture.update(cx, |this, window, cx| {
        this.apply_startup_font_names(
            vec!["Arial".into(), "Consolas".into(), "Segoe UI".into()],
            window,
            cx,
        );
        assert_eq!(cx.global::<AppState>().settings.font, "Consolas");
    });
    fixture.press(cx, "enter");
    fixture.update(cx, |_, _, cx| {
        assert_eq!(cx.global::<AppState>().settings.font, "Segoe UI")
    });
}

#[gpui::test]
fn missing_font_is_saved_as_the_actual_fallback_after_refresh(cx: &mut TestAppContext) {
    // The headless Windows platform uses NoopTextSystem, which accepts every font.
    // Only take the real text system; all windows below remain test windows.
    let platform = gpui_platform::current_platform(false);
    let mut cx =
        TestAppContext::build_with_text_system(cx.dispatcher.clone(), None, platform.text_system());
    let fixture = Fixture::new(&mut cx);
    let missing = "Edot Missing Font 9c72";
    let expected = fixture.update(&mut cx, |this, window, cx| {
        cx.global_mut::<AppState>().settings.font = missing.into();
        cx.global::<AppState>()
            .settings
            .save(&cx.global::<AppState>().config_dir)
            .unwrap();
        let font = gpui::font(missing);
        assert!(cx.text_system().font_id(&font).is_err());
        let id = cx.text_system().resolve_font(&font);
        let fallback = cx.text_system().get_font_for_id(id).unwrap();
        let expected = gpui::font_name_with_fallbacks(&fallback.family, "Segoe UI").to_owned();
        this.show_settings(window, cx);
        assert_eq!(cx.global::<AppState>().settings.font, missing);
        this.refresh_startup_fonts(window, cx);
        assert!(cx.global::<AppState>().font_refresh.is_some());
        let config_dir = cx.global::<AppState>().config_dir.clone();
        assert!(this.switch_space(fixture.dir.join("font-refresh-space"), window, cx));
        assert!(cx.global::<AppState>().font_refresh.is_some());
        assert_eq!(cx.global::<AppState>().config_dir, config_dir);
        assert_eq!(cx.global::<AppState>().settings.font, missing);
        expected
    });
    cx.run_until_parked();
    fixture.update(&mut cx, |this, window, cx| {
        assert!(cx.global::<AppState>().font_refresh.is_none());
        assert!(cx.global::<AppState>().font_refresh_started);
        this.refresh_startup_fonts(window, cx);
        assert!(cx.global::<AppState>().font_refresh.is_none());
        assert_eq!(cx.global::<AppState>().settings.font, expected);
        assert_eq!(
            Settings::load(&cx.global::<AppState>().config_dir)
                .unwrap()
                .font,
            expected
        );
        let fonts = cx.text_system().all_font_names();
        let saved = fs::read(
            cx.global::<AppState>()
                .config_dir
                .join(crate::settings::SETTINGS_FILE),
        )
        .unwrap();
        this.apply_startup_font_names(fonts.clone(), window, cx);
        assert_eq!(
            fs::read(
                cx.global::<AppState>()
                    .config_dir
                    .join(crate::settings::SETTINGS_FILE)
            )
            .unwrap(),
            saved
        );

        let blocked = fixture.dir.join("blocked-font-config");
        fs::write(&blocked, "preserve").unwrap();
        cx.global_mut::<AppState>().config_dir = blocked.clone();
        cx.global_mut::<AppState>().settings.font = missing.into();
        let previous = cx.global::<AppState>().settings.clone();
        this.apply_startup_font_names(fonts, window, cx);
        assert_eq!(cx.global::<AppState>().settings, previous);
        assert!(this.error.is_some());
        assert_eq!(fs::read_to_string(blocked).unwrap(), "preserve");
    });
}

#[test]
fn space_open_initializes_and_restores_binary_session() {
    let dir = std::env::temp_dir().join(format!(
        "edot-session-space-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut paths = space::Paths {
        space: dir.join("new"),
        config: dir.join("config"),
    };
    let loaded = LoadedSpace::open(&paths).unwrap();
    assert_eq!(
        fs::metadata(paths.space.join("session.bin")).unwrap().len(),
        24
    );
    assert!(!paths.space.join("session.tsv").exists());
    drop(loaded);

    paths.space = dir.join("existing");
    let mut book = Notebook::load(&paths.space).unwrap();
    let root = book.add_folder(0, "root".into()).unwrap();
    let child = book.add(root, "child".into()).unwrap();
    book.save(&paths.space).unwrap();
    Session {
        tabs: vec![root, child],
        active: Some(child),
        expanded: HashSet::from([root]),
        ..Default::default()
    }
    .save(&paths.space)
    .unwrap();
    let loaded = LoadedSpace::open(&paths).unwrap();
    assert_eq!(loaded.session.tabs, [child]);
    assert_eq!(loaded.session.active, Some(child));
    assert_eq!(loaded.session.expanded, HashSet::from([root]));
    let expected = loaded.session.clone();
    drop(loaded);
    let loaded = LoadedSpace::open(&paths).unwrap();
    assert_eq!(loaded.session, expected);
    drop(loaded);
    fs::remove_dir_all(dir).unwrap();
}

#[gpui::test]
fn space_switch_saves_dirty_body_restores_tree_and_shares_global_ini(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let other = f.dir.join("other-space");
    let mut book = Notebook::load(&other).unwrap();
    let root = book.add_folder(0, "root".into()).unwrap();
    let child = book.add(root, "child".into()).unwrap();
    book.save(&other).unwrap();
    fs::write(book.file(&other, child), "active child body").unwrap();
    Session {
        tabs: vec![child],
        active: Some(child),
        ..Default::default()
    }
    .save(&other)
    .unwrap();
    let settings = Settings {
        theme: ThemeMode::Dark,
        font_size: 26,
        wrap: false,
        ..Default::default()
    };
    f.update(cx, |this, window, cx| {
        this.change_settings(settings.clone(), window, cx);
        let fonts = vec!["Consolas".into(), "Segoe UI".into()];
        this.apply_startup_font_names(fonts.clone(), window, cx);
        this.show_settings(window, cx);
        assert_eq!(cx.global::<AppState>().font_names.as_ref(), Some(&fonts));
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| {
            state.replace_all("saved before switching", window, cx)
        });
        this.changed(f.ids[0], window, cx);
        assert!(this.set_read_only(true, window, cx));
        this.set_collapsed(f.folder, true, cx);
        // Switching does not read external settings edits or rebuild the font cache.
        Settings::default()
            .save(&cx.global::<AppState>().config_dir)
            .unwrap();
        assert!(this.switch_space(other.clone(), window, cx));
        assert_eq!(this.dir, other);
        assert_eq!(cx.global::<AppState>().font_names.as_ref(), Some(&fonts));
        this.show_settings(window, cx);
        assert_eq!(cx.global::<AppState>().font_names.as_ref(), Some(&fonts));
        assert!(this.change_settings(cx.global::<AppState>().settings.clone(), window, cx));
        assert_eq!(
            cx.global::<AppState>().settings,
            Settings {
                read_only: true,
                ..settings.clone()
            }
        );
        assert!(this.active_editor().unwrap().read(cx).is_read_only());
        assert_eq!(this.collapsed, HashSet::from([root]));
        assert_eq!(this.session.active, Some(child));
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "active child body"
        );
        this.set_collapsed(root, false, cx);
        assert!(this.switch_space(f.dir.clone(), window, cx));
        assert!(this.collapsed.contains(&f.folder));
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "saved before switching"
        );
        assert!(this.active_editor().unwrap().read(cx).is_read_only());
        assert!(this.switch_space(other.clone(), window, cx));
        assert!(!this.collapsed.contains(&root));
        assert_eq!(
            Session::load(&other, &this.book).expanded,
            HashSet::from([root])
        );
        let paths = space::Paths {
            space: other.clone(),
            config: cx.global::<AppState>().config_dir.clone(),
        };
        assert_eq!(
            fs::read_to_string(paths.config.join("last-space.txt")).unwrap(),
            fs::canonicalize(&other).unwrap().to_str().unwrap()
        );
    });
    assert!(!other.join("edot.sqlite").exists());
    assert!(!other.join("settings.ini").exists());
    f.update(cx, |this, window, cx| {
        this.toggle_space_list(window, cx);
        assert_eq!(this.space_ui.registry.entries.len(), 2);
        this.begin_space_rename(other.clone(), window, cx);
        this.space_ui
            .rename
            .as_ref()
            .unwrap()
            .1
            .update(cx, |input, cx| {
                input.set_value("공유 Space 🙂", window, cx);
            });
    });
    f.press(cx, "enter");
    f.update(cx, |this, _, _| {
        assert!(this.space_ui.rename.is_none());
        assert!(this.space_ui.open);
        assert_eq!(this.current_space_name(), "공유 Space 🙂");
    });
    assert_eq!(space::read_name(&other).unwrap(), "공유 Space 🙂");
    assert!(
        !fs::read_to_string(f.dir.join("test-global-config/spaces.tsv"))
            .unwrap()
            .contains("공유 Space")
    );
    f.press(cx, "escape");
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let footer = visual.debug_bounds("space-footer").unwrap();
    assert_eq!(footer.bottom(), px(760.));
    visual.simulate_mouse_down(footer.center(), gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(footer.center(), gpui::MouseButton::Left, Default::default());
    visual.update(|window, cx| window.render_frame(cx));
    let popup = visual.debug_bounds("space-popup").unwrap();
    assert!(popup.top() >= px(0.));
    assert!(popup.bottom() <= footer.top());
    f.update(cx, |this, _, _| assert!(this.space_ui.open));
    // The list also works from search mode, and keyboard selection changes Spaces.
    f.update(cx, |this, _, _| this.search_mode = true);
    visual.update(|window, cx| window.render_frame(cx));
    assert!(visual.debug_bounds("space-footer").is_some());
    f.press(cx, "up");
    f.press(cx, "enter");
    f.update(cx, |this, window, cx| {
        assert_eq!(this.dir, fs::canonicalize(&f.dir).unwrap());
        assert!(!this.space_ui.open);
        assert!(this.switch_space(other.clone(), window, cx));
        assert_eq!(this.current_space_name(), "공유 Space 🙂");
        this.toggle_space_list(window, cx);
        this.confirm_remove_space(other.clone(), window, cx);
    });
    f.press(cx, "enter");
    f.update(cx, |this, window, cx| {
        assert_eq!(this.space_ui.registry.entries.len(), 1);
        assert_eq!(this.dir, other);
        assert_eq!(this.current_space_name(), "공유 Space 🙂");
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "active child body"
        );
        assert!(!window.has_active_dialog(cx));
        this.close_space_list(window, cx);
        // Removing the active entry does not make it reappear during startup initialization.
        assert_eq!(
            space::Registry::initialize(&cx.global::<AppState>().config_dir, &other)
                .unwrap()
                .entries
                .len(),
            1
        );
    });
    assert_eq!(
        fs::read_to_string(book.file(&other, child)).unwrap(),
        "active child body"
    );
    assert_eq!(space::read_name(&other).unwrap(), "공유 Space 🙂");
    // Startup loads the same global mode before it creates the active editor.
    let paths = space::Paths {
        space: f.dir.clone(),
        config: f.dir.join("test-global-config"),
    };
    let loaded = LoadedSpace::open(&paths).unwrap();
    let state = AppState::load(paths.config).unwrap();
    assert!(state.settings.read_only);
    cx.update(|cx| cx.set_global(state));
    let restarted = cx.add_window(|window, cx| Edot::new(paths.space, Ok(loaded), window, cx));
    restarted
        .update(cx, |this, _, cx| {
            assert!(cx.global::<AppState>().settings.read_only);
            assert!(this.active_editor().unwrap().read(cx).is_read_only());
        })
        .unwrap();
}

#[gpui::test]
fn failed_space_or_session_write_preserves_current_state(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let invalid = f.dir.join("invalid-space");
    fs::create_dir(&invalid).unwrap();
    fs::write(invalid.join("tree.tsv"), "corrupt tree").unwrap();
    f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        assert!(!this.switch_space(invalid.clone(), window, cx));
        assert_eq!(this.dir, f.dir);
        assert_eq!(this.active_editor().unwrap(), editor);
        this.error = None;
        let missing = f.dir.join("missing-registered-space");
        this.select_registered_space(missing.clone(), window, cx);
        assert!(this.error.is_some());
        assert!(!missing.exists());
        assert_eq!(this.active_editor().unwrap(), editor);
        this.error = None;
        this.toggle_space_list(window, cx);
        let old_name = this.current_space_name();
        this.begin_space_rename(f.dir.clone(), window, cx);
        this.space_ui
            .rename
            .as_ref()
            .unwrap()
            .1
            .update(cx, |input, cx| input.set_value("failed name", window, cx));
        fs::create_dir(f.dir.join("space.ini")).unwrap();
        this.finish_space_rename(window, cx);
        assert_eq!(this.current_space_name(), old_name);
        assert!(this.space_ui.rename.is_some());
        assert!(this.error.is_some());
        this.error = None;
        this.close_space_list(window, cx);
        fs::create_dir(f.dir.join("session.tmp")).unwrap();
        let session = this.session.clone();
        this.set_collapsed(f.folder, true, cx);
        assert!(!this.collapsed.contains(&f.folder));
        assert_eq!(this.session, session);
        assert!(this.error.is_some());
        this.error = None;
        assert!(!this.switch_space(f.dir.join("new-space"), window, cx));
        assert_eq!(this.dir, f.dir);
        assert!(!f.dir.join("new-space").exists());
    });
    assert_eq!(
        fs::read_to_string(invalid.join("tree.tsv")).unwrap(),
        "corrupt tree"
    );
}
