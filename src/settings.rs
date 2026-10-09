use crate::storage::replace_config_file;
use std::io;
use std::path::Path;
pub(crate) mod ini;
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
    pub read_only: bool,
    pub confirm_delete: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            font: DEFAULT_FONT.into(),
            font_size: DEFAULT_FONT_SIZE,
            wrap: true,
            read_only: false,
            confirm_delete: true,
        }
    }
}

impl Settings {
    pub(crate) fn apply_stored(&mut self, key: &str, value: &str) {
        match (key, value) {
            ("theme", "light") => self.theme = ThemeMode::Light,
            ("theme", "dark") => self.theme = ThemeMode::Dark,
            ("theme", "system") => self.theme = ThemeMode::System,
            ("confirm_delete", "true") => self.confirm_delete = true,
            ("confirm_delete", "false") => self.confirm_delete = false,
            ("read_only", "true") => self.read_only = true,
            ("read_only", "false") => self.read_only = false,
            ("font", font) if valid_font(font) => self.font = font.into(),
            ("font_size", size) => {
                if let Ok(size @ MIN_FONT_SIZE..=MAX_FONT_SIZE) = size.parse::<u32>() {
                    self.font_size = size;
                }
            }
            _ => {}
        }
    }

    pub fn load(dir: &Path) -> io::Result<Self> {
        Self::read_document(dir)?.settings()
    }

    pub fn load_or_create(dir: &Path) -> io::Result<Self> {
        if dir.join(SETTINGS_FILE).try_exists()? {
            return Self::load(dir);
        }
        let _lock = Self::lock(dir)?;
        if dir.join(SETTINGS_FILE).try_exists()? {
            return Self::load(dir);
        }
        let document = Self::read_document(dir)?;
        let settings = document.settings()?;
        replace_config_file(
            &dir.join(SETTINGS_FILE),
            document.updated(&settings).as_bytes(),
        )?;
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
        match std::fs::read_to_string(dir.join(SETTINGS_FILE)) {
            Ok(text) => Document::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Document::parse(""),
            Err(error) => Err(error),
        }
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
