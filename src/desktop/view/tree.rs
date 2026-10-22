//! Tree presentation.
use super::*;

impl Edot {
    pub(in crate::desktop) fn sidebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let width = sidebar_width(
            self.session.sidebar_width,
            f32::from(window.viewport_size().width),
        );
        div()
            .v_flex()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().sidebar)
            .child(
                div()
                    .h_flex()
                    .h(px(40.))
                    .flex_shrink_0()
                    .px_1()
                    .gap_1()
                    .child(
                        Button::new("notes-mode")
                            .small()
                            .label(if width < 220. {
                                "노트"
                            } else {
                                "노트 목록"
                            })
                            .h_full()
                            .flex_1()
                            .rounded_none()
                            .border_b(px(3.))
                            .border_color(if !self.search_mode {
                                cx.theme().primary
                            } else {
                                gpui::transparent_black()
                            })
                            .text_color(if !self.search_mode {
                                cx.theme().primary
                            } else {
                                cx.theme().muted_foreground
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.search_mode = false;
                                this.tree_focus.focus(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("search-mode")
                            .small()
                            .label(if width < 220. {
                                "검색"
                            } else {
                                "전체 검색"
                            })
                            .h_full()
                            .flex_1()
                            .rounded_none()
                            .border_b(px(3.))
                            .border_color(if self.search_mode {
                                cx.theme().primary
                            } else {
                                gpui::transparent_black()
                            })
                            .text_color(if self.search_mode {
                                cx.theme().primary
                            } else {
                                cx.theme().muted_foreground
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.search_mode = true;
                                this.queue_search(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        tool(
                            "hide-sidebar",
                            IconName::ChevronsLeft,
                            "사이드바 숨기기 · Ctrl+\\",
                        )
                        .w(px(36.))
                        .h(px(36.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.session.sidebar_visible = false;
                            cx.notify();
                        })),
                    ),
            )
            .child(if self.search_mode {
                self.global_view(window, cx)
            } else {
                self.tree_view(cx)
            })
            .child(self.space_footer(cx))
            .into_any_element()
    }
    pub(in crate::desktop) fn tree_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let visible = Rc::new(self.visible_tree());
        let count = visible.len();
        let rows = visible.clone();
        let list = uniform_list(
            "note-tree",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| this.tree_row(index, &rows, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.tree_scroll)
        .size_full();
        let tree_bounds = self.tree_bounds.clone();
        let scroll = self.tree_scroll.0.borrow().base_handle.clone();
        let root_y = (scroll.bounds().top() - self.tree_bounds.get().top()
            + scroll.offset().y
            + px(count as f32 * TREE_ROW_HEIGHT + 4.))
        .min((self.tree_bounds.get().size.height - px(32.)).max(px(0.)));
        div()
            .id("tree-host")
            .debug_selector(|| "tree-host".into())
            .relative()
            .flex_1()
            .min_h_0()
            .pt_3()
            .px_2()
            .track_focus(&self.tree_focus)
            .child(
                canvas(move |bounds, _, _| tree_bounds.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .child(list)
            .when(
                self.drop_target.is_some_and(|t| t.zone == DropZone::Root),
                |view| {
                    view.child(
                        div()
                            .debug_selector(|| "tree-root-drop".into())
                            .absolute()
                            .left_2()
                            .right_2()
                            .top(root_y)
                            .h(px(29.))
                            .h_flex()
                            .justify_center()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().primary)
                            .bg(cx.theme().selection)
                            .text_color(cx.theme().primary)
                            .child("＋ 최상위로 이동"),
                    )
                },
            )
            .child(Scrollbar::vertical(&self.tree_scroll))
            .when(count == 0, |view| {
                view.child(
                    div()
                        .absolute()
                        .top_8()
                        .left_4()
                        .right_4()
                        .text_color(cx.theme().muted_foreground)
                        .child("Ctrl + N으로 첫 노트를 만드세요"),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.select_active_note();
                    this.tree_focus.focus(window, cx);
                    cx.notify();
                }),
            )
            .on_drop(cx.listener(|this, drag: &TreeDrag, window, cx| {
                this.finish_tree_drag(&drag.ids, window, cx);
                cx.stop_propagation();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.show_tree_background_menu(event.position, window, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }
    pub(in crate::desktop) fn tree_row(
        &mut self,
        index: usize,
        visible: &[(NoteId, usize)],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, depth) = visible[index];
        let note = self.book.get(id).unwrap();
        let title = note.title.clone();
        let folder = note.is_folder();
        let expanded = !self.collapsed.contains(&id);
        let selected = self.selection.ids.contains(&id);
        let active = self.session.active == Some(id);
        let context = self.context_note == Some(id);
        let multiple = selected && self.selection.ids.len() > 1;
        let row_background = if multiple {
            cx.theme().multi_selection
        } else {
            cx.theme().selection
        };
        let row_hover = if selected || active {
            row_background.blend(cx.theme().primary.opacity(0.08))
        } else {
            cx.theme().accent
        };
        // During closing the same title editor is already hosted by the tab bar.
        let renaming = self.session.sidebar_visible
            && self.rename.as_ref().is_some_and(|(note, _)| *note == id);
        let ids = if selected {
            self.book
                .selected_roots(&self.selection.ids.iter().copied().collect::<Vec<_>>())
        } else {
            vec![id]
        };
        let drag = TreeDrag { ids };
        let target = self.drop_target.filter(|target| target.anchor == Some(id));
        let weak = cx.entity().downgrade();
        let drag_view = weak.clone();
        div()
            .id(("note", id))
            .text_size(px(component::theme::TREE_FONT_SIZE))
            .debug_selector(move || format!("tree-note-{id}"))
            .h_flex()
            .relative()
            .h(px(TREE_ROW_HEIGHT))
            .w_full()
            .gap_1()
            .pr_2()
            .pl(px(4. + depth as f32 * TREE_INDENT))
            .rounded(px(4.))
            .border_1()
            .border_color(if context {
                cx.theme().primary
            } else {
                transparent_black()
            })
            .cursor_pointer()
            .when(selected || active, |row| row.bg(row_background))
            .when(!context && self.dragging_notes.is_empty(), |row| {
                row.hover(move |row| row.bg(row_hover))
            })
            .when(active || self.caret == Some(id), |row| {
                row.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(if active {
                            cx.theme().primary
                        } else {
                            cx.theme().muted_foreground
                        }),
                )
            })
            .when(
                target.is_some_and(|target| target.zone == DropZone::Inside),
                |row| {
                    row.child(
                        div()
                            .debug_selector(|| "tree-inside-drop".into())
                            .absolute()
                            .inset_0()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(cx.theme().primary),
                    )
                },
            )
            .child(
                div()
                    .id(("expand", id))
                    .debug_selector(move || format!("tree-expand-{id}"))
                    .w(px(16.))
                    .h(px(24.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .when(folder, |icon| {
                        icon.role(Role::Button)
                            .aria_label(if expanded { "접기" } else { "펼치기" })
                            .aria_expanded(expanded)
                            .rounded_sm()
                            .text_color(cx.theme().muted_foreground)
                            .hover(|style| {
                                style
                                    .bg(cx.theme().primary.opacity(0.18))
                                    .text_color(cx.theme().primary)
                            })
                            .active(|style| style.bg(cx.theme().primary.opacity(0.3)))
                            .child(
                                Icon::new(if expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size(px(14.)),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.set_collapsed(id, !this.collapsed.contains(&id), cx);
                                    cx.stop_propagation();
                                }),
                            )
                    }),
            )
            .child(
                Icon::new(if folder {
                    if expanded {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    }
                } else {
                    IconName::FileText
                })
                .size(px(16.))
                .flex_shrink_0()
                .text_color(if folder {
                    cx.theme().primary
                } else {
                    cx.theme().muted_foreground
                }),
            )
            .child(if renaming {
                Input::new(&self.title_input)
                    .small()
                    .h(px(27.))
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(title)
                    .into_any_element()
            })
            .when_some(
                target.filter(|target| target.zone != DropZone::Inside),
                |row, target| {
                    row.child(
                        div()
                            .debug_selector(|| "tree-insertion-marker".into())
                            .absolute()
                            .left(self.insertion_indent(target))
                            .right(px(4.))
                            .h(px(2.))
                            .bg(cx.theme().primary)
                            .when(target.zone == DropZone::Before, |line| line.top(px(1.)))
                            .when(target.zone != DropZone::Before, |line| line.bottom(px(1.)))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(-3.))
                                    .top(px(-2.))
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(cx.theme().primary),
                            ),
                    )
                },
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if !renaming {
                        this.tree_click(id, event.modifiers, window, cx);
                    }
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.pressed == Some(id) {
                    this.selection.single(id);
                    this.pressed = None;
                    if folder {
                        this.set_collapsed(id, !this.collapsed.contains(&id), cx);
                    }
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.show_tree_menu(id, event.position, window, cx);
                    cx.stop_propagation();
                }),
            )
            .when(!renaming, |row| {
                row.on_drag(drag, move |drag, _, window, cx| {
                    let _ = drag_view.update(cx, |this, cx| {
                        this.tree_drag_at(window.mouse_position(), &drag.ids, cx)
                    });
                    cx.new(|_| TreeDragPreview)
                })
            })
            .on_drop(cx.listener(move |this, drag: &TreeDrag, window, cx| {
                this.finish_tree_drag(&drag.ids, window, cx);
                cx.stop_propagation();
            }))
            .into_any_element()
    }
}
