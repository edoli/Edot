use super::*;
use crate::vault;
use std::collections::HashSet;

#[gpui::test]
fn vault_switch_saves_dirty_body_restores_tree_and_shares_global_ini(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let other = f.dir.join("other-vault");
    let mut book = Notebook::load(&other).unwrap();
    let root = book.add(0, "root".into());
    let child = book.add(root, "child".into());
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
        assert!(this.switch_vault(other.clone(), window, cx));
        assert_eq!(this.dir, other);
        assert_eq!(this.settings, settings);
        assert_eq!(this.collapsed, HashSet::from([root]));
        assert_eq!(this.session.active, Some(child));
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "active child body"
        );
        this.set_collapsed(root, false, cx);
        assert!(this.switch_vault(f.dir.clone(), window, cx));
        assert!(this.collapsed.contains(&f.ids[0]));
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "saved before switching"
        );
        assert!(this.switch_vault(other.clone(), window, cx));
        assert!(!this.collapsed.contains(&root));
        assert_eq!(
            Session::load(&other, &this.book).expanded,
            HashSet::from([root])
        );
        let paths = vault::Paths {
            vault: other.clone(),
            config: this.config_dir.clone(),
        };
        assert_eq!(
            fs::read_to_string(paths.config.join("last-vault.txt")).unwrap(),
            fs::canonicalize(&other).unwrap().to_str().unwrap()
        );
    });
    assert!(!other.join("edot.sqlite").exists());
    assert!(!other.join("settings.ini").exists());
}

#[gpui::test]
fn failed_vault_or_session_write_preserves_current_state(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let invalid = f.dir.join("invalid-vault");
    fs::create_dir(&invalid).unwrap();
    fs::write(invalid.join("tree.tsv"), "corrupt tree").unwrap();
    f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        assert!(!this.switch_vault(invalid.clone(), window, cx));
        assert_eq!(this.dir, f.dir);
        assert_eq!(this.active_editor().unwrap(), editor);
        this.error = None;
        fs::create_dir(f.dir.join("session.tmp")).unwrap();
        let session = this.session.clone();
        this.set_collapsed(f.ids[0], true, cx);
        assert!(!this.collapsed.contains(&f.ids[0]));
        assert_eq!(this.session, session);
        assert!(this.error.is_some());
        this.error = None;
        assert!(!this.switch_vault(f.dir.join("new-vault"), window, cx));
        assert_eq!(this.dir, f.dir);
        assert!(!f.dir.join("new-vault").exists());
    });
    assert_eq!(
        fs::read_to_string(invalid.join("tree.tsv")).unwrap(),
        "corrupt tree"
    );
}

#[gpui::test]
fn new_vault_is_initialized_as_text_and_keeps_running_settings(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let new = f.dir.join("new-vault");
    let config = f.dir.join("test-global-config");
    f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| {
            state.replace_all("unsaved edits", window, cx)
        });
        this.changed(f.ids[0], window, cx);
        let selection = editor.read(cx).selected_range();
        let changed = Settings {
            theme: ThemeMode::Dark,
            wrap: false,
            ..Default::default()
        };
        changed.save(&config).unwrap();
        assert_eq!(this.settings, Settings::default());
        assert_eq!(editor.read(cx).value(), "unsaved edits");
        assert_eq!(editor.read(cx).selected_range(), selection);
        assert!(this.open[0].dirty);
        fs::write(config.join("settings.ini"), "version = 999").unwrap();
        assert!(this.switch_vault(new.clone(), window, cx));
        assert_eq!(this.settings, Settings::default());
        assert!(this.book.notes.is_empty());
        assert!(this.open.is_empty());
    });
    assert!(new.join("notes").is_dir());
    assert_eq!(fs::read_to_string(new.join("tree.tsv")).unwrap(), "EDOT1\n");
    assert!(new.join("session.tsv").is_file());
    assert!(!new.join("edot.sqlite").exists());
    assert_eq!(
        fs::read_to_string(config.join("settings.ini")).unwrap(),
        "version = 999"
    );
}
