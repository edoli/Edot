//! Process-wide settings and font state, independent of the active Space.
use super::*;

pub(super) struct AppState {
    pub(super) config_dir: PathBuf,
    pub(super) settings: Settings,
    pub(super) font_refresh: Option<Task<()>>,
    pub(super) font_refresh_started: bool,
    pub(super) font_names: Option<Vec<String>>,
    pub(super) update_state: crate::update::State,
    pub(super) update_checked: Option<std::time::Instant>,
    pub(super) update_task: Option<Task<()>>,
    pub(super) update_worker: Option<DedicatedExecutor>,
    pub(super) update_message: Option<String>,
}

impl Global for AppState {}

impl AppState {
    pub(super) fn new(config_dir: PathBuf, settings: Settings) -> Self {
        let update_message = crate::update::read_result(&config_dir);
        Self {
            config_dir,
            settings,
            font_refresh: None,
            font_refresh_started: false,
            font_names: None,
            update_state: Default::default(),
            update_checked: None,
            update_task: None,
            update_worker: None,
            update_message,
        }
    }

    pub(super) fn load(config_dir: PathBuf) -> io::Result<Self> {
        let settings = Settings::load_or_create(&config_dir)?;
        Ok(Self::new(config_dir, settings))
    }
}
