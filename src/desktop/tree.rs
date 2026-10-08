//! Note creation, selection, renaming and persisted tree changes.
use super::*;

impl Edot {
    pub(super) fn set_collapsed(&mut self, id: u64, collapsed: bool, cx: &mut Context<Self>) {
        let was_collapsed = self.collapsed.contains(&id);
        if was_collapsed == collapsed {
            return;
        }
        if collapsed {
            self.collapsed.insert(id);
        } else {
            self.collapsed.remove(&id);
        }
        let previous_session = self.session.clone();
        if let Err(error) = self.save_session() {
            if was_collapsed {
                self.collapsed.insert(id);
            } else {
                self.collapsed.remove(&id);
            }
            self.session = previous_session;
            self.fail("트리 상태를 저장하지 못했습니다", error, cx);
        }
        cx.notify();
    }

    pub(super) fn create_note(
        &mut self,
        child: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        if let Err(error) = self.save_tree() {
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
    pub(super) fn begin_rename(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn finish_rename(
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
            if let Err(error) = self.save_tree() {
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
    pub(super) fn visible_tree(&self) -> Vec<(u64, usize)> {
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
    pub(super) fn reveal_tree(&self, id: u64) {
        if let Some(index) = self.visible_tree().iter().position(|(row, _)| *row == id) {
            self.tree_scroll
                .scroll_to_item(index, ScrollStrategy::Nearest);
        }
    }
    pub(super) fn tree_click(
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
    pub(super) fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn delete_confirmed(
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
        self.file_sync.tree_revision = self.file_sync.tree_revision.wrapping_add(1);
        let active_index = self
            .open
            .iter()
            .position(|tab| Some(tab.id) == self.session.active)
            .unwrap_or(0);
        self.open.retain(|tab| !ids.contains(&tab.id));
        self.closed_tabs.retain(|tab| !ids.contains(&tab.id));
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
    pub(super) fn group_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        if let Err(error) = self.save_tree() {
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
    pub(super) fn move_notes(&mut self, ids: &[u64], target: DropTarget, cx: &mut Context<Self>) {
        let previous = self.book.clone();
        if self.book.move_notes(ids, target.parent, target.before) {
            if let Err(error) = self.save_tree() {
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
    pub(super) fn outdent(&mut self, id: u64, cx: &mut Context<Self>) {
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
}
