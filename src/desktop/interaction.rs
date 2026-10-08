use super::*;
use component::scroll::AutoScroll;

impl Edot {
    pub(super) fn tree_drag_at(
        &mut self,
        position: Point<Pixels>,
        ids: &[NoteId],
        cx: &mut Context<Self>,
    ) {
        self.dragging_notes = ids.to_vec();
        self.tree_auto.last_drag_position = Some(position);
        self.resolve_tree_drag();
        let bounds = self.tree_bounds.get();
        let delta = if bounds.contains(&position) {
            if position.y < bounds.top() + px(TREE_SCROLL_EDGE) {
                Some(px(-TREE_SCROLL_ROWS * TREE_ROW_HEIGHT))
            } else if position.y >= bounds.bottom() - px(TREE_SCROLL_EDGE) {
                Some(px(TREE_SCROLL_ROWS * TREE_ROW_HEIGHT))
            } else {
                None
            }
        } else {
            None
        };
        self.tree_auto
            .set_interval(delta, TREE_SCROLL_INTERVAL, cx, |delta, this, cx| {
                if !cx.has_active_drag() {
                    this.stop_drag();
                    cx.notify();
                    return;
                }
                let handle = this.tree_scroll.0.borrow().base_handle.clone();
                let offset = handle.offset();
                handle.set_offset(point(
                    offset.x,
                    (offset.y - delta).clamp(-handle.max_offset().y, px(0.)),
                ));
                this.resolve_tree_drag();
                cx.notify();
            });
        cx.notify();
    }
    fn resolve_tree_drag(&mut self) {
        let Some(position) = self.tree_auto.last_drag_position else {
            return;
        };
        self.drop_target = self.tree_drop_at(position, &self.dragging_notes);
        self.pressed = None;
    }
    pub(super) fn tree_drop_at(
        &self,
        position: Point<Pixels>,
        ids: &[NoteId],
    ) -> Option<DropTarget> {
        if !self.session.sidebar_visible
            || self.search_mode
            || !self.tree_bounds.get().contains(&position)
        {
            return None;
        }
        let handle = self.tree_scroll.0.borrow().base_handle.clone();
        let y = f32::from(position.y - handle.bounds().top() - handle.offset().y);
        let visible = self.visible_tree();
        if y >= 0.
            && let Some(&(id, _)) = visible.get((y / TREE_ROW_HEIGHT) as usize)
        {
            let index = (y / TREE_ROW_HEIGHT) as usize;
            DropTarget::resolve_row(
                &self.book,
                ids,
                id,
                index.checked_sub(1).map(|i| visible[i].0),
                visible.get(index + 1).map(|r| r.0),
                (y - index as f32 * TREE_ROW_HEIGHT) as i32,
                TREE_ROW_HEIGHT as i32,
            )
        } else {
            DropTarget::resolve_many(&self.book, ids, None, DropZone::Root)
        }
    }
    pub(super) fn finish_tree_drag(
        &mut self,
        ids: &[NoteId],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Resolve again at release, so a stale hover target cannot move a note.
        if let Some(target) = self.tree_drop_at(window.mouse_position(), ids) {
            self.move_notes(ids, target, cx);
        }
        self.stop_drag();
        cx.notify();
    }
    pub(super) fn tree_drag_description(&self) -> Option<String> {
        let source = *self.dragging_notes.first()?;
        let source = if self.dragging_notes.len() > 1 {
            format!("{}개 항목", self.dragging_notes.len())
        } else {
            self.book.get(source)?.title.clone()
        };
        let action = self.drop_target.map_or_else(
            || "이 위치에는 놓을 수 없습니다".into(),
            |target| {
                let title = target
                    .note
                    .and_then(|id| self.book.get(id))
                    .map_or("", |n| n.title.as_str());
                match target.zone {
                    DropZone::Before => format!("{title} 위에 놓기"),
                    DropZone::After => format!("{title} 아래에 놓기"),
                    DropZone::Inside => format!("{title}의 하위로 이동"),
                    DropZone::InsideFirst => format!("{title} 안의 맨 위에 놓기"),
                    DropZone::InsideLast => format!("{title} 안의 맨 아래에 놓기"),
                    DropZone::AfterSubtree => format!("{title} 아래에 같은 레벨로 놓기"),
                    DropZone::Root => "최상위로 이동".into(),
                }
            },
        );
        Some(format!("{source} → {action}   ·   Esc: 취소"))
    }
    pub(super) fn insertion_indent(&self, target: DropTarget) -> Pixels {
        let mut parent = target.parent;
        let mut depth = 0;
        while let Some(note) = self.book.get(parent) {
            depth += 1;
            parent = note.parent;
        }
        px(TREE_ICON_LEFT + depth as f32 * TREE_INDENT)
    }
    pub(super) fn tab_drag_at(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.tab_auto.last_drag_position = Some(position);
        self.resolve_tab_drag();
        let bounds = self.tab_scroll.bounds();
        let rotated = Bounds::new(
            point(bounds.top(), bounds.left()),
            size(bounds.size.height, bounds.size.width),
        );
        let delta = if position.y >= bounds.top() && position.y <= bounds.bottom() {
            AutoScroll::compute_delta(position.x, rotated)
        } else {
            None
        };
        self.tab_auto.set(delta, cx, |delta, this, cx| {
            if !cx.has_active_drag() {
                this.tab_auto.stop();
                return;
            }
            let offset = this.tab_scroll.offset();
            this.tab_scroll
                .set_offset(point(offset.x - delta, offset.y));
            this.resolve_tab_drag();
            cx.notify();
        });
        cx.notify();
    }
    fn resolve_tab_drag(&mut self) {
        let Some(position) = self.tab_auto.last_drag_position else {
            return;
        };
        let x =
            f32::from(position.x - self.tab_scroll.bounds().left() - self.tab_scroll.offset().x);
        self.tab_drop =
            Some(((x / self.tab_width as f32 + 0.5).floor().max(0.) as usize).min(self.open.len()));
    }
    pub(super) fn stop_drag(&mut self) {
        self.tree_auto.stop();
        self.tab_auto.stop();
        self.dragging_notes.clear();
        self.pressed = None;
        self.drop_target = None;
        self.tab_drop = None;
    }
}
