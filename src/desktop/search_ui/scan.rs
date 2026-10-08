use super::*;

impl Edot {
    pub(in crate::desktop) fn queue_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_results.generation += 1;
        let generation = self.search_results.generation;
        self.search_results.scan_task = None;
        self.search_results.note_tasks.clear();
        self.search_results.pending_notes.clear();
        self.search_results.errors = 0;
        self.search_results.groups.clear();
        self.search_results.visible.clear();
        self.search_results.cursor = None;
        self.global.last = None;
        let pattern = match self.global.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            Ok(None) => {
                self.search_results.scanning = false;
                self.global.message = "검색어를 입력하세요".into();
                cx.notify();
                return;
            }
            Err(error) => {
                self.search_results.scanning = false;
                self.global.message = error;
                cx.notify();
                return;
            }
        };
        self.search_results.scanning = true;
        self.global.message = "전체 메모 검색 중…".into();
        self.search_results.scan_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            let Ok(ids) = this.update(cx, |this, _| {
                this.book.notes.iter().map(|n| n.id).collect::<Vec<_>>()
            }) else {
                return;
            };
            let mut rows = Vec::new();
            let mut displayed = 0;
            let mut errors = 0;
            for (index, id) in ids.iter().copied().enumerate() {
                let Ok(Some((path, loaded))) = this.update(cx, |this, cx| {
                    if this.search_results.generation != generation {
                        return None;
                    }
                    let loaded = this
                        .open
                        .iter()
                        .find(|tab| tab.id == id)
                        .and_then(|tab| tab.editor.as_ref())
                        .map(|editor| editor.read(cx).value().to_string());
                    Some((this.book.file(&this.dir, id), loaded))
                }) else {
                    return;
                };
                let pattern = pattern.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let text = loaded.map(Ok).unwrap_or_else(|| fs::read_to_string(path))?;
                        Ok::<_, io::Error>(global_search::scan(
                            id,
                            &text,
                            &pattern,
                            global_search::MAX_RESULTS.saturating_sub(displayed),
                        ))
                    })
                    .await;
                match result {
                    Ok(Some(row)) => {
                        displayed += row.matches.len();
                        rows.push(row);
                    }
                    Err(_) => errors += 1,
                    _ => {}
                }
                if index % 16 == 0 {
                    let _ = this.update(cx, |this, cx| {
                        this.global.message =
                            format!("전체 메모 검색 중… {}/{}", index + 1, ids.len());
                        cx.notify();
                    });
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                if this.search_results.generation != generation {
                    return;
                }
                this.search_results.groups = rows;
                this.search_results.errors = errors;
                this.update_search_message();
                this.rebuild_results();
                this.search_results.scanning = false;
                this.refresh_pending_notes(window, cx);
                if let Some(backwards) = this.search_results.pending_navigation.take() {
                    this.global_next(backwards, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(in crate::desktop) fn update_search_message(&mut self) {
        let total: usize = self.search_results.groups.iter().map(|r| r.count).sum();
        let displayed: usize = self
            .search_results
            .groups
            .iter()
            .map(|r| r.matches.len())
            .sum();
        self.global.message = format!("{total}곳 · {}개 메모", self.search_results.groups.len());
        if self.search_results.errors > 0 {
            self.global
                .message
                .push_str(&format!(" · {}개 읽기 실패", self.search_results.errors));
        }
        if total > displayed {
            self.global
                .message
                .push_str(&format!(" · {displayed}개까지 표시"));
        }
    }
    pub(in crate::desktop) fn refresh_pending_notes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for id in std::mem::take(&mut self.search_results.pending_notes) {
            self.queue_note_search(id, window, cx);
        }
    }
    pub(in crate::desktop) fn queue_note_search(
        &mut self,
        id: NoteId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A full scan may already have read this note. Reconcile after it finishes.
        if self.search_results.scanning {
            self.search_results.pending_notes.insert(id);
            return;
        }
        let Ok(Some(pattern)) = self.global.pattern(cx) else {
            return;
        };
        let generation = self.search_results.generation;
        self.search_results.note_tasks.remove(&id);
        let task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            let Ok(Some((path, loaded, revision))) = this.update(cx, |this, cx| {
                this.book.get(id)?;
                let editor = this
                    .open
                    .iter()
                    .find(|t| t.id == id)
                    .and_then(|t| t.editor.as_ref());
                Some((
                    this.book.file(&this.dir, id),
                    editor.map(|e| e.read(cx).value()),
                    editor.map(|e| e.read(cx).revision),
                ))
            }) else {
                return;
            };
            let row = cx
                .background_executor()
                .spawn(async move {
                    let text = loaded
                        .map(|s| Ok(s.to_string()))
                        .unwrap_or_else(|| fs::read_to_string(path))?;
                    Ok::<_, io::Error>(global_search::scan(
                        id,
                        &text,
                        &pattern,
                        global_search::MAX_RESULTS,
                    ))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.search_results.generation {
                    return;
                }
                this.search_results.note_tasks.remove(&id);
                let current = this
                    .open
                    .iter()
                    .find(|t| t.id == id)
                    .and_then(|t| t.editor.as_ref())
                    .map(|e| e.read(cx).revision);
                if current != revision {
                    this.queue_note_search(id, window, cx);
                    return;
                }
                match row {
                    Ok(row) => {
                        this.merge_note_results(id, row);
                        // Only refill previously truncated previews when space becomes available.
                        let displayed: usize = this
                            .search_results
                            .groups
                            .iter()
                            .map(|r| r.matches.len())
                            .sum();
                        if displayed < global_search::MAX_RESULTS
                            && let Some(next) = this
                                .search_results
                                .groups
                                .iter()
                                .find(|r| {
                                    r.count > r.matches.len()
                                        && !this.search_results.note_tasks.contains_key(&r.id)
                                })
                                .map(|r| r.id)
                        {
                            this.queue_note_search(next, window, cx);
                        }
                    }
                    Err(error) => this.global.message = format!("메모 검색 실패: {error}"),
                }
                cx.notify();
            });
        });
        self.search_results.note_tasks.insert(id, task);
    }
}
