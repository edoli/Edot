use super::*;
use crate::shortcuts::{ActiveShortcuts, Command};

impl Edot {
    pub(super) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.key_down_on(event, cfg!(target_os = "macos"), window, cx);
    }

    pub(super) fn key_down_on(
        &mut self,
        event: &KeyDownEvent,
        macos: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.startup_failed || window.has_active_dialog(cx) || self.keyboard_menu.is_some() {
            return;
        }
        let key = event.keystroke.key.as_str();
        if self.error.is_some() {
            if matches!(key, "escape" | "enter") {
                self.error = None;
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if self.space_ui.open {
            return; // The Space popup owns its keys, including name editing.
        }
        if event.prefer_character_input
            || event.keystroke.modifiers.alt
                && event
                    .keystroke
                    .key_char
                    .as_ref()
                    .is_some_and(|text| text != &event.keystroke.key)
            || [
                &self.title_input,
                &self.local.query,
                &self.local.replacement,
                &self.global.query,
                &self.global.replacement,
            ]
            .into_iter()
            .chain(self.active_editor().as_ref())
            .any(|input| {
                let input = input.read(cx);
                input.is_composing() && input.focus_handle(cx).is_focused(window)
            })
            || self.preferences.as_ref().is_some_and(|preferences| {
                [&preferences.theme, &preferences.font, &preferences.size]
                    .into_iter()
                    .any(|select| select.read(cx).is_composing(window, cx))
            })
        {
            return;
        }
        let command = cx
            .global::<ActiveShortcuts>()
            .command(&event.keystroke, macos);
        let modifiers = event.keystroke.modifiers;
        let ctrl = component::shortcuts::primary_on(modifiers, macos);
        let shift = modifiers.shift;
        if self.shortcut_help.is_some() || self.trash_browser.is_some() {
            return;
        }
        if event.is_held
            && command.is_some_and(|command| {
                !matches!(
                    command,
                    Command::ZoomIn
                        | Command::ZoomOut
                        | Command::NextTab
                        | Command::PreviousTab
                        | Command::NextMatch
                        | Command::PreviousMatch
                        | Command::Undo
                        | Command::Redo
                        | Command::Cut
                        | Command::Copy
                        | Command::Paste
                        | Command::SelectAll
                        | Command::NextOccurrence
                )
            })
        {
            cx.stop_propagation();
            return;
        }
        if command == Some(Command::Help) {
            if !event.is_held {
                self.show_shortcuts(window, cx);
            }
            cx.stop_propagation();
            return;
        }
        let tree = self.tree_focus.is_focused(window);
        let results = self.results_focus.is_focused(window);
        let title = self.rename.is_some()
            && self
                .title_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window);
        let mut handled = true;
        if (key == "f10"
            && modifiers
                == Modifiers {
                    shift: true,
                    ..Modifiers::default()
                }
            || matches!(key, "menu" | "apps") && modifiers == Modifiers::default())
            && !title
            && !self.settings_visible
        {
            self.show_keyboard_menu(window, cx);
        } else if title && matches!(key, "escape" | "enter" | "tab") {
            self.finish_rename(key != "escape", true, window, cx);
        } else if command == Some(Command::Wrap) {
            let mut next = cx.global::<AppState>().settings.clone();
            next.wrap = !next.wrap;
            self.change_settings(next, window, cx);
        } else if command == Some(Command::NewFolder) {
            self.create_folder(None, window, cx);
        } else if command == Some(Command::NewNote) {
            self.create_note(None, window, cx);
        } else if command == Some(Command::Save) {
            self.finish_rename(true, false, window, cx);
            self.save_all_explicit(cx);
        } else if command == Some(Command::CloseTab) || macos && ctrl && shift && key == "w" {
            if macos && (ctrl && shift && key == "w" || self.open.is_empty()) {
                self.close_window(window, cx);
                return;
            }
            if let Some(id) = self.session.active {
                self.close_tabs(id, crate::tabs::CloseMode::Current, window, cx);
            }
        } else if command == Some(Command::ReopenTab) {
            self.reopen_closed_tab(window, cx);
        } else if command == Some(Command::Sidebar) {
            if !event.is_held {
                self.session.sidebar_visible = !self.session.sidebar_visible;
            }
        } else if command == Some(Command::ReadOnly) {
            if !event.is_held
                && self.active_editor().is_some()
                && self.set_read_only(!cx.global::<AppState>().settings.read_only, window, cx)
            {
                self.focus_editor(window, cx);
            }
        } else if command == Some(Command::Settings) {
            self.show_settings(window, cx);
        } else if matches!(
            command,
            Some(Command::Find | Command::Replace | Command::GlobalFind | Command::GlobalReplace)
        ) {
            self.show_find(
                matches!(command, Some(Command::GlobalFind | Command::GlobalReplace)),
                matches!(command, Some(Command::Replace | Command::GlobalReplace)),
                window,
                cx,
            );
        } else if matches!(command, Some(Command::NextTab | Command::PreviousTab)) {
            if !self.open.is_empty() {
                let index = self
                    .open
                    .iter()
                    .position(|tab| Some(tab.id) == self.session.active)
                    .unwrap_or(0);
                let previous = command == Some(Command::PreviousTab);
                let index = if previous {
                    (index + self.open.len() - 1) % self.open.len()
                } else {
                    (index + 1) % self.open.len()
                };
                self.open_note(self.open[index].id, true, window, cx);
            }
        } else if matches!(
            command,
            Some(
                Command::Tab1
                    | Command::Tab2
                    | Command::Tab3
                    | Command::Tab4
                    | Command::Tab5
                    | Command::Tab6
                    | Command::Tab7
                    | Command::Tab8
                    | Command::LastTab
            )
        ) {
            let index = match command.unwrap() {
                Command::Tab1 => 0,
                Command::Tab2 => 1,
                Command::Tab3 => 2,
                Command::Tab4 => 3,
                Command::Tab5 => 4,
                Command::Tab6 => 5,
                Command::Tab7 => 6,
                Command::Tab8 => 7,
                _ => self.open.len().saturating_sub(1),
            };
            if let Some(tab) = self.open.get(index) {
                self.open_note(tab.id, true, window, cx);
            }
        } else if command == Some(Command::Rename) {
            let id = if self.session.sidebar_visible {
                self.caret
            } else {
                self.session.active
            };
            if let Some(id) = id {
                self.begin_rename(id, window, cx);
            }
        } else if matches!(command, Some(Command::NextMatch | Command::PreviousMatch)) {
            let global_focus = self
                .global
                .query
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
                || self
                    .global
                    .replacement
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
                || results;
            let global = self.search_mode && (!self.find_visible || global_focus);
            self.navigate(global, command == Some(Command::PreviousMatch), window, cx);
        } else if matches!(command, Some(Command::DateTime | Command::Date))
            && !self.settings_visible
        {
            let editor_focus = self
                .active_editor()
                .is_some_and(|editor| editor.read(cx).focus_handle(cx).is_focused(window));
            if title || tree || editor_focus || self.focus.is_focused(window) {
                match datetime::current(command == Some(Command::DateTime)) {
                    Ok(text) if title => self
                        .title_input
                        .update(cx, |state, cx| state.replace(text, window, cx)),
                    Ok(text) => {
                        if let Some(editor) = self.active_editor() {
                            if editor.read(cx).is_read_only() {
                                cx.stop_propagation();
                                return;
                            }
                            editor.update(cx, |state, cx| {
                                state.replace(text, window, cx);
                                state.focus(window, cx);
                            });
                            self.changed(self.session.active.unwrap(), window, cx);
                        }
                    }
                    Err(error) => self.fail("날짜를 삽입하지 못했습니다", error, cx),
                }
            } else {
                handled = false;
            }
        } else if !self.settings_visible
            && matches!(
                command,
                Some(Command::ZoomIn | Command::ZoomOut | Command::ZoomReset)
            )
        {
            self.zoom = match command.unwrap() {
                Command::ZoomIn => (self.zoom + ZOOM_STEP).min(ZOOM_MAX),
                Command::ZoomOut => self.zoom.saturating_sub(ZOOM_STEP).max(ZOOM_MIN),
                _ => ZOOM_DEFAULT,
            };
        } else if key == "escape" {
            if cx.stop_active_drag(window) {
                self.stop_drag();
                cx.stop_propagation();
                cx.notify();
                return;
            }
            if let Some(editor) = self.active_editor()
                && editor.read(cx).focus_handle(cx).is_focused(window)
                && editor.update(cx, |s, cx| s.cancel_extra_selections(cx))
            {
                cx.stop_propagation();
                return;
            }
            if self.error.is_some() {
                self.error = None;
            } else if self.settings_visible {
                // Select handles its own first Escape while its popup is open.
                handled = false;
            } else if self.find_visible {
                self.find_visible = false;
                self.focus_editor(window, cx);
            } else if self.search_mode {
                self.focus_editor(window, cx);
            }
            self.stop_drag();
            self.resizing = None;
        } else if tree && command == Some(Command::SelectAll) {
            self.selection.all(
                &self
                    .visible_tree()
                    .iter()
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>(),
            );
        } else if tree {
            handled = self.tree_key(key, ctrl, shift, window, cx);
        } else if results {
            handled = self.result_key(key, window, cx);
        } else {
            handled = false;
        }
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub(super) fn tree_key(
        &mut self,
        key: &str,
        ctrl: bool,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let visible: Vec<_> = self.visible_tree().iter().map(|(id, _)| *id).collect();
        if ctrl && key == "space" {
            if let Some(id) = self.caret {
                self.selection.click(id, &visible, true, false);
            }
            return true;
        }
        if key == "delete" {
            self.confirm_delete(window, cx);
            return true;
        }
        if key == "enter" {
            if let Some(id) = self.caret {
                if self.book.is_folder(id) {
                    self.set_collapsed(id, !self.collapsed.contains(&id), cx);
                } else {
                    self.open_note(id, true, window, cx);
                }
            }
            return true;
        }
        let index = self
            .caret
            .and_then(|id| visible.iter().position(|row| *row == id))
            .unwrap_or(0);
        let next = match key {
            "up" => visible.get(index.saturating_sub(1)).copied(),
            "down" => visible
                .get((index + 1).min(visible.len().saturating_sub(1)))
                .copied(),
            "home" => visible.first().copied(),
            "end" => visible.last().copied(),
            "left" => self.caret.and_then(|id| {
                if self.book.is_folder(id) && !self.collapsed.contains(&id) {
                    self.set_collapsed(id, true, cx);
                    None
                } else {
                    self.book
                        .get(id)
                        .map(|note| note.parent)
                        .filter(|id| *id != 0)
                }
            }),
            "right" => self.caret.and_then(|id| {
                if self.collapsed.contains(&id) {
                    self.set_collapsed(id, false, cx);
                    None
                } else {
                    self.book.children(id).first().copied()
                }
            }),
            _ => return false,
        };
        if let Some(id) = next {
            self.caret = Some(id);
            if shift || !ctrl {
                self.selection.click(id, &visible, ctrl, shift);
            }
            if !ctrl && !shift {
                self.open_tree_note(id, window, cx);
            }
            self.reveal_tree(id);
        }
        true
    }
    pub(super) fn result_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.search_results.visible.is_empty() {
            return false;
        }
        let index = self.search_results.cursor.unwrap_or(0);
        let next = match key {
            "up" => index.saturating_sub(1),
            "down" => (index + 1).min(self.search_results.visible.len() - 1),
            "home" => 0,
            "end" => self.search_results.visible.len() - 1,
            "enter" | "space" => {
                self.activate_result(index, window, cx);
                return true;
            }
            "left" | "right" => {
                let row = self.search_results.visible[index];
                let id = self.search_results.groups[row.group].id;
                if key == "left" {
                    self.search_results.collapsed.insert(id);
                } else {
                    self.search_results.collapsed.remove(&id);
                }
                self.rebuild_results();
                self.search_results.cursor = self
                    .search_results
                    .visible
                    .iter()
                    .position(|r| r.group == row.group);
                return true;
            }
            _ => return false,
        };
        self.search_results.cursor = Some(next);
        self.search_results
            .scroll
            .scroll_to_item(next, ScrollStrategy::Nearest);
        let row = self.search_results.visible[next];
        if let Some(hit) = row.hit {
            self.open_match(
                MatchKey {
                    id: self.search_results.groups[row.group].id,
                    range: self.search_results.groups[row.group].matches[hit].range,
                },
                window,
                cx,
            );
        }
        true
    }
}
