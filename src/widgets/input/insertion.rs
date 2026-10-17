//! Keep asynchronous image insertion attached to its original text range.
use super::*;
use std::cell::Cell;

#[derive(Clone)]
pub struct DropInsertion {
    start: Rc<Cell<Option<usize>>>,
    end: Rc<Cell<Option<usize>>>,
}

impl InputState {
    pub fn can_insert_images(&self) -> bool {
        self.multiline && self.text_mode == TextMode::Markdown && !self.is_read_only()
    }
    #[cfg(test)]
    pub(crate) fn image_drop_feedback(&self) -> Option<usize> {
        self.drop_position
            .map(|position| self.mouse_index(position))
    }
    pub fn accepts_image_drop(&self, position: Point<Pixels>, window: &Window) -> bool {
        self.can_insert_images()
            && self
                .drop_hitbox
                .as_ref()
                .is_some_and(|hitbox| hitbox.is_hovered_at(position, window))
    }

    pub fn show_image_drop(
        &mut self,
        position: Option<Point<Pixels>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let position = position.filter(|position| self.accepts_image_drop(*position, window));
        if self.drop_position != position {
            self.drop_position = position;
            cx.notify();
        }
    }

    pub(super) fn drop_caret(&self) -> Option<Point<Pixels>> {
        let mouse = self.drop_position?;
        let index = self.mouse_index(mouse);
        let before = self.position_with_wrap_side(index, false);
        let after = self.position_with_wrap_side(index, true);
        let y = mouse.y - self.bounds.top() - self.scroll.y;
        Some(
            if (y - (after.y + self.line_height / 2.)).abs()
                < (y - (before.y + self.line_height / 2.)).abs()
            {
                after
            } else {
                before
            },
        )
    }

    pub fn supports_image_path(path: &std::path::Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"]
                    .iter()
                    .any(|supported| extension.eq_ignore_ascii_case(supported))
            })
    }

    pub fn begin_drop(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> DropInsertion {
        self.finish_composition();
        self.stop_pointer_drag();
        let index = self.mouse_index(position);
        self.set_selected_range(index..index, cx);
        self.focus(window, cx);
        self.begin_image_insertion(index..index)
    }

    pub(super) fn begin_image_paste(&mut self) -> DropInsertion {
        self.finish_composition();
        self.stop_pointer_drag();
        self.begin_image_insertion(self.selection.clone())
    }

    fn begin_image_insertion(&mut self, range: Range<usize>) -> DropInsertion {
        let start = Rc::new(Cell::new(Some(range.start)));
        let end = Rc::new(Cell::new(Some(range.end)));
        self.pending_insertions.push(Rc::downgrade(&start));
        self.pending_insertions.push(Rc::downgrade(&end));
        DropInsertion { start, end }
    }

    pub fn finish_drop(
        &mut self,
        anchor: &DropInsertion,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.can_insert_images() || self.is_composing() {
            return false;
        }
        let Some(start) = anchor.start.take() else {
            return false;
        };
        let Some(end) = anchor.end.take() else {
            return false;
        };
        self.commit(start..end, text, false, cx);
        true
    }

    pub(super) fn rebase_insertions(&mut self, range: &Range<usize>, length: usize) {
        self.pending_insertions.retain(|anchor| {
            let Some(anchor) = anchor.upgrade() else {
                return false;
            };
            let Some(index) = anchor.get() else {
                return false;
            };
            anchor.set(Some(if index < range.start {
                index
            } else if index >= range.end {
                index - range.len() + length
            } else {
                range.start + length
            }));
            true
        });
    }
}
