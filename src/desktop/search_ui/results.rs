use super::*;

impl Edot {
    pub(in crate::desktop) fn result_identity(
        &self,
        index: usize,
    ) -> Option<(NoteId, Option<CharRange>)> {
        let row = self.search_results.visible.get(index)?;
        let group = &self.search_results.groups[row.group];
        Some((group.id, row.hit.map(|hit| group.matches[hit].range)))
    }
    pub(in crate::desktop) fn merge_note_results(
        &mut self,
        id: NoteId,
        mut row: Option<ResultRow>,
    ) {
        let selected = self
            .search_results
            .cursor
            .and_then(|i| self.result_identity(i));
        let scroll = self.search_results.scroll.0.borrow().base_handle.clone();
        let offset = scroll.offset();
        let top = (f32::from(-offset.y) / SEARCH_ROW_HEIGHT).floor().max(0.) as usize;
        let anchor = self.result_identity(top);
        let other_count: usize = self
            .search_results
            .groups
            .iter()
            .filter(|r| r.id != id)
            .map(|r| r.matches.len())
            .sum();
        if let Some(row) = &mut row {
            row.matches
                .truncate(global_search::MAX_RESULTS.saturating_sub(other_count));
        }
        if let Some(index) = self.search_results.groups.iter().position(|r| r.id == id) {
            if let Some(row) = row {
                self.search_results.groups[index] = row;
            } else {
                self.search_results.groups.remove(index);
            }
        } else if let Some(row) = row {
            let order = self
                .book
                .notes
                .iter()
                .position(|n| n.id == id)
                .unwrap_or(usize::MAX);
            let index = self.search_results.groups.partition_point(|r| {
                self.book
                    .notes
                    .iter()
                    .position(|n| n.id == r.id)
                    .unwrap_or(usize::MAX)
                    < order
            });
            self.search_results.groups.insert(index, row);
        }
        self.rebuild_results();
        self.search_results.cursor = selected.and_then(|key| {
            (0..self.search_results.visible.len()).find(|&i| self.result_identity(i) == Some(key))
        });
        if let Some(key) = anchor
            && let Some(index) = (0..self.search_results.visible.len())
                .find(|&i| self.result_identity(i) == Some(key))
        {
            scroll.set_offset(point(
                offset.x,
                offset.y - px((index as f32 - top as f32) * SEARCH_ROW_HEIGHT),
            ));
        }
        if self.global.last.is_some_and(|key| {
            key.id == id
                && !self
                    .search_results
                    .groups
                    .iter()
                    .any(|r| r.id == id && r.matches.iter().any(|m| m.range == key.range))
        }) {
            self.global.last = None;
        }
        self.update_search_message();
    }
    pub(in crate::desktop) fn rebuild_results(&mut self) {
        self.search_results.visible = global_search::visible_rows(
            &self.search_results.groups,
            &self.search_results.collapsed,
        );
        self.search_results.cursor = self
            .search_results
            .cursor
            .filter(|index| *index < self.search_results.visible.len());
    }
    pub(in crate::desktop) fn open_match(
        &mut self,
        key: MatchKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pattern = match self.global.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            _ => return,
        };
        let text = match self.text(key.id, cx) {
            Ok(text) => text,
            Err(error) => {
                self.fail("검색 결과를 열지 못했습니다", error, cx);
                return;
            }
        };
        let mut valid = false;
        pattern.visit_text(&text, |range| {
            if range == key.range {
                valid = true;
                return false;
            }
            range.start <= key.range.start
        });
        if !valid {
            self.global.message = "본문이 변경되었습니다. 결과를 새로 고칩니다".into();
            self.queue_note_search(key.id, window, cx);
            return;
        }
        if self.open_note(key.id, false, window, cx) {
            if let Some(editor) = self.active_editor() {
                editor.update(cx, |state, cx| {
                    state.set_selected_range(editing::byte_range(&text, key.range), cx)
                });
            }
            self.global.last = Some(key);
            cx.notify();
        }
    }
    pub(in crate::desktop) fn activate_result(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.search_results.visible.get(index).copied() else {
            return;
        };
        self.search_results.cursor = Some(index);
        let group = &self.search_results.groups[row.group];
        if let Some(hit) = row.hit {
            self.open_match(
                MatchKey {
                    id: group.id,
                    range: group.matches[hit].range,
                },
                window,
                cx,
            );
        } else {
            let id = group.id;
            if !self.search_results.collapsed.insert(id) {
                self.search_results.collapsed.remove(&id);
            }
            self.rebuild_results();
        }
        self.results_focus.focus(window, cx);
        cx.notify();
    }
    pub(in crate::desktop) fn global_next(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search_results.scanning {
            self.search_results.pending_navigation = Some(backwards);
            return;
        }
        let pattern = match self.global.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            _ => return,
        };
        let total: usize = self.search_results.groups.iter().map(|row| row.count).sum();
        let displayed: usize = self
            .search_results
            .groups
            .iter()
            .map(|row| row.matches.len())
            .sum();
        if displayed == total {
            let keys: Vec<_> = self
                .search_results
                .groups
                .iter()
                .flat_map(|row| {
                    row.matches.iter().map(|hit| MatchKey {
                        id: row.id,
                        range: hit.range,
                    })
                })
                .collect();
            if keys.is_empty() {
                return;
            }
            let current = self
                .global
                .last
                .and_then(|key| keys.iter().position(|hit| *hit == key));
            let index = match (current, backwards) {
                (Some(index), true) => (index + keys.len() - 1) % keys.len(),
                (Some(index), false) => (index + 1) % keys.len(),
                (None, true) => keys.len() - 1,
                (None, false) => 0,
            };
            self.select_global_match(keys[index], window, cx);
            return;
        }
        let current = self.global.last;
        let generation = self.search_results.generation;
        let documents: Vec<_> = self
            .book
            .notes
            .iter()
            .filter(|note| !note.is_folder())
            .map(|note| {
                let loaded = self
                    .open
                    .iter()
                    .find(|tab| tab.id == note.id)
                    .and_then(|tab| tab.editor.as_ref())
                    .map(|editor| editor.read(cx).value().to_string());
                (note.id, self.book.file(&self.dir, note.id), loaded)
            })
            .collect();
        self.search_results.scanning = true;
        self.search_results.scan_task = Some(cx.spawn_in(window, async move |this, cx| {
            let key = cx
                .background_executor()
                .spawn(async move {
                    // Navigation uses all matches, including those beyond the preview limit.
                    let mut first = None;
                    let mut last = None;
                    let mut next = None;
                    let mut previous = None;
                    let mut passed = current.is_none();
                    let mut seen_current = false;
                    for (id, path, loaded) in documents {
                        let Ok(text) = loaded.map(Ok).unwrap_or_else(|| fs::read_to_string(path))
                        else {
                            continue;
                        };
                        pattern.visit_text(&text, |range| {
                            let key = MatchKey { id, range };
                            first.get_or_insert(key);
                            if current == Some(key) {
                                previous = last;
                                passed = true;
                                seen_current = true;
                            } else if passed && next.is_none() {
                                next = Some(key);
                            }
                            last = Some(key);
                            true
                        });
                    }
                    if backwards {
                        if seen_current {
                            previous.or(last)
                        } else {
                            last
                        }
                    } else {
                        next.or(first)
                    }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.search_results.generation != generation {
                    return;
                }
                this.search_results.scanning = false;
                this.refresh_pending_notes(window, cx);
                if let Some(key) = key {
                    this.select_global_match(key, window, cx);
                }
                if let Some(backwards) = this.search_results.pending_navigation.take() {
                    this.global_next(backwards, window, cx);
                }
                cx.notify();
            });
        }));
    }
    pub(in crate::desktop) fn select_global_match(
        &mut self,
        key: MatchKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search_results.collapsed.remove(&key.id);
        self.rebuild_results();
        self.open_match(key, window, cx);
        if let Some(index) = self.search_results.visible.iter().position(|row| {
            row.hit.is_some_and(|hit| {
                self.search_results.groups[row.group].id == key.id
                    && self.search_results.groups[row.group].matches[hit].range == key.range
            })
        }) {
            self.search_results.cursor = Some(index);
            self.search_results
                .scroll
                .scroll_to_item(index, ScrollStrategy::Nearest);
        }
        cx.notify();
    }
}
