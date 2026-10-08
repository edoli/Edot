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
fn settings_round_trip_and_atomic_replace() {
    let dir = fixture();
    assert_eq!(Settings::load(&dir).unwrap(), Settings::default());
    let mut settings = Settings {
        theme: ThemeMode::Dark,
        font: "맑은 고딕".into(),
        font_size: 24,
        wrap: false,
    };
    settings.save(&dir).unwrap();
    assert_eq!(Settings::load(&dir).unwrap(), settings);
    settings.theme = ThemeMode::Light;
    settings.save(&dir).unwrap();
    assert_eq!(Settings::load(&dir).unwrap(), settings);
    assert!(!dir.join("edot.sqlite").exists());
}

#[test]
fn invalid_values_fall_back_but_unknown_version_is_rejected() {
    let text = format!(
        "version=1\ntheme='unknown'\nwrap='bad'\nfont='{}'\nfuture=1\n",
        "a".repeat(32)
    );
    assert_eq!(Settings::parse_ini(&text).unwrap(), Settings::default());
    assert!(Settings::parse_ini("version=2").is_err());
    assert!(Settings::parse_ini("garbage").is_err());
    assert!(!valid_font("bad\tfont"));
    assert!(!valid_font("\0"));
    assert!(!valid_font(" "));
    assert_eq!(Settings::parse_ini("font='Arial'").unwrap().font_size, 18);
    for size in ["7", "73", "0", "-1", "'bad'", "4294967296"] {
        assert_eq!(
            Settings::parse_ini(&format!("font_size={size}"))
                .unwrap()
                .font_size,
            18
        );
    }
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
        Settings::parse_ini("version=1\nfont_size=-1\nwrap='bad'\ntheme='bad'").unwrap()
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
fn parallel_saves_publish_complete_documents_and_preserve_unknown_keys() {
    let dir = fixture();
    std::fs::write(dir.join(SETTINGS_FILE), "version=1\nunknown='preserved'\n").unwrap();
    std::thread::scope(|scope| {
        for size in [19, 20, 21, 22] {
            let dir = &dir;
            scope.spawn(move || {
                for _ in 0..8 {
                    Settings {
                        font_size: size,
                        ..Default::default()
                    }
                    .save(dir)
                    .unwrap();
                    let loaded = Settings::load(dir).unwrap();
                    assert!((19..=22).contains(&loaded.font_size));
                }
            });
        }
    });
    assert!(
        std::fs::read_to_string(dir.join(SETTINGS_FILE))
            .unwrap()
            .contains("unknown='preserved'")
    );
    assert!(std::fs::read_dir(&dir).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
}

#[test]
fn imports_previous_global_toml_once_without_changing_source() {
    let dir = fixture();
    let vault = dir.join("vault");
    let legacy = "# Original preferences\nversion = 1\ntheme = \"dark\"\nfont = \"맑은 고딕\"\nfont_size = 14\nwrap = false\n[extension]\nanswer = 42\n";
    std::fs::write(dir.join("settings.toml"), legacy).unwrap();
    let imported = Settings::load_or_migrate(&dir, &vault).unwrap();
    assert_eq!(imported.theme, ThemeMode::Dark);
    assert_eq!(imported.font, "맑은 고딕");
    assert_eq!(imported.font_size, 14);
    assert!(!imported.wrap);
    assert_eq!(
        std::fs::read_to_string(dir.join("settings.toml")).unwrap(),
        legacy
    );
    let ini = std::fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
    assert!(ini.contains("# Original preferences"));
    assert!(ini.contains("[extension]\nanswer = 42"));
    // A stale or corrupt old file must not override the current global settings.
    std::fs::write(dir.join("settings.toml"), "version = 999").unwrap();
    assert_eq!(Settings::load_or_migrate(&dir, &vault).unwrap(), imported);
    assert_eq!(Settings::load(&dir).unwrap(), imported);
}

#[test]
fn invalid_legacy_config_is_preserved_and_not_replaced_with_defaults() {
    let dir = fixture();
    let vault = dir.join("vault");
    for source in ["version = 999", "font = \"unterminated", "font = [\n"] {
        std::fs::write(dir.join("settings.toml"), source).unwrap();
        assert!(Settings::load_or_migrate(&dir, &vault).is_err());
        assert!(Settings::default().save(&dir).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.toml")).unwrap(),
            source
        );
        assert!(!dir.join(SETTINGS_FILE).exists());
    }
}

#[test]
fn ini_handles_plain_values_unicode_escapes_bom_and_empty_crlf_values() {
    let dir = fixture();
    let path = dir.join(SETTINGS_FILE);
    std::fs::write(&path, "\u{feff}; preferences\r\nfont =   \r\nfont_size = 21 ; size\r\nwrap = false\r\n[extension]\r\nfont_size = 70\r\n").unwrap();
    let loaded = Settings::load(&dir).unwrap();
    assert_eq!(loaded.font, DEFAULT_FONT);
    assert_eq!(loaded.font_size, 21);
    assert!(!loaded.wrap);
    loaded.save(&dir).unwrap();
    assert_eq!(Settings::load(&dir).unwrap(), loaded);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("font =   \"Consolas\"\r\nfont_size = 21 ; size"));
    assert!(text.contains("[extension]\r\nfont_size = 70\r\n"));
    assert_eq!(
        Settings::parse_ini("font = 맑은 고딕").unwrap().font,
        "맑은 고딕"
    );
    assert_eq!(
        Settings::parse_ini("font = \"\\u0041\\U0001F642\"")
            .unwrap()
            .font,
        "A🙂"
    );
    assert_eq!(
        Settings::parse_ini("font = \"Font #;=\\\\\\\"\"")
            .unwrap()
            .font,
        "Font #;=\\\""
    );
    for text in [
        "font = \"\\q\"",
        "font = \"\\uD800\"",
        "[",
        "[]",
        "theme=dark\ntheme=light",
        "not a field",
    ] {
        assert!(Settings::parse_ini(text).is_err(), "{text}");
    }
}

#[test]
fn imports_vault_tsv_once_and_shares_global_ini_without_modifying_source() {
    let dir = fixture();
    let config = dir.join("config");
    let first = dir.join("first");
    let second = dir.join("second");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    let source = "EDOTSETTINGS1\ntheme\tdark\nfont\t맑은 고딕\nfont_size\t24\nwrap\t0\n";
    std::fs::write(first.join("settings.tsv"), source).unwrap();
    std::fs::write(second.join("settings.tsv"), "invalid old settings").unwrap();
    let imported = Settings::load_or_migrate(&config, &first).unwrap();
    assert_eq!(imported.theme, ThemeMode::Dark);
    assert_eq!(imported.font, "맑은 고딕");
    assert_eq!(imported.font_size, 24);
    assert!(!imported.wrap);
    assert_eq!(
        std::fs::read_to_string(first.join("settings.tsv")).unwrap(),
        source
    );
    assert_eq!(
        Settings::load_or_migrate(&config, &second).unwrap(),
        imported
    );
    let changed = Settings {
        font_size: 29,
        ..imported
    };
    changed.save(&config).unwrap();
    assert_eq!(Settings::load_or_migrate(&config, &first).unwrap(), changed);
    assert!(!first.join(SETTINGS_FILE).exists());
    assert!(!second.join(SETTINGS_FILE).exists());
}

#[test]
fn malformed_old_vault_settings_block_initialization_and_are_preserved() {
    let dir = fixture();
    let config = dir.join("config");
    std::fs::write(dir.join("settings.tsv"), "EDOTSETTINGS2\n").unwrap();
    assert!(Settings::load_or_migrate(&config, &dir).is_err());
    assert!(!config.join(SETTINGS_FILE).exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("settings.tsv")).unwrap(),
        "EDOTSETTINGS2\n"
    );
}
