use super::*;
use component::scroll::AutoScroll;

impl Edot {
    pub(super) fn show_keyboard_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use component::menu::PopupMenu;
        let weak = cx.entity().downgrade();
        let focus = window.focused(cx).unwrap_or_else(|| self.focus.clone());
        let tree = self.tree_focus.is_focused(window);
        let id = if tree {
            self.caret
        } else {
            self.session.active
        };
        let Some(id) = id else {
            return;
        };
        if tree && !self.selection.ids.contains(&id) {
            self.selection.single(id);
        }
        let multiple = self.selection.ids.len() > 1;
        let nested = self.book.get(id).is_some_and(|note| note.parent != 0);
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            menu = menu.action_context(focus);
            if tree {
                if !multiple {
                    let action = weak.clone();
                    menu = menu.item(PopupMenuItem::new("하위 메모 추가").on_click(
                        move |_, window, cx| {
                            let _ = action
                                .update(cx, |this, cx| this.create_note(Some(id), window, cx));
                        },
                    ));
                    let action = weak.clone();
                    menu = menu.item(PopupMenuItem::new("이름 바꾸기").shortcut("F2").on_click(
                        move |_, window, cx| {
                            let _ = action.update(cx, |this, cx| this.begin_rename(id, window, cx));
                        },
                    ));
                    let action = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new("한 단계 위로")
                            .disabled(!nested)
                            .on_click(move |_, _, cx| {
                                let _ = action.update(cx, |this, cx| this.outdent(id, cx));
                            }),
                    );
                }
                let remove = weak.clone();
                menu = menu.separator().item(
                    PopupMenuItem::new("삭제")
                        .shortcut("Delete")
                        .destructive()
                        .on_click(move |_, window, cx| {
                            let _ = remove.update(cx, |this, cx| this.confirm_delete(window, cx));
                        }),
                );
                if multiple {
                    menu = menu.item(
                        PopupMenuItem::new("선택한 항목과 함께 새 폴더 만들기").on_click(
                            move |_, window, cx| {
                                let _ = weak.update(cx, |this, cx| this.group_selected(window, cx));
                            },
                        ),
                    );
                }
            } else {
                menu = menu
                    .menu("실행 취소", "Ctrl+Z", Box::new(component::input::Undo))
                    .menu("다시 실행", "Ctrl+Y", Box::new(component::input::Redo))
                    .separator()
                    .menu("잘라내기", "Ctrl+X", Box::new(component::input::Cut))
                    .menu("복사", "Ctrl+C", Box::new(component::input::Copy))
                    .menu("붙여넣기", "Ctrl+V", Box::new(component::input::Paste))
                    .menu("전체 선택", "Ctrl+A", Box::new(component::input::SelectAll));
            }
            menu
        });
        self.menu_subscription = Some(cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.keyboard_menu = None;
            cx.notify();
        }));
        self.menu_position = if tree {
            let index = self
                .visible_tree()
                .iter()
                .position(|row| row.0 == id)
                .unwrap_or(0);
            let handle = &self.tree_scroll.0.borrow().base_handle;
            point(
                handle.bounds().left() + px(36.),
                (handle.bounds().top() + handle.offset().y + px((index + 1) as f32 * 29.))
                    .max(handle.bounds().top()),
            )
        } else {
            point(
                px(if self.session.sidebar_visible {
                    self.session.sidebar_width + 30
                } else {
                    30
                } as f32),
                px(70.),
            )
        };
        menu.read(cx).focus_handle(cx).focus(window, cx);
        self.keyboard_menu = Some(menu);
        cx.notify();
    }
    pub(super) fn tree_drag_at(
        &mut self,
        position: Point<Pixels>,
        ids: &[u64],
        cx: &mut Context<Self>,
    ) {
        self.dragging_notes = ids.to_vec();
        self.tree_auto.last_drag_position = Some(position);
        self.resolve_tree_drag();
        let bounds = self.tree_bounds.get();
        let delta = if bounds.contains(&position) {
            if position.y < bounds.top() + px(24.) {
                Some(px(-2. * TREE_ROW_HEIGHT))
            } else if position.y >= bounds.bottom() - px(24.) {
                Some(px(2. * TREE_ROW_HEIGHT))
            } else {
                None
            }
        } else {
            None
        };
        self.tree_auto
            .set_interval(delta, Duration::from_millis(160), cx, |delta, this, cx| {
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
    pub(super) fn tree_drop_at(&self, position: Point<Pixels>, ids: &[u64]) -> Option<DropTarget> {
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
        ids: &[u64],
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
