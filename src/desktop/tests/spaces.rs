use super::*;
use crate::space;
use crate::widgets::WindowExt;
use std::collections::HashSet;

#[test]
fn space_open_initializes_binary_but_defers_legacy_migration_until_save() {
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

    paths.space = dir.join("legacy");
    let mut book = Notebook::load(&paths.space).unwrap();
    let root = book.add(0, "root".into()).unwrap();
    let child = book.add(root, "child".into()).unwrap();
    book.save(&paths.space).unwrap();
    let legacy = format!("EDOTSESSION1\n0\t300\t{child}\n{root}\n{child}\nexpanded\t{root}\n");
    fs::write(paths.space.join("session.tsv"), &legacy).unwrap();
    let loaded = LoadedSpace::open(&paths).unwrap();
    assert_eq!(loaded.session.tabs, [root, child]);
    assert_eq!(loaded.session.active, Some(child));
    assert_eq!(loaded.session.expanded, HashSet::from([root]));
    assert!(!paths.space.join("session.bin").exists());
    loaded.session.save(&paths.space).unwrap();
    assert_eq!(
        fs::read_to_string(paths.space.join("session.tsv")).unwrap(),
        legacy
    );
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
    let root = book.add(0, "root".into()).unwrap();
    let child = book.add(root, "child".into()).unwrap();
    book.save(&other).unwrap();
    fs::write(book.file(&other, root), "root body").unwrap();
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
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| {
            state.replace_all("saved before switching", window, cx)
        });
        this.changed(f.ids[0], window, cx);
        this.set_collapsed(f.ids[0], true, cx);
        assert!(this.switch_space(other.clone(), window, cx));
        assert_eq!(this.dir, other);
        assert_eq!(this.settings, settings);
        assert_eq!(this.collapsed, HashSet::from([root]));
        assert_eq!(this.session.active, Some(child));
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "active child body"
        );
        this.set_collapsed(root, false, cx);
        assert!(this.switch_space(f.dir.clone(), window, cx));
        assert!(this.collapsed.contains(&f.ids[0]));
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "saved before switching"
        );
        assert!(this.switch_space(other.clone(), window, cx));
        assert!(!this.collapsed.contains(&root));
        assert_eq!(
            Session::load(&other, &this.book).expanded,
            HashSet::from([root])
        );
        let paths = space::Paths {
            space: other.clone(),
            config: this.config_dir.clone(),
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
    f.press(cx, "tab");
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
            space::Registry::initialize(&this.config_dir, &other)
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
        this.set_collapsed(f.ids[0], true, cx);
        assert!(!this.collapsed.contains(&f.ids[0]));
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
