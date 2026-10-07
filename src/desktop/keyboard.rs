use super::*;

impl Edot {
    pub(super) fn key_down(
        &mut self,
        event: &KeyDownEvent,
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
        let ctrl = event.keystroke.modifiers.control;
        let shift = event.keystroke.modifiers.shift;
        let tree = self.tree_focus.is_focused(window);
        let results = self.results_focus.is_focused(window);
        let title = self.rename.is_some()
            && self
                .title_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window);
        let mut handled = true;
        if (key == "f10" && shift || key == "menu" || key == "apps")
            && !title
            && !self.settings_visible
        {
            self.show_keyboard_menu(window, cx);
        } else if title && matches!(key, "escape" | "enter" | "tab") {
            self.finish_rename(key != "escape", true, window, cx);
        } else if ctrl && key == "n" {
            self.create_note(None, window, cx);
        } else if ctrl && key == "s" {
            self.finish_rename(true, false, window, cx);
            self.save_all(cx);
        } else if ctrl && key == "w" {
            if let Some(id) = self.session.active {
                self.close_tabs(id, 0, window, cx);
            }
        } else if ctrl && key == "b" {
            self.session.sidebar_visible = !self.session.sidebar_visible;
        } else if ctrl && key == "," {
            self.show_settings(window, cx);
        } else if ctrl && matches!(key, "f" | "h") {
            self.show_find(shift, key == "h", window, cx);
        } else if ctrl && (key == "tab" || key == "pagedown" || key == "pageup") {
            if !self.open.is_empty() {
                let index = self
                    .open
                    .iter()
                    .position(|tab| Some(tab.id) == self.session.active)
                    .unwrap_or(0);
                let previous = key == "pageup" || key == "tab" && shift;
                let index = if previous {
                    (index + self.open.len() - 1) % self.open.len()
                } else {
                    (index + 1) % self.open.len()
                };
                self.open_note(self.open[index].id, true, window, cx);
            }
        } else if ctrl && matches!(key, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9") {
            let index = if key == "9" {
                self.open.len().saturating_sub(1)
            } else {
                key.parse::<usize>().unwrap() - 1
            };
            if let Some(tab) = self.open.get(index) {
                self.open_note(tab.id, true, window, cx);
            }
        } else if key == "f2" {
            let id = if self.session.sidebar_visible {
                self.caret
            } else {
                self.session.active
            };
            if let Some(id) = id {
                self.begin_rename(id, window, cx);
            }
        } else if key == "f3" {
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
            self.navigate(global, shift, window, cx);
        } else if !ctrl && matches!(key, "f5" | "f6") && !self.settings_visible {
            let editor_focus = self
                .active_editor()
                .is_some_and(|editor| editor.read(cx).focus_handle(cx).is_focused(window));
            if title || tree || editor_focus || self.focus.is_focused(window) {
                match datetime::current(key == "f5") {
                    Ok(text) if title => self
                        .title_input
                        .update(cx, |state, cx| state.replace(text, window, cx)),
                    Ok(text) => {
                        if let Some(editor) = self.active_editor() {
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
        } else if ctrl
            && !self.settings_visible
            && matches!(key, "+" | "=" | "-" | "0" | "add" | "subtract")
        {
            self.zoom = match key {
                "+" | "=" | "add" => (self.zoom + 10).min(500),
                "-" | "subtract" => self.zoom.saturating_sub(10).max(10),
                _ => 100,
            };
        } else if key == "escape" {
            if cx.stop_active_drag(window) {
                self.stop_drag();
                cx.stop_propagation();
                cx.notify();
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
        if ctrl && key == "a" {
            self.selection.all(&visible);
            return true;
        }
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
                self.open_note(id, true, window, cx);
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
                if !self.book.children(id).is_empty() && !self.collapsed.contains(&id) {
                    self.collapsed.insert(id);
                    None
                } else {
                    self.book
                        .get(id)
                        .map(|note| note.parent)
                        .filter(|id| *id != 0)
                }
            }),
            "right" => self.caret.and_then(|id| {
                if self.collapsed.remove(&id) {
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
                self.open_note(id, false, window, cx);
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
        if self.result_rows.is_empty() {
            return false;
        }
        let index = self.result_cursor.unwrap_or(0);
        let next = match key {
            "up" => index.saturating_sub(1),
            "down" => (index + 1).min(self.result_rows.len() - 1),
            "home" => 0,
            "end" => self.result_rows.len() - 1,
            "enter" | "space" => {
                self.activate_result(index, window, cx);
                return true;
            }
            "left" | "right" => {
                let row = self.result_rows[index];
                let id = self.rows[row.group].id;
                if key == "left" {
                    self.result_collapsed.insert(id);
                } else {
                    self.result_collapsed.remove(&id);
                }
                self.rebuild_results();
                self.result_cursor = self.result_rows.iter().position(|r| r.group == row.group);
                return true;
            }
            _ => return false,
        };
        self.result_cursor = Some(next);
        self.result_scroll
            .scroll_to_item(next, ScrollStrategy::Nearest);
        let row = self.result_rows[next];
        if let Some(hit) = row.hit {
            self.open_match(
                MatchKey {
                    id: self.rows[row.group].id,
                    range: self.rows[row.group].matches[hit].range,
                },
                window,
                cx,
            );
        }
        true
    }
}
