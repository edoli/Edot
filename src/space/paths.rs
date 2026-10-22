#[cfg(any(not(target_os = "macos"), test))]
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
struct DefaultPaths {
    config: PathBuf,
    space: PathBuf,
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("EDOT_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_paths().config)
}

pub(super) fn default_space() -> PathBuf {
    default_paths().space
}

#[cfg(target_os = "windows")]
fn default_paths() -> DefaultPaths {
    windows_defaults(|key| std::env::var_os(key))
}

#[cfg(target_os = "macos")]
fn default_paths() -> DefaultPaths {
    macos_defaults(std::env::home_dir())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn default_paths() -> DefaultPaths {
    linux_defaults(|key| std::env::var_os(key), std::env::home_dir())
}

#[cfg(any(target_os = "windows", test))]
fn windows_defaults(environment: impl Fn(&str) -> Option<OsString>) -> DefaultPaths {
    let local = environment("LOCALAPPDATA").map(PathBuf::from);
    DefaultPaths {
        config: environment("APPDATA")
            .map(|path| PathBuf::from(path).join("Edot"))
            .or_else(|| local.as_ref().map(|path| path.join("EdotConfig")))
            .unwrap_or_else(|| PathBuf::from("EdotConfig")),
        space: local
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Edot/spaces/Default"),
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_defaults(home: Option<PathBuf>) -> DefaultPaths {
    match home.filter(|path| path.is_absolute()) {
        Some(home) => {
            let config = home.join("Library/Application Support/Edot");
            DefaultPaths {
                space: config.join("spaces/Default"),
                config,
            }
        }
        None => DefaultPaths {
            config: PathBuf::from("EdotConfig"),
            space: PathBuf::from("Edot/spaces/Default"),
        },
    }
}

#[cfg(any(not(any(target_os = "windows", target_os = "macos")), test))]
fn linux_defaults(
    environment: impl Fn(&str) -> Option<OsString>,
    home: Option<PathBuf>,
) -> DefaultPaths {
    let home = home.filter(|path| path.is_absolute());
    let xdg = |key| {
        environment(key)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    DefaultPaths {
        config: xdg("XDG_CONFIG_HOME")
            .or_else(|| home.as_ref().map(|path| path.join(".config")))
            .map(|path| path.join("edot"))
            .unwrap_or_else(|| PathBuf::from("EdotConfig")),
        space: xdg("XDG_DATA_HOME")
            .or_else(|| home.as_ref().map(|path| path.join(".local/share")))
            .map(|path| path.join("edot/spaces/Default"))
            .unwrap_or_else(|| PathBuf::from("Edot/spaces/Default")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment<'a>(values: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |key| {
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| OsString::from(*value))
        }
    }

    #[test]
    fn windows_preserves_roaming_local_and_working_directory_fallbacks() {
        let values = [("APPDATA", "roaming"), ("LOCALAPPDATA", "local")];
        let paths = windows_defaults(environment(&values));
        assert_eq!(paths.config, PathBuf::from("roaming/Edot"));
        assert_eq!(paths.space, PathBuf::from("local/Edot/spaces/Default"));
        let paths = windows_defaults(environment(&values[1..]));
        assert_eq!(paths.config, PathBuf::from("local/EdotConfig"));
        let paths = windows_defaults(environment(&[]));
        assert_eq!(paths.config, PathBuf::from("EdotConfig"));
        assert_eq!(paths.space, PathBuf::from("./Edot/spaces/Default"));
    }

    #[test]
    fn macos_keeps_config_and_default_space_under_application_support() {
        let home = std::env::temp_dir().join("edot-home 한글");
        let paths = macos_defaults(Some(home.clone()));
        assert_eq!(paths.config, home.join("Library/Application Support/Edot"));
        assert_eq!(paths.space, paths.config.join("spaces/Default"));
    }

    #[test]
    fn linux_respects_absolute_xdg_roots_without_requiring_a_home() {
        let base = std::env::temp_dir().join("edot-xdg 한글");
        let config = base.join("config").into_os_string();
        let data = base.join("data").into_os_string();
        let paths = linux_defaults(
            |key| match key {
                "XDG_CONFIG_HOME" => Some(config.clone()),
                "XDG_DATA_HOME" => Some(data.clone()),
                _ => None,
            },
            None,
        );
        assert_eq!(paths.config, base.join("config/edot"));
        assert_eq!(paths.space, base.join("data/edot/spaces/Default"));
    }

    #[test]
    fn linux_ignores_empty_and_relative_xdg_roots_independently() {
        let home = std::env::temp_dir().join("edot-home 한글");
        for value in [None, Some(""), Some("relative/path")] {
            let paths = linux_defaults(|_| value.map(OsString::from), Some(home.clone()));
            assert_eq!(paths.config, home.join(".config/edot"));
            assert_eq!(paths.space, home.join(".local/share/edot/spaces/Default"));
        }
        let custom = home.join("custom-config");
        let paths = linux_defaults(
            |key| match key {
                "XDG_CONFIG_HOME" => Some(custom.clone().into_os_string()),
                "XDG_DATA_HOME" => Some("relative".into()),
                _ => None,
            },
            Some(home.clone()),
        );
        assert_eq!(paths.config, custom.join("edot"));
        assert_eq!(paths.space, home.join(".local/share/edot/spaces/Default"));
    }

    #[test]
    fn unavailable_or_relative_home_keeps_working_directory_fallbacks() {
        for home in [None, Some(PathBuf::from("relative/home"))] {
            let expected = DefaultPaths {
                config: PathBuf::from("EdotConfig"),
                space: PathBuf::from("Edot/spaces/Default"),
            };
            assert_eq!(macos_defaults(home.clone()), expected);
            assert_eq!(linux_defaults(environment(&[]), home), expected);
        }
    }
}
