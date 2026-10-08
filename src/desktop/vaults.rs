//! Shared vault loading and same-window switching.
use super::*;

pub(super) struct VaultUi {
    pub(super) name: String,
    pub(super) registry: vault::Registry,
    pub(super) open: bool,
    pub(super) focus: FocusHandle,
    pub(super) restore: Option<FocusHandle>,
    pub(super) cursor: usize,
    pub(super) rename: Option<(PathBuf, Entity<InputState>)>,
    pub(super) scroll: ScrollHandle,
    pub(super) bounds: Rc<std::cell::Cell<Bounds<Pixels>>>,
}

impl VaultUi {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            name: String::new(),
            registry: vault::Registry::default(),
            open: false,
            focus: cx.focus_handle(),
            restore: None,
            cursor: 0,
            rename: None,
            scroll: ScrollHandle::new(),
            bounds: Rc::new(std::cell::Cell::new(Bounds::default())),
        }
    }
}

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
        let switching = running_settings.is_some();
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
        vault::initialize_metadata(&paths.vault)?;
        if switching {
            vault::Registry::register(&paths.config, &paths.vault)?;
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
    pub(super) fn current_vault_name(&self) -> String {
        self.vault_ui.name.clone()
    }

    pub(super) fn toggle_vault_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.vault_ui.open {
            self.close_vault_list(window, cx);
            return;
        }
        match vault::Registry::load(&self.config_dir) {
            Ok(registry) => self.vault_ui.registry = registry,
            Err(error) => {
                self.fail("Vault 목록을 읽지 못했습니다", error, cx);
                return;
            }
        }
        if let Ok(name) = vault::read_name(&self.dir) {
            self.vault_ui.name = name;
            window.set_window_title(&format!("Edot — {}", self.current_vault_name()));
        }
        self.vault_ui.cursor = self
            .vault_ui
            .registry
            .entries
            .iter()
            .position(|entry| vault::same_path(&entry.path, &self.dir))
            .unwrap_or(0);
        self.vault_ui.restore = window.focused(cx);
        self.vault_ui.open = true;
        self.vault_ui.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_vault_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.vault_ui.open = false;
        self.vault_ui.rename = None;
        if let Some(focus) = self.vault_ui.restore.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn begin_vault_rename(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self
            .vault_ui
            .registry
            .entries
            .iter()
            .find(|entry| vault::same_path(&entry.path, &path))
        else {
            return;
        };
        let name = entry.name.clone();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        input.update(cx, |input, cx| {
            input.select_all(window, cx);
            input.focus(window, cx);
        });
        self.vault_ui.rename = Some((path, input));
        cx.notify();
    }

    pub(super) fn finish_vault_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, input)) = &self.vault_ui.rename else {
            return;
        };
        let name = input.read(cx).value().trim().to_owned();
        match vault::rename(path, &name) {
            Ok(()) => {
                if let Some(entry) = self
                    .vault_ui
                    .registry
                    .entries
                    .iter_mut()
                    .find(|entry| vault::same_path(&entry.path, path))
                {
                    entry.name = name.clone();
                }
                if vault::same_path(path, &self.dir) {
                    self.vault_ui.name = name;
                }
                self.vault_ui.rename = None;
                self.vault_ui.focus.focus(window, cx);
                window.set_window_title(&format!("Edot — {}", self.current_vault_name()));
                cx.notify();
            }
            Err(error) => self.fail("Vault 이름을 저장하지 못했습니다", error, cx),
        }
    }

    pub(super) fn remove_vault(
        &mut self,
        path: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match vault::Registry::remove(&self.config_dir, path) {
            Ok(registry) => {
                self.vault_ui.registry = registry;
                self.vault_ui.rename = None;
                self.vault_ui.cursor = self
                    .vault_ui
                    .cursor
                    .min(self.vault_ui.registry.entries.len().saturating_sub(1));
                window.set_window_title(&format!("Edot — {}", self.current_vault_name()));
                cx.notify();
                true
            }
            Err(error) => {
                self.fail("Vault를 목록에서 제거하지 못했습니다", error, cx);
                false
            }
        }
    }

    pub(super) fn confirm_remove_vault(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self
            .vault_ui
            .registry
            .entries
            .iter()
            .find(|entry| vault::same_path(&entry.path, &path))
            .map_or_else(|| vault::name(&path), |entry| entry.name.clone());
        let weak = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone();
            let path = path.clone();
            dialog
                .title("Vault 목록에서 삭제")
                .description(format!(
                    "‘{name}’을 목록에서 삭제할까요?\n폴더와 메모 파일은 그대로 보관됩니다."
                ))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("목록에서 삭제")
                        .cancel_text("취소"),
                )
                .on_ok(move |_, window, cx| {
                    weak.update(cx, |this, cx| this.remove_vault(&path, window, cx))
                        .unwrap_or(false)
                })
        });
    }

    pub(super) fn open_vault_folder(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if path.is_dir() {
            cx.open_with_system(path);
        } else {
            self.fail(
                "Vault 폴더를 열지 못했습니다",
                "폴더가 존재하지 않습니다.",
                cx,
            );
        }
    }

    pub(super) fn open_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_vault_list(window, cx);
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
        if vault::same_path(&path, &self.dir) {
            return match vault::Registry::register(&self.config_dir, &path) {
                Ok(registry) => {
                    self.vault_ui.registry = registry;
                    if let Ok(name) = vault::read_name(&self.dir) {
                        self.vault_ui.name = name;
                        window.set_window_title(&format!("Edot — {}", self.current_vault_name()));
                    }
                    self.close_vault_list(window, cx);
                    true
                }
                Err(error) => {
                    self.fail("Vault를 등록하지 못했습니다", error, cx);
                    false
                }
            };
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

    pub(super) fn select_registered_vault(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !path.is_dir() {
            self.fail(
                "Vault를 열지 못했습니다",
                "등록된 폴더가 존재하지 않습니다.",
                cx,
            );
            return;
        }
        self.switch_vault(path, window, cx);
    }
}
