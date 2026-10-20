//! Immediate directory event reads and app-first reconciliation.
use super::*;
use crate::watch::{Changes, Watcher};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

pub(super) struct FileSync {
    watcher: Option<Watcher>,
    listen: Option<Task<()>>,
    refresh: Option<Task<()>>,
    pending: Changes,
    retry: u32,
    waiting_retry: bool,
    epoch: u64,
    pub(super) tree_revision: u64,
    pub(super) missing: HashSet<NoteId>,
}

impl Default for FileSync {
    fn default() -> Self {
        static EPOCH: AtomicU64 = AtomicU64::new(0);
        Self {
            watcher: None,
            listen: None,
            refresh: None,
            pending: Changes::default(),
            retry: 0,
            waiting_retry: false,
            epoch: EPOCH.fetch_add(1, Ordering::Relaxed),
            tree_revision: 0,
            missing: HashSet::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    entity: EntityId,
    revision: u64,
    saved_revision: u64,
}

impl Drop for FileSync {
    fn drop(&mut self) {
        self.refresh = None;
        self.listen = None;
        self.watcher = None;
    }
}

impl Stamp {
    fn of(tab: &OpenTab, cx: &App) -> Option<Self> {
        let editor = tab.editor.as_ref()?;
        Some(Self {
            entity: editor.entity_id(),
            revision: editor.read(cx).revision,
            saved_revision: tab.saved_revision,
        })
    }
}

pub(super) struct Snapshot {
    dir: PathBuf,
    epoch: u64,
    tree_revision: u64,
    changes: Changes,
    bodies: Vec<(NoteId, Stamp)>,
}

pub(super) struct ReadResult {
    snapshot: Snapshot,
    tree: Option<io::Result<Notebook>>,
    bodies: Vec<(NoteId, Stamp, io::Result<String>)>,
}

impl Snapshot {
    pub(super) fn read(self) -> ReadResult {
        let tree = (self.changes.tree || self.changes.rescan).then(|| Notebook::reload(&self.dir));
        let bodies = self
            .bodies
            .iter()
            .map(|&(id, stamp)| {
                (
                    id,
                    stamp,
                    fs::read_to_string(self.dir.join("notes").join(format!("{id}.txt"))),
                )
            })
            .collect();
        ReadResult {
            snapshot: self,
            tree,
            bodies,
        }
    }
}

impl Edot {
    pub(super) fn save_tree(&mut self) -> io::Result<()> {
        self.book.save(&self.dir)?;
        self.file_sync.tree_revision = self.file_sync.tree_revision.wrapping_add(1);
        Ok(())
    }

    #[cfg_attr(test, allow(dead_code))] // GPUI test scheduling disallows OS-thread wakes.
    pub(super) fn start_file_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.dir.clone();
        self.file_sync.listen = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                match Watcher::new(&dir) {
                    Ok((watcher, events)) => {
                        if this
                            .update(cx, |this, _| this.file_sync.watcher = Some(watcher))
                            .is_err()
                        {
                            return;
                        }
                        while let Some(result) = events.next().await {
                            if this
                                .update_in(cx, |this, window, cx| match result {
                                    Ok(changes) => this.request_file_sync(changes, window, cx),
                                    Err(error) => {
                                        this.status = format!("파일 감시 재시작 중 · {error}");
                                        this.request_file_sync(Changes::all(), window, cx);
                                    }
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                        if this
                            .update(cx, |this, _| this.file_sync.watcher = None)
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        if this
                            .update(cx, |this, cx| {
                                this.status = format!("파일 감시 재시작 중 · {error}");
                                cx.notify();
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                cx.background_executor().timer(Duration::from_secs(1)).await;
            }
        }));
    }

    pub(super) fn request_file_sync(
        &mut self,
        changes: Changes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.startup_failed || changes.is_empty() {
            return;
        }
        self.file_sync.pending.merge(changes);
        if self.file_sync.waiting_retry {
            // A fresh notification supersedes the backoff; pending retries are kept.
            self.file_sync.refresh = None;
            self.file_sync.waiting_retry = false;
        }
        self.file_sync.retry = 0;
        if self.file_sync.refresh.is_some() {
            return;
        }
        self.file_sync.refresh = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                let Ok(delay) = this.update(cx, |this, _| {
                    this.file_sync.waiting_retry = this.file_sync.retry > 0;
                    this.file_sync
                        .waiting_retry
                        .then(|| Duration::from_millis(200 * (1 + this.file_sync.retry as u64)))
                }) else {
                    return;
                };
                if let Some(delay) = delay {
                    cx.background_executor().timer(delay).await;
                }
                let Ok(snapshot) = this.update(cx, |this, cx| {
                    this.file_sync.waiting_retry = false;
                    let changes = std::mem::take(&mut this.file_sync.pending);
                    this.sync_snapshot(changes, cx)
                }) else {
                    return;
                };
                let read = cx
                    .background_executor()
                    .spawn(async move { snapshot.read() })
                    .await;
                let Ok(again) = this.update_in(cx, |this, window, cx| {
                    let fresh_changes = !this.file_sync.pending.is_empty();
                    let retry = this.apply_file_sync(read, window, cx);
                    if !retry.is_empty()
                        && (this.file_sync.retry < 5 || !this.dragging_notes.is_empty())
                    {
                        this.file_sync.pending.merge(retry);
                        if !fresh_changes {
                            this.file_sync.retry = (this.file_sync.retry + 1).min(5);
                        }
                    }
                    if this.file_sync.pending.is_empty() {
                        this.file_sync.refresh = None;
                        false
                    } else {
                        true
                    }
                }) else {
                    return;
                };
                if !again {
                    return;
                }
            }
        }));
    }

    pub(super) fn sync_snapshot(&self, changes: Changes, cx: &App) -> Snapshot {
        let bodies = self
            .open
            .iter()
            .filter_map(|tab| {
                if changes.rescan || changes.tree || changes.notes.contains(&tab.id) {
                    Some((tab.id, Stamp::of(tab, cx)?))
                } else {
                    None
                }
            })
            .collect();
        Snapshot {
            dir: self.dir.clone(),
            epoch: self.file_sync.epoch,
            tree_revision: self.file_sync.tree_revision,
            changes,
            bodies,
        }
    }

    pub(super) fn apply_file_sync(
        &mut self,
        read: ReadResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Changes {
        let ReadResult {
            snapshot,
            tree,
            bodies,
        } = read;
        if snapshot.epoch != self.file_sync.epoch || snapshot.dir != self.dir {
            return Changes::default();
        }
        let mut retry = Changes::default();
        let mut tree_changed = false;
        let mut updated = false;
        let mut error = None;
        if let Some(tree) = tree {
            if snapshot.tree_revision != self.file_sync.tree_revision
                || !self.dragging_notes.is_empty()
            {
                retry.tree = true;
            } else {
                match tree.and_then(|book| self.apply_synced_tree(book, window, cx)) {
                    Ok(changed) => tree_changed = changed,
                    Err(failure) => {
                        retry.tree = true;
                        error = Some(format!(
                            "목록 변경을 읽지 못했습니다 · 기존 목록 유지 · {failure}"
                        ));
                    }
                }
            }
        }
        for (id, stamp, text) in bodies {
            let Some(tab) = self.open.iter_mut().find(|tab| tab.id == id) else {
                continue;
            };
            if Stamp::of(tab, cx) != Some(stamp) {
                retry.notes.insert(id);
                continue;
            }
            let editor = tab.editor.as_ref().unwrap();
            match text {
                Ok(text) => {
                    self.file_sync.missing.remove(&id);
                    let local = editor.read(cx).value();
                    if editor.read(cx).is_composing() {
                        retry.notes.insert(id);
                        continue;
                    }
                    // Read the actual editor, since its Change event may still be queued.
                    tab.dirty = local.as_ref() != tab.saved;
                    if local.as_ref() == text {
                        tab.saved = text;
                        tab.saved_revision = tab.saved_revision.wrapping_add(1);
                        tab.dirty = false;
                    } else if !tab.dirty && text != tab.saved {
                        editor.update(cx, |state, cx| state.reload_from_disk(&text, cx));
                        tab.metrics = editing::TextMetrics::new(&text);
                        tab.metrics_revision = editor.read(cx).revision;
                        tab.saved = text;
                        tab.saved_revision = tab.saved_revision.wrapping_add(1);
                        updated = true;
                    }
                    // A dirty editor wins: keep its saved base and normal autosave.
                }
                Err(failure) => {
                    if failure.kind() == io::ErrorKind::NotFound {
                        self.file_sync.missing.insert(id);
                        error = Some(
                            "본문 파일이 삭제되었습니다 · 현재 내용 유지 · Ctrl+S로 복원".into(),
                        );
                    } else {
                        error = Some(format!(
                            "본문 변경을 읽지 못했습니다 · 현재 내용 유지 · {failure}"
                        ));
                    }
                    retry.notes.insert(id);
                }
            }
        }
        if self.search_mode
            && (tree_changed || snapshot.changes.rescan || snapshot.changes.notes.len() > 64)
        {
            self.queue_search(window, cx);
        } else if self.search_mode {
            for id in snapshot.changes.notes {
                self.queue_note_search(id, window, cx);
            }
        }
        if let Some(error) = error {
            self.status = error;
        } else if updated || tree_changed {
            self.local.last = None;
            self.status = if self.open.iter().any(|tab| tab.dirty) {
                "저장 중…"
            } else {
                "외부 변경 반영됨"
            }
            .into();
        }
        cx.notify();
        retry
    }

    fn apply_synced_tree(
        &mut self,
        mut book: Notebook,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<bool> {
        if book.notes == self.book.notes {
            return Ok(false);
        }
        if book.notes.iter().any(|note| {
            self.book
                .get(note.id)
                .is_some_and(|old| old.kind != note.kind)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "기존 항목의 유형은 변경할 수 없습니다.",
            ));
        }
        let mut valid: HashSet<_> = book.notes.iter().map(|note| note.id).collect();
        let mut root_order = book.notes.iter().filter(|note| note.parent == 0).count() as u32;
        let mut restored = false;
        let protected = self
            .open
            .iter()
            .filter(|tab| {
                tab.editor.as_ref().is_some_and(|editor| {
                    editor.read(cx).value().as_ref() != tab.saved || editor.read(cx).is_composing()
                })
            })
            .map(|tab| tab.id)
            .chain(self.rename.as_ref().map(|(id, _)| *id));
        for id in protected {
            if !valid.contains(&id)
                && let Some(note) = self.book.get(id)
            {
                let mut note = note.clone();
                note.parent = 0; // Avoid reintroducing deleted ancestors or cycles.
                note.order = root_order;
                root_order = root_order.wrapping_add(1);
                valid.insert(note.id);
                book.notes.push(note);
                restored = true;
            }
        }
        if restored {
            book.save(&self.dir)?;
            self.file_sync.tree_revision = self.file_sync.tree_revision.wrapping_add(1);
        }
        let old_folders = Session::folders(&self.book);
        let folders = Session::folders(&book);
        self.collapsed.retain(|id| folders.contains(id));
        self.collapsed
            .extend(folders.difference(&old_folders).copied());
        let editor_focused = self
            .active_editor()
            .is_some_and(|editor| editor.read(cx).focus_handle(cx).is_focused(window));
        let active_index = self
            .open
            .iter()
            .position(|tab| Some(tab.id) == self.session.active)
            .unwrap_or(0);
        self.open.retain(|tab| valid.contains(&tab.id));
        self.closed_tabs.retain(|tab| valid.contains(&tab.id));
        self.selection.retain(|id| valid.contains(&id));
        if self.context_note.is_some_and(|id| !valid.contains(&id)) {
            self.keyboard_menu = None;
            self.context_note = None;
            self.menu_subscription = None;
        }
        self.file_sync.missing.retain(|id| valid.contains(id));
        if self.caret.is_some_and(|id| !valid.contains(&id)) {
            self.caret = None;
        }
        self.book = book;
        self.session.tabs = self.open.iter().map(|tab| tab.id).collect();
        if self.session.active.is_some_and(|id| !valid.contains(&id)) {
            self.session.active = None;
            if let Some(tab) = self
                .open
                .get(active_index.min(self.open.len().saturating_sub(1)))
            {
                self.open_note(tab.id, false, window, cx);
            }
            if editor_focused {
                self.focus_editor(window, cx);
            }
        }
        Ok(true)
    }
}
