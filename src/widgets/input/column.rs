//! Rectangular selection in shaped visual rows (logical pixels, including scrolling).
use super::*;

impl InputState {
    fn column_point(&self, p: Point<Pixels>) -> Point<Pixels> {
        let y = self.row_at_y(p.y).map_or(self.content_padding.top, |line| {
            let row = ((p.y - line.top) / line.line_height).floor();
            let max = if line.rich.is_some() {
                0.
            } else {
                line.line.wrap_boundaries.len() as f32
            };
            line.top + line.line_height * row.clamp(0., max)
        });
        point(p.x.max(self.content_padding.left), y)
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
        let start = anchor.y.min(head.y);
        let end = anchor.y.max(head.y);
        let first = self
            .lines
            .partition_point(|line| line.top + line.height() <= start);
        let mut rows: Vec<_> = self.lines[first..]
            .iter()
            .take_while(|line| line.top <= end)
            .flat_map(|line| {
                let last_wrap = if line.rich.is_some() {
                    0
                } else {
                    line.line.wrap_boundaries.len()
                };
                (0..=last_wrap).map(|row| line.top + line.line_height * row as f32)
            })
            .filter(|&y| start <= y && y <= end)
            .collect();
        if rows.is_empty() {
            rows.push(head.y);
        } else if head.y < anchor.y {
            rows.reverse();
        }
        let left = anchor.x.min(head.x);
        let right = anchor.x.max(head.x);
        let mut selections = Vec::with_capacity(rows.len());
        for &y in &rows {
            let start = self.index_at(point(anchor.x, y + px(0.5)));
            let end = self.index_at(point(head.x, y + px(0.5)));
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
            for y in rows {
                let end = self.index_at(point(px(1e8), y + px(0.5)));
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
