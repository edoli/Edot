use crate::storage::replace_config_file;
use std::io;
use std::path::Path;
mod ini;
use ini::Document;

pub const SETTINGS_FILE: &str = "settings.ini";

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
    pub(crate) fn apply_stored(&mut self, key: &str, value: &str) {
        match (key, value) {
            ("theme", "light") => self.theme = ThemeMode::Light,
            ("theme", "dark") => self.theme = ThemeMode::Dark,
            ("theme", "system") => self.theme = ThemeMode::System,
            ("font", font) if valid_font(font) => self.font = font.into(),
            ("font_size", size) => {
                if let Ok(size @ MIN_FONT_SIZE..=MAX_FONT_SIZE) = size.parse::<u32>() {
                    self.font_size = size;
                }
            }
            ("wrap", "1") => self.wrap = true,
            ("wrap", "0") => self.wrap = false,
            _ => {}
        }
    }

    pub fn load(dir: &Path) -> io::Result<Self> {
        Self::read_document(dir)?.settings()
    }

    pub fn load_or_migrate(dir: &Path, vault: &Path) -> io::Result<Self> {
        if dir.join(SETTINGS_FILE).try_exists()? {
            return Self::load(dir);
        }
        let _lock = Self::lock(dir)?;
        if dir.join(SETTINGS_FILE).try_exists()? {
            return Self::load(dir);
        }
        // The former app-generated TOML uses the same supported key/value syntax.
        // Keep that file intact and publish the converted settings only once.
        let document = Self::read_document(dir)?;
        let settings = if dir.join("settings.toml").try_exists()? {
            document.settings()?
        } else {
            Self::read_legacy(vault)?
        };
        replace_config_file(
            &dir.join(SETTINGS_FILE),
            document.updated(&settings).as_bytes(),
        )?;
        Ok(settings)
    }

    fn read_legacy(dir: &Path) -> io::Result<Self> {
        let text = match std::fs::read_to_string(dir.join("settings.tsv")) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let mut lines = text.lines();
        if lines.next() != Some("EDOTSETTINGS1") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "지원하지 않는 이전 설정 형식입니다.",
            ));
        }
        let mut settings = Self::default();
        for line in lines {
            if let Some((key, value)) = line.split_once('\t') {
                settings.apply_stored(key, value);
            }
        }
        Ok(settings)
    }

    fn lock(dir: &Path) -> io::Result<std::fs::File> {
        std::fs::create_dir_all(dir)?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(".settings.lock"))?;
        lock.lock()?;
        Ok(lock)
    }

    #[cfg(test)]
    fn parse_ini(text: &str) -> io::Result<Self> {
        Document::parse(text)?.settings()
    }

    fn read_document(dir: &Path) -> io::Result<Document> {
        for name in [SETTINGS_FILE, "settings.toml"] {
            match std::fs::read_to_string(dir.join(name)) {
                Ok(text) => return Document::parse(&text),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Document::parse("")
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        if !valid_font(&self.font) || !(MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&self.font_size) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "설정 값이 잘못되었습니다.",
            ));
        }
        let _lock = Self::lock(dir)?;
        let path = dir.join(SETTINGS_FILE);
        let document = Self::read_document(dir)?;
        document.settings()?;
        replace_config_file(&path, document.updated(self).as_bytes())
    }
}

/// Font family constraints shared by the editor and settings validation.
pub fn valid_font(font: &str) -> bool {
    !font.trim().is_empty()
        && font.encode_utf16().count() < 32
        && !font.chars().any(char::is_control)
}

#[cfg(test)]
mod tests;
