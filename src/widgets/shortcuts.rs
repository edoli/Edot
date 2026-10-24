//! Shared modifier policy and shortcut labels; GPUI calls Cmd/Win `platform`.
use gpui::{Modifiers, MouseButton, MouseDownEvent, SharedString};

pub fn context_click(event: &MouseDownEvent) -> bool {
    cfg!(target_os = "macos")
        && event.button == MouseButton::Left
        && event.modifiers.control
        && !event.modifiers.platform
        && !event.modifiers.alt
}

pub fn primary(modifiers: Modifiers) -> bool {
    primary_on(modifiers, cfg!(target_os = "macos"))
}

pub(crate) fn primary_on(modifiers: Modifiers, macos: bool) -> bool {
    !modifiers.alt
        && if macos {
            modifiers.platform && !modifiers.control
        } else {
            modifiers.control && !modifiers.platform
        }
}

pub fn label(value: impl Into<SharedString>) -> SharedString {
    let value = value.into();
    if cfg!(target_os = "macos") {
        mac_label(&value).into()
    } else {
        value
    }
}

fn mac_label(value: &str) -> String {
    if value == "F11" {
        return "F11 / Ctrl+Cmd+F".into();
    }
    // Translate the original label in one pass: generated Cmd/Option text is
    // never translated again, and labels allocate only their final String.
    const REPLACEMENTS: &[(&str, &str)] = &[
        ("Ctrl+Y / Ctrl+Shift+Z", "Cmd+Shift+Z"),
        ("Ctrl+W / Ctrl+F4", "Cmd+W"),
        ("Ctrl+Shift+Tab", "Ctrl+Shift+Tab"),
        ("Ctrl+Tab", "Ctrl+Tab"),
        ("Ctrl+PageDown", "Ctrl+PageDown"),
        ("Ctrl+PageUp", "Ctrl+PageUp"),
        ("Ctrl+Home", "Cmd+↑"),
        ("Ctrl+End", "Cmd+↓"),
        ("Ctrl+Shift+`", "Cmd+Shift+X"),
        ("Ctrl+`", "Cmd+E"),
        ("Ctrl+Shift+H", "Cmd+Option+Shift+F"),
        ("Ctrl+H", "Cmd+Option+F"),
        ("Ctrl+Y", "Cmd+Shift+Z"),
        ("Ctrl+←", "Option+←"),
        ("Ctrl+→", "Option+→"),
        ("Ctrl+Backspace", "Option+Backspace"),
        ("Ctrl+Delete", "Option+Delete"),
        ("Alt+Z", "Cmd+Option+Z"),
        ("Ctrl+", "Cmd+"),
        ("Alt+", "Option+"),
    ];
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while !rest.is_empty() {
        if let Some((from, to)) = REPLACEMENTS.iter().find(|(from, _)| rest.starts_with(from)) {
            output.push_str(to);
            rest = &rest[from.len()..];
        } else {
            let ch = rest.chars().next().unwrap();
            output.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_excludes_altgr_and_the_other_platform_modifier() {
        let ctrl = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        let cmd = Modifiers {
            platform: true,
            ..Modifiers::default()
        };
        assert!(primary_on(ctrl, false));
        assert!(!primary_on(ctrl, true));
        assert!(primary_on(cmd, true));
        assert!(!primary_on(cmd, false));
        for macos in [false, true] {
            assert!(!primary_on(
                Modifiers {
                    alt: true,
                    control: true,
                    ..Modifiers::default()
                },
                macos
            ));
            assert!(!primary_on(
                Modifiers {
                    control: true,
                    platform: true,
                    ..Modifiers::default()
                },
                macos
            ));
        }
        assert_eq!(mac_label("Ctrl+W / Ctrl+F4"), "Cmd+W");
        assert_eq!(
            mac_label("Ctrl+Tab / Ctrl+PageDown"),
            "Ctrl+Tab / Ctrl+PageDown"
        );
        assert_eq!(
            mac_label("Ctrl+H / Ctrl+Shift+H"),
            "Cmd+Option+F / Cmd+Option+Shift+F"
        );
        assert_eq!(
            mac_label("Ctrl+Backspace / Ctrl+Home"),
            "Option+Backspace / Cmd+↑"
        );
    }
}
