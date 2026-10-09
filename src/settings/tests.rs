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
        confirm_delete: false,
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
            "version=1\nfont_size=-1\nwrap='bad'\ntheme='bad'\nconfirm_delete='bad'"
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
