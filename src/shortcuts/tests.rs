use super::*;

fn key(value: &str) -> Keystroke {
    Keystroke::parse(value).unwrap()
}

#[test]
fn remapping_replaces_aliases_detects_conflicts_and_restores_defaults() {
    let mut bindings = Bindings::default();
    assert!(
        bindings
            .set(Command::NewNote, Some(&key("ctrl-s")))
            .is_err()
    );
    assert!(bindings.0.is_empty());
    bindings
        .set(Command::CloseTab, Some(&key("ctrl-k")))
        .unwrap();
    let active = ActiveShortcuts::new(bindings.clone());
    for old in ["ctrl-w", "ctrl-f4"] {
        assert_eq!(active.command(&key(old), false), None);
    }
    assert_eq!(
        active.command(&key("ctrl-k"), false),
        Some(Command::CloseTab)
    );
    bindings
        .set(Command::NewNote, Some(&key("ctrl-w")))
        .unwrap();
    bindings.0.remove(&Command::CloseTab);
    assert!(bindings.validate().is_err());
    bindings.0.clear();
    assert!(bindings.validate().is_ok());
}

#[test]
fn matching_normalizes_shifted_symbols_and_requires_exact_modifiers() {
    let active = ActiveShortcuts::default();
    for stroke in ["ctrl-~", "ctrl-shift-~", "ctrl-shift-`"] {
        assert_eq!(active.command(&key(stroke), false), Some(Command::Strike));
    }
    assert_eq!(
        active.command(&key("ctrl-shift-n"), false),
        Some(Command::NewFolder)
    );
    for stroke in ["ctrl-alt-n", "ctrl-shift-s", "ctrl-shift-f3", "ctrl-f11"] {
        assert_eq!(active.command(&key(stroke), false), None);
    }
    assert_eq!(active.command(&key("cmd-e"), true), Some(Command::Code));
    assert_eq!(
        active.command(&key("ctrl-tab"), true),
        Some(Command::NextTab)
    );
    assert_eq!(active.command(&key("cmd-tab"), true), None);
}

#[test]
fn input_navigation_and_system_keys_cannot_be_overridden() {
    let mut bindings = Bindings::default();
    for stroke in [
        "a",
        "shift-a",
        "enter",
        "escape",
        "ctrl-left",
        "ctrl-enter",
        "ctrl-space",
        "shift-f10",
        "win-k",
        "ctrl-alt-k",
        "alt-f4",
    ] {
        assert!(
            bindings.set(Command::Save, Some(&key(stroke))).is_err(),
            "{stroke}"
        );
    }
    bindings.set(Command::Save, Some(&key("f12"))).unwrap();
    bindings.set(Command::Save, None).unwrap();
    assert_eq!(
        ActiveShortcuts::new(bindings).command(&key("ctrl-s"), false),
        None
    );
}

#[test]
fn lists_add_edit_remove_independently_and_reject_duplicates_atomically() {
    let mut bindings = Bindings::default();
    bindings
        .edit(Command::NewNote, None, &key("ctrl-k"))
        .unwrap();
    assert_eq!(bindings.0[&Command::NewNote], "ctrl-n|ctrl-k");
    let active = ActiveShortcuts::new(bindings.clone());
    for stroke in ["ctrl-n", "ctrl-k"] {
        assert_eq!(active.command(&key(stroke), false), Some(Command::NewNote));
    }
    let before = bindings.clone();
    for stroke in ["ctrl-n", "ctrl-k", "ctrl-s"] {
        assert!(bindings.edit(Command::NewNote, None, &key(stroke)).is_err());
        assert_eq!(bindings, before);
    }
    bindings
        .edit(Command::NewNote, Some(1), &key("ctrl-j"))
        .unwrap();
    assert_eq!(bindings.0[&Command::NewNote], "ctrl-n|ctrl-j");
    bindings.remove(Command::NewNote, 0).unwrap();
    assert_eq!(bindings.0[&Command::NewNote], "ctrl-j");
    bindings.remove(Command::NewNote, 0).unwrap();
    assert_eq!(bindings.0[&Command::NewNote], "");
    assert!(bindings.remove(Command::NewNote, 0).is_err());
    bindings.remove(Command::CloseTab, 0).unwrap();
    let active = ActiveShortcuts::new(bindings);
    assert_eq!(active.command(&key("ctrl-w"), false), None);
    assert_eq!(
        active.command(&key("ctrl-f4"), false),
        Some(Command::CloseTab)
    );
}
