//! Open editor lifetime, persistence and tab operations.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct ClosedTab {
    pub(super) id: NoteId,
    index: usize,
}

pub(super) struct OpenTab {
    pub(super) id: NoteId,
    pub(super) editor: Option<Entity<TextareaState>>,
    pub(super) saved: String,
    pub(super) saved_revision: u64,
    pub(super) metrics: editing::TextMetrics,
    pub(super) metrics_revision: u64,
    pub(super) dirty: bool,
    pub(super) preview: bool,
    pub(super) _subscriptions: Vec<Subscription>,
}

impl Edot {
    pub(super) fn active_editor(&self) -> Option<Entity<TextareaState>> {
        self.open
            .iter()
            .find(|tab| Some(tab.id) == self.session.active)
            .and_then(|tab| tab.editor.clone())
    }
    pub(super) fn text(&self, id: NoteId, cx: &App) -> io::Result<String> {
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
    pub(super) fn focus_editor(&self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = self.active_editor() {
            editor.update(cx, |state, cx| state.focus(window, cx));
        } else {
            self.tree_focus.focus(window, cx);
        }
    }
    fn keep_tab(&mut self, id: NoteId, cx: &mut Context<Self>) {
        if let Some(tab) = self.open.iter_mut().find(|tab| tab.id == id)
            && tab.preview
        {
            tab.preview = false;
            cx.notify();
        }
    }
    pub(super) fn open_tree_note(
        &mut self,
        id: NoteId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Focus callbacks may still be queued when the next tree click arrives.
        // Protect edits and an in-progress IME composition even without focus.
        for tab in &mut self.open {
            if tab.preview
                && tab.editor.as_ref().is_some_and(|editor| {
                    let state = editor.read(cx);
                    state.focus_handle(cx).is_focused(window)
                        || state.is_composing()
                        || state.value().as_ref() != tab.saved
                })
            {
                tab.preview = false;
            }
        }
        let existing = self.open.iter().find(|tab| tab.id == id);
        let new_tab = existing.is_none();
        let preview = existing.is_none_or(|tab| tab.preview);
        let previous = self.open.iter().position(|tab| tab.preview && tab.id != id);
        // Read and activate the target before removing the previous preview.
        // A read or rename failure leaves the old editor available.
        if !self.open_note(id, false, window, cx) {
            return false;
        }
        self.open
            .iter_mut()
            .find(|tab| tab.id == id)
            .unwrap()
            .preview = preview;
        if let Some(index) = previous {
            self.open.remove(index);
            if new_tab {
                let tab = self.open.pop().unwrap();
                self.open.insert(index, tab);
            }
        }
        if let Some(index) = self.open.iter().position(|tab| tab.id == id) {
            self.tab_scroll.scroll_to_item(index);
        }
        cx.notify();
        true
    }
    pub(super) fn open_note(
        &mut self,
        id: NoteId,
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
                    self.fail("노트를 열지 못했습니다", error, cx);
                    return false;
                }
            };
            let editor = cx.new(|cx| {
                let mut state = TextareaState::new(window, cx)
                    .content_padding(Edges {
                        left: px(16.),
                        top: px(6.),
                        right: px(4.),
                        bottom: px(0.),
                    })
                    .soft_wrap(cx.global::<AppState>().settings.wrap)
                    .source_directory(self.dir.clone())
                    .default_value(text.clone());
                state.set_read_only(cx.global::<AppState>().settings.read_only, cx);
                state
            });
            let subscription =
                cx.subscribe_in(&editor, window, move |this, editor, event, window, cx| {
                    match event {
                        InputEvent::Change => this.changed(id, window, cx),
                        InputEvent::DropImages { paths, position } => {
                            this.drop_images(editor.clone(), paths, *position, window, cx);
                        }
                        InputEvent::PasteImages { item, insertion } => {
                            this.paste_images(editor.clone(), item, insertion.clone(), window, cx);
                        }
                        _ => {}
                    }
                    cx.notify();
                });
            let observe = cx.observe(&editor, |_, _, cx| cx.notify());
            let editor_focus = editor.read(cx).focus_handle(cx);
            let focus_subscription = cx.on_focus(&editor_focus, window, move |this, _, cx| {
                this.keep_tab(id, cx);
            });
            let tab = OpenTab {
                id,
                editor: Some(editor),
                metrics: editing::TextMetrics::new(&text),
                metrics_revision: 0,
                saved: text,
                saved_revision: 0,
                dirty: false,
                preview: false,
                _subscriptions: vec![subscription, observe, focus_subscription],
            };
            if let Some(index) = existing {
                self.open[index] = tab;
            } else {
                self.open.push(tab);
            }
        }
        self.keep_tab(id, cx);
        self.closed_tabs.retain(|tab| tab.id != id);
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
    pub(super) fn changed(&mut self, id: NoteId, window: &mut Window, cx: &mut Context<Self>) {
        self.keep_tab(id, cx);
        if let Some(tab) = self.open.iter_mut().find(|tab| tab.id == id)
            && let Some(editor) = &tab.editor
        {
            let text = editor.read(cx).value();
            tab.dirty = text.as_ref() != tab.saved;
            tab.metrics = editing::TextMetrics::new(&text);
            tab.metrics_revision = editor.read(cx).revision;
            if !tab.dirty {
                cx.notify();
                return;
            }
        }
        self.status = "저장 중…".into();
        self.autosave = Some(cx.spawn(async |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                this.save_all(cx);
            });
        }));
        if self.search_mode {
            self.queue_note_search(id, window, cx);
        }
        cx.notify();
    }
    pub(super) fn save_all(&mut self, cx: &mut Context<Self>) -> bool {
        self.save_all_inner(false, cx)
    }

    pub(super) fn save_all_explicit(&mut self, cx: &mut Context<Self>) -> bool {
        self.save_all_inner(true, cx)
    }

    fn save_all_inner(&mut self, restore_missing: bool, cx: &mut Context<Self>) -> bool {
        if self.startup_failed {
            return false;
        }
        for tab in &mut self.open {
            if !(tab.dirty || restore_missing && self.file_sync.missing.contains(&tab.id)) {
                continue;
            }
            let Some(editor) = &tab.editor else {
                continue;
            };
            let text = editor.read(cx).value().to_string();
            let path = self.book.file(&self.dir, tab.id);
            if !restore_missing && !path.exists() {
                self.status =
                    "본문 파일이 삭제되었습니다 · Ctrl+S로 앱 내용을 복원할 수 있습니다".into();
                cx.notify();
                return false;
            }
            if let Err(error) = replace_file(&path, text.as_bytes()) {
                self.fail(
                    "노트를 저장하지 못했습니다. 수정 내용은 편집기에 남아 있습니다",
                    error,
                    cx,
                );
                return false;
            }
            tab.saved = text;
            tab.saved_revision = tab.saved_revision.wrapping_add(1);
            tab.dirty = false;
            self.file_sync.missing.remove(&tab.id);
        }
        self.status = "저장됨".into();
        cx.notify();
        true
    }
    pub(super) fn save_session(&mut self) -> io::Result<()> {
        self.session.tabs = self.open.iter().map(|tab| tab.id).collect();
        self.session.expanded = Session::folders(&self.book)
            .difference(&self.collapsed)
            .copied()
            .collect();
        self.session.save(&self.dir)
    }
    pub(super) fn close_tabs(
        &mut self,
        id: NoteId,
        mode: CloseMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.finish_rename(true, false, window, cx) || !self.save_all(cx) {
            return;
        }
        let Some(index) = self.open.iter().position(|tab| tab.id == id) else {
            return;
        };
        let mut cursor = 0;
        let mut kept = 0;
        self.open.retain(|tab| {
            let keep = match mode {
                CloseMode::Others => cursor == index,
                CloseMode::ToRight => cursor <= index,
                CloseMode::Current => cursor != index,
            };
            cursor += 1;
            if keep {
                kept += 1;
            } else {
                // Position after earlier removals, so undoing a batch in reverse
                // order restores the original tab order.
                self.closed_tabs.push(ClosedTab {
                    id: tab.id,
                    index: kept,
                });
            }
            keep
        });
        let excess = self
            .closed_tabs
            .len()
            .saturating_sub(CLOSED_TAB_HISTORY_LIMIT);
        self.closed_tabs.drain(..excess);
        if self
            .session
            .active
            .is_some_and(|id| !self.open.iter().any(|tab| tab.id == id))
        {
            self.session.active = None;
            if let Some(tab) = self.open.get(index.min(self.open.len().saturating_sub(1))) {
                self.open_note(tab.id, true, window, cx);
            } else {
                self.focus.focus(window, cx);
            }
        }
        cx.notify();
    }
    pub(super) fn reopen_closed_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(closed) = self.closed_tabs.last().copied() {
            if self.book.get(closed.id).is_none() || self.open.iter().any(|tab| tab.id == closed.id)
            {
                self.closed_tabs.pop();
                continue;
            }
            // Failed reads/renames keep the record available for a later retry.
            if !self.open_note(closed.id, true, window, cx) {
                return;
            }
            // open_note appends new tabs and removes their closed history entry.
            let tab = self.open.pop().unwrap();
            let index = closed.index.min(self.open.len());
            self.open.insert(index, tab);
            self.tab_scroll.scroll_to_item(index);
            cx.notify();
            return;
        }
    }
    pub(super) fn reorder_tab(&mut self, id: NoteId, gap: usize, cx: &mut Context<Self>) {
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
}
