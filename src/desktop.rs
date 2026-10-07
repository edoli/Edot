use crate::widgets::{self as component, InputEvent, *};
use crate::{
    datetime,
    drag::{DropTarget, DropZone},
    editing::{self, CharRange},
    global_search::{self, MatchKey, ResultRow, VisibleRow},
    model::{Notebook, replace_file},
    platform::{self, StoreLock},
    search::{Cache, Pattern},
    selection::Selection,
    session::Session,
    settings::{Settings, ThemeMode},
    startup,
};
use gpui::{prelude::*, *};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

mod interaction;
mod keyboard;
mod search_ui;
#[cfg(test)]
mod tests;
mod view;

const SIDEBAR_RESIZE_WIDTH: f32 = 6.;
const TAB_MIN_WIDTH: i32 = 140;
const TREE_ROW_HEIGHT: f32 = 29.;
const TREE_INDENT: f32 = 22.;
// Row padding + disclosure width + gap: align insertion lines with the note icon.
const TREE_ICON_LEFT: f32 = 24.;

struct OpenTab {
    id: u64,
    editor: Option<Entity<TextareaState>>,
    saved: String,
    metrics: editing::TextMetrics,
    metrics_revision: u64,
    dirty: bool,
    _subscriptions: Vec<Subscription>,
}

struct SearchPanel {
    query: Entity<InputState>,
    replacement: Entity<InputState>,
    case: bool,
    regex: bool,
    replace: bool,
    message: String,
    cache: Cache,
    last: Option<MatchKey>,
}
impl SearchPanel {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            query: cx.new(|cx| InputState::new(window, cx).placeholder("찾을 내용")),
            replacement: cx.new(|cx| InputState::new(window, cx).placeholder("바꿀 내용")),
            case: false,
            regex: false,
            replace: false,
            message: "검색어를 입력하세요".into(),
            cache: Cache::default(),
            last: None,
        }
    }
    fn pattern(&mut self, cx: &App) -> Result<Option<Pattern>, String> {
        let query = self.query.read(cx).value();
        if query.is_empty() {
            return Ok(None);
        }
        self.cache.get(&query, self.case, self.regex).map(Some)
    }
}

struct Preferences {
    focus: FocusHandle,
    theme: Entity<SelectState<Vec<SharedString>>>,
    font: Entity<SelectState<Vec<SharedString>>>,
    size: Entity<SelectState<Vec<SharedString>>>,
    wrap_focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

pub struct Edot {
    dir: PathBuf,
    _lock: Option<StoreLock>,
    book: Notebook,
    settings: Settings,
    session: Session,
    open: Vec<OpenTab>,
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
    rows: Vec<ResultRow>,
    result_rows: Vec<VisibleRow>,
    result_collapsed: HashSet<u64>,
    result_cursor: Option<usize>,
    search_generation: u64,
    scanning: bool,
    pending_navigation: Option<bool>,
    global_task: Option<Task<()>>,
    note_search_tasks: HashMap<u64, Task<()>>,
    pending_search_notes: HashSet<u64>,
    search_errors: usize,
    autosave: Option<Task<()>>,
    zoom: u32,
    status: String,
    error: Option<String>,
    startup_failed: bool,
    subscriptions: Vec<Subscription>,
    tree_scroll: UniformListScrollHandle,
    tree_bounds: Rc<std::cell::Cell<Bounds<Pixels>>>,
    result_scroll: UniformListScrollHandle,
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
        loaded: Result<(StoreLock, Notebook, Settings), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (lock, book, settings, error) = match loaded {
            Ok((lock, book, settings)) => (Some(lock), book, settings, None),
            Err(error) => (None, Notebook::default(), Settings::default(), Some(error)),
        };
        let session = Session::load(&dir, &book);
        let local = SearchPanel::new(window, cx);
        let global = SearchPanel::new(window, cx);
        let title_input = cx.new(|cx| InputState::new(window, cx));
        let mut this = Self {
            dir,
            _lock: lock,
            book,
            settings,
            session,
            open: vec![],
            tree_focus: cx.focus_handle(),
            results_focus: cx.focus_handle(),
            focus: cx.focus_handle(),
            selection: Selection::default(),
            caret: None,
            collapsed: HashSet::new(),
            rename: None,
            title_input,
            local,
            global,
            find_visible: false,
            search_mode: false,
            settings_visible: false,
            preferences: None,
            rows: vec![],
            result_rows: vec![],
            result_collapsed: HashSet::new(),
            result_cursor: None,
            search_generation: 0,
            scanning: false,
            pending_navigation: None,
            global_task: None,
            note_search_tasks: HashMap::new(),
            pending_search_notes: HashSet::new(),
            search_errors: 0,
            autosave: None,
            zoom: 100,
            status: "준비됨".into(),
            startup_failed: error.is_some(),
            error,
            subscriptions: vec![],
            tree_scroll: UniformListScrollHandle::new(),
            tree_bounds: Default::default(),
            result_scroll: UniformListScrollHandle::new(),
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
            tab_width: 180,
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
            this.open_note(id, true, window, cx);
        } else {
            this.tree_focus.focus(window, cx);
        }
        this
    }

    fn fail(&mut self, action: &str, error: impl std::fmt::Display, cx: &mut Context<Self>) {
        self.error = Some(format!("{action}\n{error}"));
        self.status = "오류 · 저장 상태를 확인하세요".into();
        cx.notify();
    }
    fn active_editor(&self) -> Option<Entity<TextareaState>> {
        self.open
            .iter()
            .find(|tab| Some(tab.id) == self.session.active)
            .and_then(|tab| tab.editor.clone())
    }
    fn text(&self, id: u64, cx: &App) -> io::Result<String> {
        if let Some(editor) = self
            .open
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| tab.editor.as_ref())
        {
            return Ok(editor.read(cx).value().to_string());
        }
        fs::read_to_string(self.book.file(&self.dir, id))
    }
    fn focus_editor(&self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = self.active_editor() {
            editor.update(cx, |state, cx| state.focus(window, cx));
        } else {
            self.tree_focus.focus(window, cx);
        }
    }
    fn open_note(
        &mut self,
        id: u64,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.book.get(id).is_none() || !self.finish_rename(true, false, window, cx) {
            return false;
        }
        let existing = self.open.iter().position(|tab| tab.id == id);
        if existing.is_none_or(|i| self.open[i].editor.is_none()) {
            let text = match fs::read_to_string(self.book.file(&self.dir, id)) {
                Ok(text) => text,
                Err(error) => {
                    self.fail("메모를 열지 못했습니다", error, cx);
                    return false;
                }
            };
            let editor = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .content_padding(Edges {
                        left: px(16.),
                        top: px(6.),
                        right: px(4.),
                        bottom: px(0.),
                    })
                    .soft_wrap(self.settings.wrap)
                    .default_value(text.clone())
            });
            let subscription =
                cx.subscribe_in(&editor, window, move |this, _, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.changed(id, window, cx);
                    }
                    cx.notify();
                });
            let observe = cx.observe(&editor, |_, _, cx| cx.notify());
            let tab = OpenTab {
                id,
                editor: Some(editor),
                metrics: editing::TextMetrics::new(&text),
                metrics_revision: 0,
                saved: text,
                dirty: false,
                _subscriptions: vec![subscription, observe],
            };
            if let Some(index) = existing {
                self.open[index] = tab;
            } else {
                self.open.push(tab);
            }
        }
        self.session.active = Some(id);
        self.local.last = None;
        if focus {
            self.focus_editor(window, cx);
            if !self.search_mode {
                self.caret = Some(id);
                if self.selection.ids.len() <= 1 {
                    self.selection.single(id);
                }
                let mut parent = self.book.get(id).unwrap().parent;
                while parent != 0 {
                    self.collapsed.remove(&parent);
                    parent = self.book.get(parent).unwrap().parent;
                }
                self.reveal_tree(id);
            }
        }
        if let Some(index) = self.open.iter().position(|tab| tab.id == id) {
            self.tab_scroll.scroll_to_item(index);
        }
        self.status = if self.open.iter().any(|tab| tab.dirty) {
            "저장 중…"
        } else {
            "저장됨"
        }
        .into();
        cx.notify();
        true
    }
    fn changed(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.open.iter_mut().find(|tab| tab.id == id)
            && let Some(editor) = &tab.editor
        {
            let text = editor.read(cx).value();
            tab.dirty = text.as_ref() != tab.saved;
            tab.metrics = editing::TextMetrics::new(&text);
            tab.metrics_revision = editor.read(cx).revision;
        }
        self.status = "저장 중…".into();
        self.autosave = Some(cx.spawn(async |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(650))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.save_all(cx);
            });
        }));
        if self.search_mode {
            self.queue_note_search(id, window, cx);
        }
        cx.notify();
    }
    fn save_all(&mut self, cx: &mut Context<Self>) -> bool {
        if self.startup_failed {
            return false;
        }
        for tab in &mut self.open {
            if !tab.dirty {
                continue;
            }
            let Some(editor) = &tab.editor else {
                continue;
            };
            let text = editor.read(cx).value().to_string();
            if let Err(error) = replace_file(&self.book.file(&self.dir, tab.id), text.as_bytes()) {
                self.fail(
                    "메모를 저장하지 못했습니다. 수정 내용은 편집기에 남아 있습니다",
                    error,
                    cx,
                );
                return false;
            }
            tab.saved = text;
            tab.dirty = false;
        }
        self.status = "저장됨".into();
        cx.notify();
        true
    }
    fn save_session(&mut self) -> io::Result<()> {
        self.session.tabs = self.open.iter().map(|tab| tab.id).collect();
        self.session.save(&self.dir)
    }
    fn close_tabs(&mut self, id: u64, mode: u8, window: &mut Window, cx: &mut Context<Self>) {
        if !self.finish_rename(true, false, window, cx) || !self.save_all(cx) {
            return;
        }
        let Some(index) = self.open.iter().position(|tab| tab.id == id) else {
            return;
        };
        let mut cursor = 0;
        self.open.retain(|_| {
            let keep = match mode {
                1 => cursor == index,
                2 => cursor <= index,
                _ => cursor != index,
            };
            cursor += 1;
            keep
        });
        if self
            .session
            .active
            .is_some_and(|id| !self.open.iter().any(|tab| tab.id == id))
        {
            self.session.active = None;
            if let Some(tab) = self.open.get(index.min(self.open.len().saturating_sub(1))) {
                self.open_note(tab.id, true, window, cx);
            }
        }
        cx.notify();
    }
    fn create_note(&mut self, child: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        if self.startup_failed {
            return;
        }
        let parent = child.unwrap_or_else(|| {
            self.caret
                .and_then(|id| self.book.get(id))
                .map_or(0, |n| n.parent)
        });
        let previous = self.book.clone();
        let id = self.book.add(parent, "새 메모".into());
        let file = self.book.file(&self.dir, id);
        if let Err(error) = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
        {
            self.book = previous;
            self.fail("메모를 만들지 못했습니다", error, cx);
            return;
        }
        if let Err(error) = self.book.save(&self.dir) {
            self.book = previous;
            let _ = fs::remove_file(file);
            self.fail("메모 목록을 저장하지 못했습니다", error, cx);
            return;
        }
        self.collapsed.remove(&parent);
        self.search_mode = false;
        self.selection.single(id);
        self.caret = Some(id);
        if self.open_note(id, true, window, cx) {
            self.begin_rename(id, window, cx);
        }
    }
    fn begin_rename(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.rename.is_some() {
            self.title_input
                .update(cx, |state, cx| state.focus(window, cx));
            return;
        }
        if self.session.sidebar_visible && self.selection.ids.len() > 1 {
            return;
        }
        let Some(note) = self.book.get(id) else {
            return;
        };
        self.rename = Some((id, window.focused(cx).unwrap_or_else(|| self.focus.clone())));
        let title = note.title.clone();
        if self.session.sidebar_visible {
            self.search_mode = false;
            self.reveal_tree(id);
        }
        self.title_input.update(cx, |state, cx| {
            state.set_value(title, window, cx);
            state.select_all(window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }
    fn finish_rename(
        &mut self,
        commit: bool,
        restore: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((id, focus)) = self.rename.take() else {
            return true;
        };
        let title = self.title_input.read(cx).value().trim().to_owned();
        let mut success = true;
        if commit && !title.is_empty() && self.book.get(id).is_some_and(|n| n.title != title) {
            let previous = self.book.clone();
            self.book.get_mut(id).unwrap().title = title;
            if let Err(error) = self.book.save(&self.dir) {
                self.book = previous;
                self.fail("이름을 저장하지 못했습니다", error, cx);
                success = false;
            }
        }
        if restore {
            focus.focus(window, cx);
        }
        cx.notify();
        success
    }
    fn visible_tree(&self) -> Vec<(u64, usize)> {
        let mut out = Vec::new();
        let mut pending: Vec<_> = self
            .book
            .children(0)
            .into_iter()
            .rev()
            .map(|id| (id, 0))
            .collect();
        while let Some((id, depth)) = pending.pop() {
            out.push((id, depth));
            if !self.collapsed.contains(&id) {
                pending.extend(
                    self.book
                        .children(id)
                        .into_iter()
                        .rev()
                        .map(|id| (id, depth + 1)),
                );
            }
        }
        out
    }
    fn reveal_tree(&self, id: u64) {
        if let Some(index) = self.visible_tree().iter().position(|(row, _)| *row == id) {
            self.tree_scroll
                .scroll_to_item(index, ScrollStrategy::Nearest);
        }
    }
    fn tree_click(
        &mut self,
        id: u64,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let visible: Vec<_> = self.visible_tree().iter().map(|(id, _)| *id).collect();
        let preserve = !modifiers.control && !modifiers.shift && self.selection.ids.contains(&id);
        if !preserve {
            self.selection
                .click(id, &visible, modifiers.control, modifiers.shift);
        }
        self.pressed = preserve.then_some(id);
        self.caret = Some(id);
        if !modifiers.control && !modifiers.shift {
            self.open_note(id, false, window, cx);
        }
        self.tree_focus.focus(window, cx);
        cx.notify();
    }
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected: Vec<_> = self.selection.ids.iter().copied().collect();
        let count = self.book.selected_subtrees(&selected).len();
        if count == 0 {
            return;
        }
        let weak = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone(); let selected = selected.clone();
            dialog.title("메모 삭제").description(format!("선택한 항목과 하위 메모 {count}개를 삭제할까요?\n파일은 휴지통 폴더에 보관됩니다.")).confirm()
                .button_props(DialogButtonProps::default().ok_text("삭제").cancel_text("취소"))
                .on_ok(move |_, window, cx| weak.update(cx, |this, cx| this.delete_confirmed(&selected, window, cx)).unwrap_or(false))
        });
    }
    fn delete_confirmed(
        &mut self,
        selected: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.save_all(cx) {
            return false;
        }
        let ids = match self.book.trash_notes(&self.dir, selected) {
            Ok(ids) => ids,
            Err(error) => {
                self.fail("메모를 삭제하지 못했습니다", error, cx);
                return false;
            }
        };
        let active_index = self
            .open
            .iter()
            .position(|tab| Some(tab.id) == self.session.active)
            .unwrap_or(0);
        self.open.retain(|tab| !ids.contains(&tab.id));
        self.selection.retain(|id| !ids.contains(&id));
        self.collapsed.retain(|id| !ids.contains(id));
        if self.caret.is_some_and(|id| ids.contains(&id)) {
            self.caret = None;
        }
        if self.session.active.is_some_and(|id| ids.contains(&id)) {
            self.session.active = None;
            if let Some(tab) = self
                .open
                .get(active_index.min(self.open.len().saturating_sub(1)))
            {
                self.open_note(tab.id, false, window, cx);
            }
        }
        self.tree_focus.focus(window, cx);
        if self.search_mode {
            self.queue_search(window, cx);
        }
        cx.notify();
        true
    }
    fn group_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection.ids.len() < 2 {
            return;
        }
        let ids: Vec<_> = self.selection.ids.iter().copied().collect();
        let previous = self.book.clone();
        let Some(id) = self.book.group_notes(&ids) else {
            return;
        };
        let file = self.book.file(&self.dir, id);
        if let Err(error) = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
        {
            self.book = previous;
            self.fail("폴더를 만들지 못했습니다", error, cx);
            return;
        }
        if let Err(error) = self.book.save(&self.dir) {
            self.book = previous;
            let _ = fs::remove_file(file);
            self.fail("폴더를 저장하지 못했습니다", error, cx);
            return;
        }
        self.selection.single(id);
        self.caret = Some(id);
        self.collapsed.remove(&id);
        self.open_note(id, true, window, cx);
        self.begin_rename(id, window, cx);
    }
    fn move_notes(&mut self, ids: &[u64], target: DropTarget, cx: &mut Context<Self>) {
        let previous = self.book.clone();
        if self.book.move_notes(ids, target.parent, target.before) {
            if let Err(error) = self.book.save(&self.dir) {
                self.book = previous;
                self.fail("이동을 저장하지 못했습니다", error, cx);
            } else {
                self.collapsed.remove(&target.parent);
                self.status = "메모를 이동했습니다".into();
            }
        }
        self.drop_target = None;
        self.pressed = None;
        cx.notify();
    }
    fn outdent(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(parent) = self
            .book
            .get(id)
            .map(|note| note.parent)
            .filter(|id| *id != 0)
        else {
            return;
        };
        if let Some(target) =
            DropTarget::resolve_many(&self.book, &[id], Some(parent), DropZone::After)
        {
            self.move_notes(&[id], target, cx);
        }
    }
    fn reorder_tab(&mut self, id: u64, gap: usize, cx: &mut Context<Self>) {
        let Some(index) = self.open.iter().position(|tab| tab.id == id) else {
            return;
        };
        let item = self.open.remove(index);
        let target = gap
            .saturating_sub(usize::from(gap > index))
            .min(self.open.len());
        self.open.insert(target, item);
        if let Err(error) = self.save_session() {
            let item = self.open.remove(target);
            self.open.insert(index, item);
            self.fail("탭 순서를 저장하지 못했습니다", error, cx);
        }
        self.tab_drop = None;
        cx.notify();
    }
    fn apply_theme(&self, window: &mut Window, cx: &mut App) {
        let dark = match self.settings.theme {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        };
        Theme::update(cx, |theme| {
            theme.font_family = "Segoe UI".into();
            theme.font_size = px(15.);
            theme.mono_font_family = self.settings.font.clone().into();
            theme.mono_font_size = px(self.settings.font_size as f32);
            theme.colors.background = rgb(if dark { 0x1e2127 } else { 0xffffff }).into();
            theme.colors.foreground = rgb(if dark { 0xe5e9f0 } else { 0x1f2937 }).into();
            theme.colors.bar = rgb(if dark { 0x15181d } else { 0xf1f4f8 }).into();
            theme.colors.sidebar = rgb(if dark { 0x181b21 } else { 0xf5f7fa }).into();
            theme.colors.border = rgb(if dark { 0x3e4450 } else { 0xdce2ea }).into();
            theme.colors.primary = rgb(if dark { 0x6ba4ff } else { 0x2674f5 }).into();
            theme.colors.selection = rgb(if dark { 0x2b4162 } else { 0xddebff }).into();
            theme.colors.popover = theme.colors.background;
            theme.colors.popover_foreground = theme.colors.foreground;
            theme.colors.accent = rgb(if dark { 0x292e38 } else { 0xe8edf5 }).into();
            theme.colors.accent_foreground = theme.colors.foreground;
            theme.colors.destructive = rgb(if dark { 0xff8589 } else { 0xc42b32 }).into();
            theme.colors.muted_foreground = rgb(if dark { 0xa5afbf } else { 0x657187 }).into();
            theme.colors.primary_foreground = rgb(if dark { 0x10151d } else { 0xffffff }).into();
            theme.colors.ring = theme.colors.primary;
            theme.colors.primary_hover = rgb(if dark { 0x86b5ff } else { 0x1b63d9 }).into();
            theme.colors.primary_active = rgb(if dark { 0x5591ed } else { 0x1656bf }).into();
            theme.colors.button_primary = theme.colors.primary;
            theme.colors.button_primary_foreground = theme.colors.primary_foreground;
            theme.colors.button_primary_hover = theme.colors.primary_hover;
            theme.colors.button_primary_active = theme.colors.primary_active;
            theme.colors.list = theme.colors.background;
            theme.colors.list_even = theme.colors.background;
            theme.colors.list_head = theme.colors.accent;
            theme.colors.list_hover = theme.colors.accent;
            theme.colors.list_active = theme.colors.selection;
            theme.colors.list_active_border = theme.colors.primary;
        });
    }
    fn change_settings(&mut self, next: Settings, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = next.save(&self.dir) {
            self.fail("설정을 저장하지 못했습니다", error, cx);
            self.sync_preferences(window, cx);
            return;
        }
        self.settings = next;
        self.apply_theme(window, cx);
        for tab in &self.open {
            if let Some(editor) = &tab.editor {
                editor.update(cx, |state, cx| {
                    let offset = state.scroll_offset();
                    state.set_soft_wrap(self.settings.wrap, window, cx);
                    state.set_scroll_offset(offset, cx);
                });
            }
        }
        cx.notify();
    }
    fn sync_preferences(&self, window: &mut Window, cx: &mut App) {
        let Some(prefs) = &self.preferences else {
            return;
        };
        let mode: SharedString = match self.settings.theme {
            ThemeMode::Light => "밝게",
            ThemeMode::Dark => "어둡게",
            ThemeMode::System => "시스템",
        }
        .into();
        prefs
            .theme
            .update(cx, |s, cx| s.set_selected_value(&mode, window, cx));
        prefs.font.update(cx, |s, cx| {
            s.set_selected_value(&self.settings.font.clone().into(), window, cx)
        });
        prefs.size.update(cx, |s, cx| {
            s.set_selected_value(&self.settings.font_size.to_string().into(), window, cx)
        });
    }
    fn dismiss_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_visible = false;
        if let Some(prefs) = &self.preferences {
            for select in [&prefs.theme, &prefs.font, &prefs.size] {
                select.update(cx, |s, cx| s.close(cx));
            }
        }
        cx.notify();
    }
    fn show_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_visible = !self.settings_visible;
        if !self.settings_visible {
            self.focus_editor(window, cx);
            cx.notify();
            return;
        }
        self.find_visible = false;
        if self.preferences.is_none() {
            let mut fonts = cx.text_system().all_font_names();
            // Keep names compatible with the existing settings format and omit
            // vertical aliases, as the previous GDI enumeration did.
            fonts.retain(|name| crate::settings::valid_font(name) && !name.starts_with('@'));
            if !fonts.contains(&self.settings.font) {
                fonts.push(self.settings.font.clone());
            }
            fonts.sort_unstable();
            fonts.dedup();
            let theme = cx.new(|cx| {
                SelectState::new(
                    vec!["밝게".into(), "어둡게".into(), "시스템".into()],
                    None,
                    window,
                    cx,
                )
            });
            let font = cx.new(|cx| {
                SelectState::new(
                    fonts
                        .into_iter()
                        .map(SharedString::from)
                        .collect::<Vec<_>>(),
                    None,
                    window,
                    cx,
                )
                .searchable(true)
            });
            let size = cx.new(|cx| {
                SelectState::new(
                    (8..=72)
                        .map(|n| SharedString::from(n.to_string()))
                        .collect::<Vec<_>>(),
                    None,
                    window,
                    cx,
                )
                .suffix(" px")
            });
            let mut subscriptions = vec![];
            for (kind, entity) in [(0, &theme), (1, &font), (2, &size)] {
                subscriptions.push(cx.subscribe_in(
                    entity,
                    window,
                    move |this, _, event, window, cx| {
                        let SelectEvent::Confirm(Some(value)) = event else {
                            return;
                        };
                        let mut next = this.settings.clone();
                        match kind {
                            0 => {
                                next.theme = match value.as_ref() {
                                    "밝게" => ThemeMode::Light,
                                    "어둡게" => ThemeMode::Dark,
                                    _ => ThemeMode::System,
                                }
                            }
                            1 => next.font = value.to_string(),
                            _ => next.font_size = value.parse().unwrap_or(18),
                        }
                        if next != this.settings {
                            this.change_settings(next, window, cx);
                        }
                    },
                ));
            }
            let focus = cx.focus_handle();
            subscriptions.push(cx.on_focus_out(&focus, window, |this, _, _, cx| {
                if this.settings_visible {
                    this.dismiss_settings(cx);
                }
            }));
            subscriptions.push(cx.observe_window_activation(window, |this, window, cx| {
                if this.settings_visible && !window.is_window_active() {
                    this.dismiss_settings(cx);
                }
            }));
            self.preferences = Some(Preferences {
                focus,
                theme,
                font,
                size,
                wrap_focus: cx.focus_handle().tab_stop(true),
                _subscriptions: subscriptions,
            });
        }
        if let Some(prefs) = &self.preferences {
            for select in [&prefs.theme, &prefs.font, &prefs.size] {
                select.update(cx, |s, cx| s.close(cx));
            }
        }
        self.sync_preferences(window, cx);
        self.preferences
            .as_ref()
            .unwrap()
            .theme
            .update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }
}

pub fn run() {
    startup::start();
    let dir = platform::data_dir();
    let loaded = (|| -> io::Result<_> {
        let lock = StoreLock::acquire(&dir)?;
        let _span = startup::span("notebook.load");
        let book = Notebook::load(&dir)?;
        let settings = Settings::load(&dir)?;
        Ok((lock, book, settings))
    })()
    .map_err(|error| format!("메모 저장소를 열지 못했습니다. 파일을 덮어쓰지 않습니다.\n{error}"));
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
                    title: Some("Edot 메모".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            if let Err(error) = component::open_window(options, cx, |window, cx| {
                cx.new(|cx| Edot::new(dir, loaded, window, cx))
            }) {
                eprintln!("창을 열지 못했습니다: {error}");
                cx.quit();
            }
            if startup::finish() {
                cx.quit();
            }
        });
}
