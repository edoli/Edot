use crate::widgets::{self as component, InputEvent, *};
use crate::{
    datetime,
    drag::{DropTarget, DropZone},
    editing::{self, CharRange},
    global_search::{self, MatchKey, ResultRow, VisibleRow},
    model::Notebook,
    platform::StoreLock,
    search::{Cache, Pattern},
    selection::Selection,
    session::Session,
    settings::{Settings, ThemeMode},
    startup,
    storage::replace_file,
    tabs::CloseMode,
    vault,
};
use gpui::{prelude::*, *};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::PathBuf,
    rc::Rc,
};

mod documents;
mod file_sync;
use documents::{ClosedTab, OpenTab};
use preferences::Preferences;
use search_ui::SearchPanel;
mod interaction;
mod keyboard;
mod preferences;
mod search_ui;
mod shortcuts;
#[cfg(test)]
mod tests;
mod tree;
mod vaults;
use vaults::LoadedVault;
mod view;

mod menus;
mod metrics;
use metrics::*;

pub struct Edot {
    dir: PathBuf,
    config_dir: PathBuf,
    _lock: Option<StoreLock>,
    book: Notebook,
    settings: Settings,
    session: Session,
    open: Vec<OpenTab>,
    closed_tabs: Vec<ClosedTab>,
    shortcut_help: Option<Entity<shortcuts::ShortcutHelp>>,
    tree_focus: FocusHandle,
    results_focus: FocusHandle,
    focus: FocusHandle,
    selection: Selection,
    caret: Option<u64>,
    collapsed: HashSet<u64>,
    rename: Option<(u64, FocusHandle)>,
    title_input: Entity<InputState>,
    local: SearchPanel,
    global: SearchPanel,
    find_visible: bool,
    search_mode: bool,
    settings_visible: bool,
    preferences: Option<Preferences>,
    search_results: search_ui::SearchResults,
    autosave: Option<Task<()>>,
    file_sync: file_sync::FileSync,
    zoom: u32,
    status: String,
    error: Option<String>,
    startup_failed: bool,
    subscriptions: Vec<Subscription>,
    tree_scroll: UniformListScrollHandle,
    tree_bounds: Rc<std::cell::Cell<Bounds<Pixels>>>,
    tab_scroll: ScrollHandle,
    pressed: Option<u64>,
    drop_target: Option<DropTarget>,
    tab_drop: Option<usize>,
    resizing: Option<(Pixels, i32)>,
    #[cfg(test)]
    splitter_probe: Rc<std::cell::RefCell<Option<(Hitbox, bool, CursorStyle)>>>,
    tree_auto: component::scroll::AutoScroll,
    tab_auto: component::scroll::AutoScroll,
    dragging_notes: Vec<u64>,
    tab_width: i32,
    keyboard_menu: Option<Entity<component::menu::PopupMenu>>,
    menu_subscription: Option<Subscription>,
    menu_position: Point<Pixels>,
}

impl Edot {
    fn new(
        dir: PathBuf,
        config_dir: PathBuf,
        loaded: Result<LoadedVault, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (lock, book, settings, session, error) = match loaded {
            Ok(LoadedVault {
                lock,
                book,
                settings,
                session,
            }) => (Some(lock), book, settings, session, None),
            Err(error) => (
                None,
                Notebook::default(),
                Settings::default(),
                Session::default(),
                Some(error),
            ),
        };
        let collapsed = session.collapsed(&book);
        let local = SearchPanel::new(window, cx);
        let global = SearchPanel::new(window, cx);
        let title_input = cx.new(|cx| InputState::new(window, cx));
        let mut this = Self {
            dir,
            config_dir,
            _lock: lock,
            book,
            settings,
            session,
            open: vec![],
            closed_tabs: vec![],
            shortcut_help: None,
            tree_focus: cx.focus_handle(),
            results_focus: cx.focus_handle(),
            focus: cx.focus_handle(),
            selection: Selection::default(),
            caret: None,
            collapsed,
            rename: None,
            title_input,
            local,
            global,
            find_visible: false,
            search_mode: false,
            settings_visible: false,
            preferences: None,
            search_results: search_ui::SearchResults::default(),
            autosave: None,
            file_sync: file_sync::FileSync::default(),
            zoom: ZOOM_DEFAULT,
            status: "준비됨".into(),
            startup_failed: error.is_some(),
            error,
            subscriptions: vec![],
            tree_scroll: UniformListScrollHandle::new(),
            tree_bounds: Default::default(),
            tab_scroll: ScrollHandle::new(),
            pressed: None,
            drop_target: None,
            tab_drop: None,
            resizing: None,
            #[cfg(test)]
            splitter_probe: Default::default(),
            tree_auto: Default::default(),
            tab_auto: Default::default(),
            dragging_notes: vec![],
            tab_width: TAB_PREFERRED_WIDTH,
            keyboard_menu: None,
            menu_subscription: None,
            menu_position: Point::default(),
        };
        this.apply_theme(window, cx);
        this.open = this
            .session
            .tabs
            .iter()
            .map(|id| OpenTab {
                id: *id,
                editor: None,
                saved: String::new(),
                saved_revision: 0,
                metrics: Default::default(),
                metrics_revision: 0,
                dirty: false,
                _subscriptions: vec![],
            })
            .collect();
        for global in [false, true] {
            let panel = if global { &this.global } else { &this.local };
            this.subscriptions.push(cx.subscribe_in(
                &panel.query,
                window,
                move |this, _, event, window, cx| match event {
                    InputEvent::Change => {
                        if global {
                            this.queue_search(window, cx);
                        } else {
                            this.find_next(false, true, window, cx);
                        }
                    }
                    InputEvent::PressEnter { shift, .. } => {
                        this.navigate(global, *shift, window, cx)
                    }
                    _ => {}
                },
            ));
            this.subscriptions.push(cx.subscribe_in(
                &panel.replacement,
                window,
                move |this, _, event, window, cx| {
                    if let InputEvent::PressEnter { shift, .. } = event {
                        if global {
                            this.navigate(true, *shift, window, cx);
                        } else {
                            this.replace_local(false, window, cx);
                        }
                    }
                },
            ));
        }
        this.subscriptions.push(cx.subscribe_in(
            &this.title_input,
            window,
            |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.finish_rename(true, true, window, cx);
                }
                if matches!(event, InputEvent::Blur) {
                    this.finish_rename(true, false, window, cx);
                }
            },
        ));
        this.subscriptions
            .push(cx.observe_window_appearance(window, |this, window, cx| {
                if this.settings.theme == ThemeMode::System {
                    this.apply_theme(window, cx);
                }
            }));
        this.subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() && !this.startup_failed {
                    this.request_file_sync(crate::watch::Changes::all(), window, cx);
                }
                if !window.is_window_active() && !this.dragging_notes.is_empty() {
                    cx.stop_active_drag(window);
                    this.stop_drag();
                    cx.notify();
                }
            }));
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| {
                if this.startup_failed {
                    return true;
                }
                if !this.finish_rename(true, false, window, cx) || !this.save_all(cx) {
                    return false;
                }
                match this.save_session() {
                    Ok(()) => true,
                    Err(error) => {
                        this.fail("세션을 저장하지 못했습니다", error, cx);
                        false
                    }
                }
            })
            .unwrap_or(true)
        });
        let active = this.session.active;
        this.session.active = None;
        if let Some(id) = active {
            // Restore the active editor without changing saved tree expansion.
            this.open_note(id, false, window, cx);
            this.caret = Some(id);
            this.selection.single(id);
            this.focus_editor(window, cx);
        } else {
            this.tree_focus.focus(window, cx);
        }
        #[cfg(not(test))]
        if !this.startup_failed {
            this.start_file_sync(window, cx);
        }
        this
    }

    fn fail(&mut self, action: &str, error: impl std::fmt::Display, cx: &mut Context<Self>) {
        self.error = Some(format!("{action}\n{error}"));
        self.status = "오류 · 저장 상태를 확인하세요".into();
        cx.notify();
    }
}

pub fn run() {
    startup::start();
    let paths = vault::Paths::resolve();
    let (dir, config_dir) = paths.as_ref().map_or_else(
        |_| (PathBuf::new(), vault::config_dir()),
        |paths| (paths.vault.clone(), paths.config.clone()),
    );
    let loaded = paths
        .and_then(|paths| LoadedVault::open(&paths))
        .map_err(|error| {
            format!("메모 저장소를 열지 못했습니다. 파일을 덮어쓰지 않습니다.\n{error}")
        });
    gpui_platform::application()
        .with_assets(component::Assets)
        .run(move |cx| {
            component::init(cx);

            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1100.), px(760.)),
                    cx,
                ))),
                window_min_size: Some(size(px(560.), px(360.))),
                titlebar: Some(TitlebarOptions {
                    title: Some(format!("Edot — {}", vault::name(&dir)).into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            if let Err(error) = component::open_window(options, cx, |window, cx| {
                cx.new(|cx| Edot::new(dir, config_dir, loaded, window, cx))
            }) {
                eprintln!("창을 열지 못했습니다: {error}");
                cx.quit();
            }
            if startup::finish() {
                cx.quit();
            }
        });
}
