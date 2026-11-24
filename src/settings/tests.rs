use super::*;

fn fixture() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "edot-config-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn ini_preserves_comments_unknown_fields_and_escaped_font_names() {
    let dir = fixture();
    let path = dir.join(SETTINGS_FILE);
    std::fs::write(&path, "# My preferences\nversion = 1\ntheme = 'light' # appearance\nfont_size = 22\nfuture = 'keep'\n[extension]\nanswer = 42\n").unwrap();
    let settings = Settings {
        theme: ThemeMode::Dark,
        font: "Font \\\"한글".into(),
        font_size: 25,
        wrap: false,
        editor_mode: EditorMode::Visual,
        confirm_delete: false,
        shortcuts: Default::default(),
    };
    settings.save(&dir).unwrap();
    assert_eq!(Settings::load(&dir).unwrap(), settings);
    let text = std::fs::read_to_string(&path).unwrap();
    for fragment in [
        "# My preferences",
        "# appearance",
        "future = 'keep'",
        "[extension]",
        "answer = 42",
    ] {
        assert!(text.contains(fragment), "{text}");
    }
    assert!(
        Settings::parse_ini(
            "version=1\nfont_size=-1\nwrap='bad'\nread_only='bad'\ntheme='bad'\nconfirm_delete='bad'"
        )
        .unwrap()
            == Settings::default()
    );
    for text in [
        "version=2",
        "version='1'",
        "font = \"unterminated",
        "theme='light'\ntheme='dark'",
    ] {
        std::fs::write(&path, text).unwrap();
        assert!(Settings::load(&dir).is_err());
        assert!(settings.save(&dir).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn editor_mode_migrates_legacy_read_only_and_keeps_explicit_preference() {
    let dir = fixture();
    let path = dir.join(SETTINGS_FILE);
    std::fs::write(
        &path,
        "version=1\nread_only = true # previous view\nshortcut_read_only = 'ctrl-k'\n",
    )
    .unwrap();
    let mut settings = Settings::load(&dir).unwrap();
    assert_eq!(settings.editor_mode, EditorMode::Visual);
    assert_eq!(
        settings.shortcuts.0[&crate::shortcuts::Command::ToggleEditorMode],
        "ctrl-k"
    );
    settings.editor_mode = EditorMode::Source;
    settings.save(&dir).unwrap();
    assert_eq!(Settings::load(&dir).unwrap(), settings);
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("# previous view")
    );
}

#[test]
fn shortcut_overrides_round_trip_disable_restore_and_preserve_unknown_fields() {
    use crate::shortcuts::Command;
    let dir = fixture();
    let path = dir.join(SETTINGS_FILE);
    std::fs::write(
        &path,
        "# shortcuts\nshortcut_future = 'keep'\n[extension]\nvalue=1\n",
    )
    .unwrap();
    let mut settings = Settings::load(&dir).unwrap();
    settings
        .shortcuts
        .set(
            Command::NewNote,
            Some(&gpui::Keystroke::parse("ctrl-shift-\\").unwrap()),
        )
        .unwrap();
    settings.shortcuts.set(Command::Copy, None).unwrap();
    settings
        .shortcuts
        .edit(
            Command::NewNote,
            None,
            &gpui::Keystroke::parse("ctrl-k").unwrap(),
        )
        .unwrap();
    settings.save(&dir).unwrap();
    assert_eq!(Settings::load(&dir).unwrap(), settings);
    settings.shortcuts.0.clear();
    settings.save(&dir).unwrap();
    assert_eq!(Settings::load(&dir).unwrap(), Settings::default());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("shortcut_new_note = default"));
    assert!(text.contains("shortcut_future = 'keep'"));
    assert!(text.contains("[extension]\nvalue=1"));
    for invalid in [
        "shortcut_new_note = 'ctrl-s'",
        "shortcut_save = 'escape'",
        "shortcut_save = 'ctrl-missing-key'",
        "shortcut_save = 'ctrl-k|'",
        "shortcut_save = 'ctrl-k|ctrl-k'",
        "shortcut_save = 'ctrl-k|ctrl-shift-~|ctrl-shift-`'",
    ] {
        assert!(Settings::parse_ini(invalid).is_err(), "{invalid}");
    }
}
