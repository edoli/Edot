use crate::storage::replace_file;
use std::io;
use std::path::Path;

pub const DEFAULT_FONT: &str = "Consolas";
pub const DEFAULT_FONT_SIZE: u32 = 18;
pub const MIN_FONT_SIZE: u32 = 8;
pub const MAX_FONT_SIZE: u32 = 72;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemeMode,
    pub font: String,
    pub font_size: u32,
    pub wrap: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            font: DEFAULT_FONT.into(),
            font_size: DEFAULT_FONT_SIZE,
            wrap: true,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> io::Result<Self> {
        let mut lines = text.lines();
        if lines.next() != Some("EDOTSETTINGS1") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "지원하지 않는 설정 형식입니다.",
            ));
        }
        let mut settings = Self::default();
        for line in lines {
            let Some((key, value)) = line.split_once('\t') else {
                continue;
            };
            match (key, value) {
                ("theme", "light") => settings.theme = ThemeMode::Light,
                ("theme", "dark") => settings.theme = ThemeMode::Dark,
                ("theme", "system") => settings.theme = ThemeMode::System,
                ("font", font) if valid_font(font) => settings.font = font.into(),
                ("font_size", size) => {
                    if let Ok(size @ MIN_FONT_SIZE..=MAX_FONT_SIZE) = size.parse::<u32>() {
                        settings.font_size = size;
                    }
                }
                ("wrap", "1") => settings.wrap = true,
                ("wrap", "0") => settings.wrap = false,
                _ => {}
            }
        }
        Ok(settings)
    }

    pub fn load(dir: &Path) -> io::Result<Self> {
        match std::fs::read_to_string(dir.join("settings.tsv")) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        let mode = match self.theme {
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
            ThemeMode::System => "system",
        };
        replace_file(
            &dir.join("settings.tsv"),
            format!(
                "EDOTSETTINGS1\ntheme\t{mode}\nfont\t{}\nfont_size\t{}\nwrap\t{}\n",
                self.font, self.font_size, self.wrap as u8
            )
            .as_bytes(),
        )
    }
}

/// Whether a font family name can round-trip through the existing settings format.
pub fn valid_font(font: &str) -> bool {
    !font.trim().is_empty()
        && font.encode_utf16().count() < 32
        && !font.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_atomic_replace() {
        let dir = std::env::temp_dir().join(format!("edot-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
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
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_values_fall_back_but_unknown_version_is_rejected() {
        let text = format!(
            "EDOTSETTINGS1\ntheme\tunknown\nwrap\tbad\nfont\t{}\nfuture\t1\n",
            "a".repeat(32)
        );
        assert_eq!(Settings::parse(&text).unwrap(), Settings::default());
        assert!(Settings::parse("EDOTSETTINGS2\n").is_err());
        assert!(Settings::parse("garbage").is_err());
        assert!(!valid_font("bad\tfont"));
        assert!(!valid_font("\0"));
        assert!(!valid_font(" "));
        assert_eq!(
            Settings::parse("EDOTSETTINGS1\nfont\tArial\n")
                .unwrap()
                .font_size,
            18
        );
        for size in ["7", "73", "0", "-1", "bad", "4294967296"] {
            assert_eq!(
                Settings::parse(&format!("EDOTSETTINGS1\nfont_size\t{size}\n"))
                    .unwrap()
                    .font_size,
                18
            );
        }
    }
}
