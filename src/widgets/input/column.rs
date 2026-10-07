//! Rectangular selection in shaped visual rows (logical pixels, including scrolling).
use super::*;

impl InputState {
    fn column_point(&self, p: Point<Pixels>) -> Point<Pixels> {
        let top = self.content_padding.top;
        let last = if self.content.is_empty() {
            top
        } else {
            self.lines.last().map_or(top, |line| {
                line.top + line.line.size(self.line_height).height - self.line_height
            })
        };
        let row = ((p.y - top) / self.line_height).floor().max(0.);
        point(
            p.x.max(self.content_padding.left),
            (top + self.line_height * row).min(last),
        )
    }

    pub(super) fn begin_column_selection(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.finish_composition();
        self.clear_extra_selections();
        self.column_anchor = Some(self.column_point(position - self.bounds.origin - self.scroll));
        self.selecting = true;
        self.update_column_selection(position, cx);
    }

    pub(super) fn update_column_selection(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(anchor) = self.column_anchor else {
            return;
        };
        let head = self.column_point(position - self.bounds.origin - self.scroll);
        let count = ((head.y - anchor.y).abs() / self.line_height).round() as usize + 1;
        let direction = if head.y >= anchor.y { 1. } else { -1. };
        let left = anchor.x.min(head.x);
        let right = anchor.x.max(head.x);
        let mut selections = Vec::with_capacity(count);
        for row in 0..count {
            let y = anchor.y + self.line_height * (direction * row as f32);
            let start = self.index_at(point(anchor.x, y + self.line_height / 2.));
            let end = self.index_at(point(head.x, y + self.line_height / 2.));
            // Hit testing clamps beyond a row's text. Reject that row if the
            // resulting caret interval lies outside the horizontal rectangle.
            // At a soft-wrap start, the same byte also represents the prior
            // row's end; compare the position on the row being selected.
            let row_x = |at| {
                let p = self.position(at);
                if p.y < y {
                    self.content_padding.left
                } else {
                    p.x
                }
            };
            if left < right
                && (row_x(start).max(row_x(end)) + px(0.001) < left
                    || row_x(start).min(row_x(end)) > right + px(0.001))
            {
                continue;
            }
            selections.push(Selection {
                range: start.min(end)..start.max(end),
                reversed: end < start,
                preferred_x: None,
                after_wrap: self.position(end).y < y,
            });
        }
        if selections.is_empty() {
            // Match VS Code's all-outside fallback: retain one end-of-row caret
            // per visual row rather than leaving the editor without a caret.
            for row in 0..count {
                let y = anchor.y + self.line_height * (direction * row as f32);
                let end = self.index_at(point(px(1e8), y + self.line_height / 2.));
                selections.push(Selection {
                    range: end..end,
                    reversed: false,
                    preferred_x: None,
                    after_wrap: self.position(end).y < y,
                });
            }
        }
        let primary = selections.pop().unwrap();
        self.selection = primary.range;
        self.reversed = primary.reversed;
        self.caret_after_wrap = primary.after_wrap;
        self.extra = selections;
        self.deduplicate_selections();
        self.last_edit = None;
        self.preferred_x = None;
        self.occurrence_word = false;
        self.moved(cx);
        // Mouse-driven scrolling is controlled by the pointer, not the caret.
        self.reveal = false;
    }
}
