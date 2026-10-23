use crate::widgets::{self as component, InputEvent, *};
use crate::{
    datetime,
    drag::{DropTarget, DropZone},
    editing::{self, CharRange},
    global_search::{self, MatchKey, ResultRow, VisibleRow},
    model::{NoteId, Notebook},
    search::{Cache, Pattern},
    selection::Selection,
    session::Session,
    settings::{Settings, ThemeMode},
    space, startup,
    storage::replace_file,
    store_lock::StoreLock,
    tabs::CloseMode,
};
use gpui::{prelude::*, *};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::PathBuf,
    rc::Rc,
};

mod app_state;
use app_state::AppState;
mod documents;
mod file_sync;
mod image_drop;
mod images;
use documents::{ClosedTab, OpenTab};
use preferences::Preferences;
use search_ui::SearchPanel;
mod interaction;
mod keyboard;
mod preferences;
mod search_ui;
mod shortcuts;
mod sidebar;
mod spaces;
#[cfg(test)]
mod tests;
mod trash_ui;
mod tree;
mod updates;
use spaces::{LoadedSpace, SpaceUi};
mod view;

mod menus;
mod metrics;
use metrics::*;

#[cfg(any(target_os = "macos", test))]
actions!(edot_app, [QuitEdot]);

#[cfg(any(target_os = "macos", test))]
fn macos_menu(window: AnyWindowHandle, view: &Entity<Edot>, cx: &mut App) {
    let quitting_view = view.downgrade();
    cx.on_app_quit(move |cx| {
        // The OS may quit without closing a window (for example from the Dock).
        // GPUI cannot veto at this stage, but we can still flush pending edits.
        let _ = window.update(cx, |_, window, cx| {
            let _ = quitting_view.update(cx, |this, cx| this.prepare_window_close(window, cx));
        });
        std::future::ready(())
    })
    .detach();
    let view = view.downgrade();
    cx.on_action(move |_: &QuitEdot, cx| {
        let _ = window.update(cx, |_, window, cx| {
            let _ = view.update(cx, |this, cx| this.close_window(window, cx));
        });
    });
    cx.bind_keys([KeyBinding::new("cmd-q", QuitEdot, None)]);
    let edit_items = component::input::EDIT_COMMANDS
        .iter()
        .zip([
            OsAction::Undo,
            OsAction::Redo,
            OsAction::Cut,
            OsAction::Copy,
            OsAction::Paste,
            OsAction::SelectAll,
        ])
        .map(|(command, os_action)| MenuItem::Action {
            name: command.label.into(),
            action: (command.action)(),
            os_action: Some(os_action),
            checked: false,
            disabled: false,
        });
    cx.set_menus([
        Menu::new("Edot").items([MenuItem::action("Edot 종료", QuitEdot)]),
        Menu::new("편집").items(edit_items),
    ]);
}

pub struct Edot {
    dir: PathBuf,
    space_ui: SpaceUi,
    _lock: Option<StoreLock>,
    book: Notebook,
    session: Session,
    open: Vec<OpenTab>,
    closed_tabs: Vec<ClosedTab>,
    shortcut_help: Option<Entity<shortcuts::ShortcutHelp>>,
    trash_browser: Option<Entity<trash_ui::TrashBrowser>>,
    tree_focus: FocusHandle,
    results_focus: FocusHandle,
    focus: FocusHandle,
    selection: Selection,
    caret: Option<NoteId>,
    collapsed: HashSet<NoteId>,
    rename: Option<(NoteId, FocusHandle)>,
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
    pressed: Option<NoteId>,
    drop_target: Option<DropTarget>,
    tab_drop: Option<usize>,
    resizing: Option<(Pixels, i32)>,
    sidebar_motion: sidebar::SidebarMotion,
    tree_auto: component::scroll::AutoScroll,
    tab_auto: component::scroll::AutoScroll,
    dragging_notes: Vec<NoteId>,
    tab_width: i32,
    keyboard_menu: Option<Entity<component::menu::PopupMenu>>,
    context_note: Option<NoteId>,
    menu_subscription: Option<Subscription>,
    menu_position: Point<Pixels>,
    menu_anchor: Anchor,
}

impl Edot {
    fn new(
        dir: PathBuf,
        loaded: Result<LoadedSpace, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (lock, book, session, error) = match loaded {
            Ok(LoadedSpace {
                lock,
                book,
                session,
            }) => (Some(lock), book, session, None),
            Err(error) => (None, Notebook::default(), Session::default(), Some(error)),
        };
        let collapsed = session.collapsed(&book);
        let local = SearchPanel::new(window, cx);
        let global = SearchPanel::new(window, cx);
        let title_input = cx.new(|cx| InputState::new(window, cx));
        let mut space_ui = SpaceUi::new(cx);
        space_ui.name = space::read_name(&dir).unwrap_or_else(|_| space::name(&dir));
        let registry = if error.is_none() {
            space::Registry::initialize(&cx.global::<AppState>().config_dir, &dir)
        } else {
            space::Registry::load(&cx.global::<AppState>().config_dir)
        };
        let registry_error = match registry {
            Ok(registry) => {
                space_ui.registry = registry;
                None
            }
            Err(error) => Some(format!("Space 목록을 읽지 못했습니다\n{error}")),
        };
        let sidebar_motion =
            sidebar::SidebarMotion::new(session.sidebar_visible, cx.background_executor().now());
        let mut this = Self {
            dir,
            space_ui,
            _lock: lock,
            book,
            session,
            open: vec![],
            closed_tabs: vec![],
            shortcut_help: None,
            trash_browser: None,
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
            sidebar_motion,
            tree_auto: Default::default(),
            tab_auto: Default::default(),
            dragging_notes: vec![],
            tab_width: TAB_PREFERRED_WIDTH,
            keyboard_menu: None,
            context_note: None,
            menu_subscription: None,
            menu_position: Point::default(),
            menu_anchor: Anchor::TopLeft,
        };
        if this.error.is_none() {
            this.error = registry_error;
        }
        window.set_window_title(&format!("Edot — {}", this.current_space_name()));
        let space_focus = this.space_ui.focus.clone();
        this.subscriptions.push(
            cx.on_focus_out(&space_focus, window, |this, _, window, cx| {
                let editing =
                    this.space_ui.rename.as_ref().is_some_and(|(_, input)| {
                        input.read(cx).focus_handle(cx).is_focused(window)
                    });
                if this.space_ui.open
                    && !editing
                    && !window.has_active_dialog(cx)
                    && this.error.is_none()
                {
                    this.space_ui.open = false;
                    this.space_ui.rename = None;
                    cx.notify();
                }
            }),
        );
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
                preview: false,
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
                if cx.global::<AppState>().settings.theme == ThemeMode::System {
                    this.apply_theme(window, cx);
                }
                // Wayland also reports decoration negotiation through this callback.
                #[cfg(target_os = "linux")]
                cx.notify();
            }));
        this.subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    let was_open = this.space_ui.open;
                    this.space_ui.open = false;
                    this.space_ui.rename = None;
                    if was_open {
                        cx.notify();
                    }
                }
                if window.is_window_active() && !this.startup_failed {
                    this.request_file_sync(crate::watch::Changes::all(), window, cx);
                }
                if !window.is_window_active() && this.context_note.is_some() {
                    this.keyboard_menu = None;
                    this.context_note = None;
                    this.menu_subscription = None;
                    cx.notify();
                }
                if !window.is_window_active() && !this.dragging_notes.is_empty() {
                    cx.stop_active_drag(window);
                    this.stop_drag();
                    cx.notify();
                }
            }));
        let weak = cx.entity().downgrade();
        #[cfg(any(target_os = "linux", test))]
        this.subscriptions
            .push(window.observe_button_layout_changed({
                let weak = weak.clone();
                move |_, cx| {
                    let _ = weak.update(cx, |_, cx| cx.notify());
                }
            }));
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.prepare_window_close(window, cx))
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
        } else if this.startup_failed {
            this.space_ui.focus.focus(window, cx);
        } else {
            this.tree_focus.focus(window, cx);
        }
        #[cfg(not(test))]
        if !this.startup_failed {
            this.start_file_sync(window, cx);
        }
        this
    }

    fn prepare_window_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.startup_failed {
            return true;
        }
        if !self.finish_rename(true, false, window, cx) || !self.save_all(cx) {
            return false;
        }
        match self.save_session() {
            Ok(()) => true,
            Err(error) => {
                self.fail("세션을 저장하지 못했습니다", error, cx);
                false
            }
        }
    }

    fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prepare_window_close(window, cx) {
            window.remove_window();
        }
    }

    fn fail(&mut self, action: &str, error: impl std::fmt::Display, cx: &mut Context<Self>) {
        self.error = Some(format!("{action}\n{error}"));
        self.status = "오류 · 저장 상태를 확인하세요".into();
        cx.notify();
    }
}

pub fn run() {
    startup::start();
    let paths = space::Paths::resolve();
    let (dir, config_dir) = paths.as_ref().map_or_else(
        |_| (PathBuf::new(), space::config_dir()),
        |paths| (paths.space.clone(), paths.config.clone()),
    );
    let state = AppState::load(config_dir.clone());
    let loaded = match &state {
        Ok(_) => paths.and_then(|paths| {
            let loaded = LoadedSpace::open(&paths)?;
            paths.remember()?;
            Ok(loaded)
        }),
        Err(error) => Err(io::Error::other(format!(
            "전역 설정을 읽지 못했습니다.\n{error}"
        ))),
    }
    .map_err(|error| format!("노트 저장소를 열지 못했습니다. 파일을 덮어쓰지 않습니다.\n{error}"));
    gpui_platform::application()
        .with_assets(component::Assets)
        .run(move |cx| {
            cx.set_http_client(std::sync::Arc::new(
                crate::image_http::ImageHttpClient::new(cx.background_executor().clone()),
            ));
            component::init(cx);
            cx.set_global(state.unwrap_or_else(|_| AppState::new(config_dir, Settings::default())));

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
                    title: Some(format!("Edot — {}", space::name(&dir)).into()),
                    appears_transparent: cfg!(target_os = "windows"),
                    ..Default::default()
                }),
                #[cfg(target_os = "linux")]
                window_decorations: Some(WindowDecorations::Server),
                ..Default::default()
            };
            if let Err(error) = component::open_window(options, cx, |window, cx| {
                let view = cx.new(|cx| Edot::new(dir, loaded, window, cx));
                #[cfg(target_os = "macos")]
                macos_menu(window.window_handle(), &view, cx);
                let weak = view.downgrade();
                // Next-frame callbacks run before presentation; the second turn starts
                // background work only after the first frame has been presented.
                window.on_next_frame(move |window, _| {
                    window.on_next_frame(move |window, cx| {
                        if let Err(error) = weak.update(cx, |this, cx| {
                            this.refresh_startup_fonts(window, cx);
                        }) {
                            eprintln!("글꼴 갱신을 시작하지 못했습니다: {error}");
                        }
                    });
                });
                if let Err(error) = image_drop::install(&view, window, cx) {
                    view.update(cx, |this, cx| {
                        this.fail("이미지 드롭을 준비하지 못했습니다", error, cx)
                    });
                }
                view
            }) {
                eprintln!("창을 열지 못했습니다: {error}");
                cx.quit();
            }
            if startup::finish() {
                cx.quit();
            }
        });
}
