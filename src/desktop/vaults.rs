//! Shared vault loading and same-window switching.
use super::*;

pub(super) struct LoadedVault {
    pub(super) lock: StoreLock,
    pub(super) book: Notebook,
    pub(super) settings: Settings,
    pub(super) session: Session,
}

impl LoadedVault {
    pub(super) fn open(paths: &vault::Paths) -> io::Result<Self> {
        Self::open_with_settings(paths, None)
    }

    fn open_with_settings(
        paths: &vault::Paths,
        running_settings: Option<Settings>,
    ) -> io::Result<Self> {
        let lock = StoreLock::acquire(&paths.vault)?;
        let _span = startup::span("notebook.load");
        let book = Notebook::load(&paths.vault)?;
        let settings = match running_settings {
            Some(settings) => settings,
            None => Settings::load_or_create(&paths.config)?,
        };
        let session = Session::load(&paths.vault, &book);
        if !paths.vault.join("tree.tsv").try_exists()? {
            book.save(&paths.vault)?;
        }
        if !paths.vault.join("session.tsv").try_exists()? {
            session.save(&paths.vault)?;
        }
        paths.remember()?;
        Ok(Self {
            lock,
            book,
            settings,
            session,
        })
    }
}

impl Edot {
    pub(super) fn open_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("메모 vault 폴더 선택".into()),
        });
        cx.spawn_in(window, async move |this, cx| match prompt.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.switch_vault(path, window, cx);
                    });
                }
            }
            Ok(Ok(None)) => {}
            error => {
                let _ = this.update(cx, |this, cx| {
                    this.fail("vault 폴더를 선택하지 못했습니다", format!("{error:?}"), cx)
                });
            }
        })
        .detach();
    }

    pub(super) fn switch_vault(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if fs::canonicalize(&path)
            .ok()
            .is_some_and(|path| Some(path) == fs::canonicalize(&self.dir).ok())
        {
            return true;
        }
        if !self.finish_rename(true, false, window, cx) || !self.save_all(cx) {
            return false;
        }
        if let Err(error) = self.save_session() {
            self.fail("세션을 저장하지 못했습니다", error, cx);
            return false;
        }
        let paths = vault::Paths {
            vault: path.clone(),
            config: self.config_dir.clone(),
        };
        match LoadedVault::open_with_settings(&paths, Some(self.settings.clone())) {
            Ok(loaded) => {
                window.set_window_title(&format!("Edot — {}", vault::name(&path)));
                *self = Self::new(path, self.config_dir.clone(), Ok(loaded), window, cx);
                cx.notify();
                true
            }
            Err(error) => {
                self.fail("vault를 열지 못했습니다", error, cx);
                false
            }
        }
    }
}
