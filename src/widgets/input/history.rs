//! Undo transactions and clipboard operations.
use super::*;
const TYPING_MERGE_INTERVAL: Duration = Duration::from_millis(700);

impl InputState {
    pub(super) fn apply(&mut self, range: Range<usize>, text: &str) {
        let mut value = String::with_capacity(self.content.len() - range.len() + text.len());
        value.push_str(&self.content[..range.start]);
        value.push_str(text);
        value.push_str(&self.content[range.end..]);
        self.content = value.into();
        self.selection = range.start + text.len()..range.start + text.len();
        self.reversed = false;
    }
    pub(super) fn commit(
        &mut self,
        range: Range<usize>,
        text: &str,
        merge: bool,
        cx: &mut Context<Self>,
    ) {
        if self.content[range.clone()] == *text {
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
            && self
                .undo
                .last()
                .is_some_and(|e| e.before.is_empty() && e.start + e.after.len() == range.start);
        if coalesce {
            self.undo.last_mut().unwrap().after.push_str(text);
        } else {
            self.undo.push(Edit {
                start: range.start,
                before: self.content[range.clone()].into(),
                after: text.into(),
                selection: self.selection.clone(),
                reversed: self.reversed,
            });
        }
        self.last_edit = merge.then(std::time::Instant::now);
        self.redo.clear();
        self.apply(range, text);
        self.marked = None;
        self.changed(cx);
    }
    pub(super) fn finish_composition(&mut self) {
        if let Some((before, selection, reversed)) = self.composition.take() {
            self.undo.push(Edit {
                start: 0,
                before: before.to_string(),
                after: self.content.to_string(),
                selection,
                reversed,
            });
            self.redo.clear();
        }
        self.marked = None;
    }
    pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition();
        self.last_edit = None;
        if let Some(edit) = self.undo.pop() {
            let redo = Edit {
                selection: self.selection.clone(),
                reversed: self.reversed,
                ..edit.clone()
            };
            self.apply(edit.start..edit.start + edit.after.len(), &edit.before);
            self.selection = edit.selection;
            self.reversed = edit.reversed;
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
                ..edit.clone()
            };
            self.apply(edit.start..edit.start + edit.before.len(), &edit.after);
            self.selection = edit.selection;
            self.reversed = edit.reversed;
            self.undo.push(undo);
            self.changed(cx);
        }
    }
    pub(super) fn clipboard_range(&self) -> Range<usize> {
        if !self.selection.is_empty() || !self.multiline {
            return self.selection.clone();
        }
        // Logical line, including its original terminator (CRLF, CR or LF).
        let cursor = self.cursor();
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
        let range = self.clipboard_range();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[range].into()));
        }
    }
    pub(super) fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
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
            self.last_edit = None;
            self.replace(text, window, cx);
            self.last_edit = None;
        }
    }
}
