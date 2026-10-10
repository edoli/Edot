//! Process-wide settings and font state, independent of the active Space.
use super::*;

pub(super) struct AppState {
    pub(super) config_dir: PathBuf,
    pub(super) settings: Settings,
    pub(super) font_refresh: Option<Task<()>>,
    pub(super) font_refresh_started: bool,
    pub(super) font_names: Option<Vec<String>>,
}

impl Global for AppState {}

impl AppState {
    pub(super) fn new(config_dir: PathBuf, settings: Settings) -> Self {
        Self {
            config_dir,
            settings,
            font_refresh: None,
            font_refresh_started: false,
            font_names: None,
        }
    }

    pub(super) fn load(config_dir: PathBuf) -> io::Result<Self> {
        let settings = Settings::load_or_create(&config_dir)?;
        Ok(Self::new(config_dir, settings))
    }
}
