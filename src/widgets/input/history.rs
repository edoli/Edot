//! Undo transactions and clipboard operations.
use super::*;
pub(super) const TYPING_MERGE_INTERVAL: Duration = Duration::from_millis(700);

impl InputState {
    pub(super) fn apply(&mut self, range: Range<usize>, text: &str) {
        let mut value = String::with_capacity(self.content.len() - range.len() + text.len());
        value.push_str(&self.content[..range.start]);
        value.push_str(text);
        value.push_str(&self.content[range.end..]);
        self.content = value.into();
        self.selection = range.start + text.len()..range.start + text.len();
        self.reversed = false;
        self.caret_after_wrap = false;
    }
    pub(super) fn commit(
        &mut self,
        range: Range<usize>,
        text: &str,
        merge: bool,
        cx: &mut Context<Self>,
    ) {
        if self.content[range.clone()] == *text {
            self.clear_extra_selections();
            self.selection = range.start + text.len()..range.start + text.len();
            self.reversed = false;
            self.marked = None;
            self.moved(cx);
            return;
        }
        let coalesce = merge
            && range.is_empty()
            && !text.contains(['\r', '\n'])
            && self
                .last_edit
                .is_some_and(|t| t.elapsed() < TYPING_MERGE_INTERVAL)
            && self.undo.last().is_some_and(|e| {
                e.others.is_empty() && e.before.is_empty() && e.start + e.after.len() == range.start
            });
        if coalesce {
            self.undo.last_mut().unwrap().after.push_str(text);
        } else {
            self.undo.push(Edit {
                start: range.start,
                before: self.content[range.clone()].into(),
                after: text.into(),
                selection: self.selection.clone(),
                reversed: self.reversed,
                after_wrap: self.caret_after_wrap,
                extra: self.extra.clone(),
                others: vec![],
            });
        }
        self.last_edit = merge.then(std::time::Instant::now);
        self.redo.clear();
        self.apply(range, text);
        self.clear_extra_selections();
        self.marked = None;
        self.changed(cx);
    }
    pub(super) fn finish_composition(&mut self) {
        if let Some(Composition {
            before,
            primary,
            extra,
        }) = self.composition.take()
        {
            self.undo.push(Edit {
                start: 0,
                before: before.to_string(),
                after: self.content.to_string(),
                selection: primary.range,
                reversed: primary.reversed,
                after_wrap: primary.after_wrap,
                extra,
                others: vec![],
            });
            self.redo.clear();
        }
        self.marked = None;
        self.extra_marked.clear();
    }
    pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition();
        self.last_edit = None;
        if let Some(edit) = self.undo.pop() {
            let redo = Edit {
                selection: self.selection.clone(),
                reversed: self.reversed,
                after_wrap: self.caret_after_wrap,
                extra: self.extra.clone(),
                ..edit.clone()
            };
            for change in edit.others.iter().rev() {
                self.apply(
                    change.start..change.start + change.after.len(),
                    &change.before,
                );
            }
            self.apply(edit.start..edit.start + edit.after.len(), &edit.before);
            self.selection = edit.selection;
            self.reversed = edit.reversed;
            self.caret_after_wrap = edit.after_wrap;
            self.extra = edit.extra;
            self.occurrence_word = false;
            self.redo.push(redo);
            self.changed(cx);
        }
    }
    pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.last_edit = None;
        if let Some(edit) = self.redo.pop() {
            let undo = Edit {
                selection: self.selection.clone(),
                reversed: self.reversed,
                after_wrap: self.caret_after_wrap,
                extra: self.extra.clone(),
                ..edit.clone()
            };
            self.apply(edit.start..edit.start + edit.before.len(), &edit.after);
            for change in &edit.others {
                self.apply(
                    change.start..change.start + change.before.len(),
                    &change.after,
                );
            }
            self.selection = edit.selection;
            self.reversed = edit.reversed;
            self.caret_after_wrap = edit.after_wrap;
            self.extra = edit.extra;
            self.occurrence_word = false;
            self.undo.push(undo);
            self.changed(cx);
        }
    }
    pub(super) fn clipboard_range(&self) -> Range<usize> {
        self.clipboard_range_at(self.selection.clone())
    }
    pub(super) fn clipboard_range_at(&self, selection: Range<usize>) -> Range<usize> {
        if !selection.is_empty() || !self.multiline {
            return selection;
        }
        // Logical line, including its original terminator (CRLF, CR or LF).
        let cursor = selection.start;
        let start = self.content[..cursor]
            .rfind(['\r', '\n'])
            .map_or(0, |i| i + 1);
        let end = self.content[cursor..]
            .find(['\r', '\n'])
            .map_or(self.content.len(), |i| {
                let end = cursor + i;
                end + if self.content[end..].starts_with("\r\n") {
                    2
                } else {
                    1
                }
            });
        start..end
    }
    pub(super) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.extra.is_empty() {
            let text = self.multiple_clipboard_text();
            if !text.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            return;
        }
        let range = self.clipboard_range();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[range].into()));
        }
    }
    pub(super) fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.extra.is_empty() {
            self.finish_composition();
            let text = self.multiple_clipboard_text();
            if !text.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                let linewise = self.selected_ranges().all(|r| r.is_empty());
                let ranges: Vec<_> = self
                    .selected_ranges()
                    .map(|r| {
                        if linewise {
                            self.clipboard_range_at(r)
                        } else {
                            r
                        }
                    })
                    .collect();
                let texts = vec![String::new(); ranges.len()];
                self.commit_multiple(ranges, texts, false, cx);
            }
            return;
        }
        let range = self.clipboard_range();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[range.clone()].into(),
            ));
            self.finish_composition();
            self.commit(range, "", false, cx);
        }
    }
    pub(super) fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
            if !self.extra.is_empty() {
                self.finish_composition();
                let ranges: Vec<_> = self.selected_ranges().collect();
                let lines: Vec<_> = text
                    .split('\n')
                    .map(|s| s.strip_suffix('\r').unwrap_or(s).to_owned())
                    .collect();
                let texts = if lines.len() == ranges.len() {
                    let mut order: Vec<_> = (0..ranges.len()).collect();
                    order.sort_by_key(|&i| ranges[i].start);
                    let mut texts = vec![String::new(); ranges.len()];
                    for (i, line) in order.into_iter().zip(lines) {
                        texts[i] = line;
                    }
                    texts
                } else {
                    vec![text; ranges.len()]
                };
                self.commit_multiple(ranges, texts, false, cx);
                return;
            }
            self.last_edit = None;
            self.replace(text, window, cx);
            self.last_edit = None;
        }
    }
}
