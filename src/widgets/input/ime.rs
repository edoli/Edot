//! UTF-16 conversion and platform text composition protocol.
use super::*;

impl InputState {
    pub(super) fn utf8(&self, offset: usize) -> usize {
        utf8_offset(&self.content, offset)
    }
    pub(super) fn utf16(&self, offset: usize) -> usize {
        self.content[..self.boundary(offset)].encode_utf16().count()
    }
    pub(super) fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.utf8(r.start)..self.utf8(r.end)
    }
    pub(super) fn to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.utf16(r.start)..self.utf16(r.end)
    }
}
fn utf8_offset(text: &str, n: usize) -> usize {
    let mut units = 0;
    for (i, ch) in text.char_indices() {
        if units >= n {
            return i;
        }
        units += ch.len_utf16();
    }
    text.len()
}
impl EntityInputHandler for InputState {
    fn text_for_range(
        &mut self,
        r: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.range_from_utf16(&r);
        *actual = Some(self.to_utf16(&r));
        Some(self.content[r].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.to_utf16(&self.selection),
            reversed: self.reversed,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| self.to_utf16(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition();
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = r
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.clone());
        let text = if self.multiline {
            text.to_string()
        } else {
            text.replace(['\r', '\n'], " ")
        };
        if self.composition.is_some() {
            let changed = self.content[r.clone()] != *text;
            self.apply(r, &text);
            self.finish_composition();
            if changed {
                self.changed(cx);
            } else {
                self.moved(cx);
            }
        } else {
            self.commit(r, &text, true, cx);
        }
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composition.is_none() {
            self.composition = Some((self.content.clone(), self.selection.clone(), self.reversed));
        }
        let r = r
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.clone());
        let start = r.start;
        self.apply(r, text);
        self.marked = Some(start..start + text.len());
        if let Some(r) = selection {
            self.selection = start + utf8_offset(text, r.start)..start + utf8_offset(text, r.end);
        }
        self.changed(cx);
    }
    fn bounds_for_range(
        &mut self,
        r: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let r = self.range_from_utf16(&r);
        let a = if r.is_empty() && r.start == self.cursor() {
            self.caret_position()
        } else {
            self.position_with_wrap_side(r.start, !r.is_empty())
        };
        let b = self.position(r.end);
        Some(Bounds::new(
            self.bounds.origin + self.scroll + a,
            size(
                if a.y == b.y {
                    (b.x - a.x).max(px(1.))
                } else {
                    px(1.)
                },
                self.line_height,
            ),
        ))
    }
    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.utf16(self.mouse_index(p)))
    }
}
