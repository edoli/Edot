//! Undo transactions and clipboard operations.
use super::*;
pub(super) const TYPING_MERGE_INTERVAL: Duration = Duration::from_millis(700);
const LINE_COPY_METADATA: &str = "edot-line-copy:1";

impl InputState {
    pub(super) fn apply(&mut self, range: Range<usize>, text: &str) {
        self.rebase_insertions(&range, text.len());
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
                task_toggle: false,
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
                task_toggle: false,
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
        if self.is_read_only()
            && !(self.is_markdown_reading()
                && self.undo.last().is_some_and(|edit| edit.task_toggle))
        {
            return;
        }
        self.finish_composition();
        self.last_edit = None;
        if let Some(edit) = self.undo.pop() {
            let reading_scroll = (self.is_markdown_reading() && edit.task_toggle)
                .then(|| self.reading.list.logical_scroll_top());
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
            if let Some(offset) = reading_scroll {
                self.reading.list.scroll_to(offset);
                self.reveal = false;
            }
        }
    }
    pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_read_only()
            && !(self.is_markdown_reading()
                && self.redo.last().is_some_and(|edit| edit.task_toggle))
        {
            return;
        }
        self.last_edit = None;
        if let Some(edit) = self.redo.pop() {
            let reading_scroll = (self.is_markdown_reading() && edit.task_toggle)
                .then(|| self.reading.list.logical_scroll_top());
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
            if let Some(offset) = reading_scroll {
                self.reading.list.scroll_to(offset);
                self.reveal = false;
            }
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
            self.write_clipboard_range(range, cx);
        }
    }
    fn write_clipboard_range(&self, range: Range<usize>, cx: &mut Context<Self>) {
        let text = self.content[range].to_string();
        let item = if self.multiline && self.selection.is_empty() {
            ClipboardItem::new_string_with_metadata(text, LINE_COPY_METADATA.into())
        } else {
            ClipboardItem::new_string(text)
        };
        cx.write_to_clipboard(item);
    }
    pub(super) fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_read_only() {
            return;
        }
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
            self.write_clipboard_range(range.clone(), cx);
            self.finish_composition();
            self.commit(range, "", false, cx);
        }
    }
    pub(super) fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_read_only() {
            return;
        }
        if let Some(item) = cx.read_from_clipboard() {
            if self.can_insert_images()
                && item.entries().iter().any(|entry| match entry {
                    ClipboardEntry::Image(image) => matches!(
                        image.format,
                        gpui::ImageFormat::Png
                            | gpui::ImageFormat::Jpeg
                            | gpui::ImageFormat::Webp
                            | gpui::ImageFormat::Gif
                            | gpui::ImageFormat::Svg
                            | gpui::ImageFormat::Bmp
                    ),
                    ClipboardEntry::ExternalPaths(paths) => {
                        paths.0.iter().any(|path| Self::supports_image_path(path))
                    }
                    _ => false,
                })
            {
                let insertion = self.begin_image_paste();
                cx.emit(InputEvent::PasteImages { item, insertion });
                return;
            }
            let Some(text) = item.text() else {
                return;
            };
            if self.multiline
                && item.metadata().is_some_and(|m| m == LINE_COPY_METADATA)
                && self.selected_ranges().all(|r| r.is_empty())
            {
                self.paste_line(text, cx);
                return;
            }
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

    fn paste_line(&mut self, mut text: String, cx: &mut Context<Self>) {
        self.finish_composition();
        // A copied final line may have no terminator. Use the destination's
        // existing line ending, or LF for a document with no line endings.
        if !text.ends_with(['\r', '\n']) {
            let newline = self.content.find(['\r', '\n']).map_or("\n", |at| {
                if self.content[at..].starts_with("\r\n") {
                    "\r\n"
                } else {
                    &self.content[at..at + 1]
                }
            });
            text.push_str(newline);
        }
        let selection = self.selection.clone();
        let mut extra = self.extra.clone();
        let ranges: Vec<_> = self
            .selected_ranges()
            .map(|r| {
                let start = self.clipboard_range_at(r).start;
                start..start
            })
            .collect();
        let mut starts: Vec<_> = ranges.iter().map(|r| r.start).collect();
        starts.sort_unstable();
        starts.dedup();
        if extra.is_empty() {
            self.commit(ranges[0].clone(), &text, false, cx);
        } else {
            let texts = vec![text.clone(); ranges.len()];
            self.commit_multiple(ranges, texts, false, cx);
        }
        // Keep every caret on its original line and column after insertion.
        let shifted = |at| at + starts.partition_point(|&start| start <= at) * text.len();
        let cursor = shifted(selection.start);
        self.selection = cursor..cursor;
        for selection in &mut extra {
            let cursor = shifted(selection.range.start);
            selection.range = cursor..cursor;
        }
        self.extra = extra;
        self.deduplicate_selections();
        self.moved(cx);
    }
}
