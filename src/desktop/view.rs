use super::*;
use component::scroll::Scrollbar;

#[derive(Clone)]
struct TreeDrag {
    ids: Vec<u64>,
    title: String,
}
#[derive(Clone)]
struct TabDrag {
    id: u64,
    title: String,
}
struct DragLabel(String);
impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .child(self.0.clone())
    }
}
fn tool(id: &'static str, icon: IconName, tip: &'static str) -> Button {
    Button::new(id).ghost().small().icon(icon).tooltip(tip)
}

impl Edot {
    pub(super) fn sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let width = (self.session.sidebar_width as f32)
            .min(f32::from(window.viewport_size().width) - 220.)
            .max(150.);
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
                            .ghost()
                            .small()
                            .label(if width < 220. {
                                "메모"
                            } else {
                                "메모 목록"
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
                            .ghost()
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
                            "사이드바 숨기기 · Ctrl+B",
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
            .into_any_element()
    }
    pub(super) fn tree_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
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
        div()
            .id("tree-host")
            .relative()
            .flex_1()
            .min_h_0()
            .pt_3()
            .px_2()
            .track_focus(&self.tree_focus)
            .child(list)
            .child(Scrollbar::vertical(&self.tree_scroll))
            .when(count == 0, |view| {
                view.child(
                    div()
                        .absolute()
                        .top_8()
                        .left_4()
                        .right_4()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Ctrl + N으로 첫 메모를 만드세요"),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.selection = Selection::default();
                    this.caret = None;
                    this.tree_focus.focus(window, cx);
                    cx.notify();
                }),
            )
            .on_drag_move::<TreeDrag>(cx.listener(
                |this, event: &DragMoveEvent<TreeDrag>, _, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }
                    let ids = event.drag(cx).ids.clone();
                    this.tree_drag_at(event.event.position, &ids, cx);
                },
            ))
            .on_drop(cx.listener(|this, drag: &TreeDrag, _, cx| {
                if let Some(target) = this.drop_target {
                    this.move_notes(&drag.ids, target, cx);
                }
                this.stop_drag();
                cx.stop_propagation();
            }))
            .into_any_element()
    }
    pub(super) fn tree_row(
        &mut self,
        index: usize,
        visible: &[(u64, usize)],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, depth) = visible[index];
        let note = self.book.get(id).unwrap();
        let title = note.title.clone();
        let tooltip = title.clone();
        let folder = !self.book.children(id).is_empty();
        let expanded = !self.collapsed.contains(&id);
        let selected = self.selection.ids.contains(&id);
        let renaming = self.rename.as_ref().is_some_and(|(note, _)| *note == id);
        let ids = if selected {
            self.book
                .selected_roots(&self.selection.ids.iter().copied().collect::<Vec<_>>())
        } else {
            vec![id]
        };
        let drag = TreeDrag {
            title: if ids.len() > 1 {
                format!("{}개 메모", ids.len())
            } else {
                title.clone()
            },
            ids,
        };
        let target = self.drop_target.filter(|target| target.anchor == Some(id));
        let weak = cx.entity().downgrade();
        div()
            .id(("note", id))
            .h_flex()
            .relative()
            .h(px(29.))
            .w_full()
            .gap_1()
            .pr_2()
            .pl(px(4. + depth as f32 * 22.))
            .rounded(px(4.))
            .text_sm()
            .cursor_pointer()
            .when(selected, |row| row.bg(cx.theme().selection))
            .when(!selected, |row| row.hover(|row| row.bg(cx.theme().accent)))
            .when(self.caret == Some(id), |row| {
                row.border_l_2().border_color(cx.theme().primary)
            })
            .when(
                target.is_some_and(|target| target.zone == DropZone::Inside),
                |row| row.border_1().border_color(cx.theme().primary),
            )
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .child(
                div()
                    .id(("expand", id))
                    .w(px(16.))
                    .h(px(24.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .when(folder, |icon| {
                        icon.child(
                            Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(px(14.)),
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if folder {
                                if !this.collapsed.insert(id) {
                                    this.collapsed.remove(&id);
                                }
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }),
                    ),
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
                            .absolute()
                            .left_0()
                            .right_0()
                            .h(px(2.))
                            .bg(cx.theme().primary)
                            .when(target.zone == DropZone::Before, |line| line.top_0())
                            .when(target.zone != DropZone::Before, |line| line.bottom_0()),
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
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if this.pressed == Some(id) {
                        this.selection.single(id);
                        this.pressed = None;
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, window, cx| {
                    if !this.selection.ids.contains(&id) {
                        this.selection.single(id);
                    }
                    this.caret = Some(id);
                    this.tree_focus.focus(window, cx);
                    cx.notify();
                }),
            )
            .on_drag(drag, |drag, _, _, cx| {
                cx.new(|_| DragLabel(drag.title.clone()))
            })
            .on_drag_move::<TreeDrag>(cx.listener(
                move |this, event: &DragMoveEvent<TreeDrag>, _, cx| {
                    let ids = event.drag(cx).ids.clone();
                    this.tree_drag_at(event.event.position, &ids, cx);
                    cx.stop_propagation();
                },
            ))
            .on_drop(cx.listener(move |this, drag: &TreeDrag, _, cx| {
                if let Some(target) = this.drop_target {
                    this.move_notes(&drag.ids, target, cx);
                }
                this.stop_drag();
                cx.stop_propagation();
            }))
            .context_menu(move |mut menu, _, cx| {
                let Some(view) = weak.upgrade() else {
                    return menu;
                };
                let this = view.read(cx);
                let multiple = this.selection.ids.len() > 1;
                let nested = this.book.get(id).is_some_and(|note| note.parent != 0);
                if !multiple {
                    let weak = weak.clone();
                    menu = menu.item(PopupMenuItem::new("하위 메모 추가").on_click(
                        move |_, window, cx| {
                            let _ =
                                weak.update(cx, |this, cx| this.create_note(Some(id), window, cx));
                        },
                    ));
                    let weak = view.downgrade();
                    menu = menu.item(PopupMenuItem::new("이름 바꾸기    F2").on_click(
                        move |_, window, cx| {
                            let _ = weak.update(cx, |this, cx| this.begin_rename(id, window, cx));
                        },
                    ));
                    let weak = view.downgrade();
                    menu = menu.item(
                        PopupMenuItem::new("한 단계 위로")
                            .disabled(!nested)
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |this, cx| this.outdent(id, cx));
                            }),
                    );
                }
                let weak = view.downgrade();
                menu = menu.separator().item(PopupMenuItem::new("삭제").on_click(
                    move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| this.confirm_delete(window, cx));
                    },
                ));
                if multiple {
                    let weak = view.downgrade();
                    menu = menu.item(
                        PopupMenuItem::new("선택한 항목과 함께 새 폴더 만들기").on_click(
                            move |_, window, cx| {
                                let _ = weak.update(cx, |this, cx| this.group_selected(window, cx));
                            },
                        ),
                    );
                }
                menu
            })
            .into_any_element()
    }
    pub(super) fn tabs_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let sidebar_width = if self.session.sidebar_visible {
            (self.session.sidebar_width as f32)
                .min(f32::from(window.viewport_size().width) - 220.)
                .max(150.)
                + SIDEBAR_RESIZE_WIDTH
        } else {
            36.
        };
        let available = (f32::from(window.viewport_size().width) - sidebar_width - 80.).max(1.);
        let layout =
            crate::tabs::TabLayout::new(available as i32, self.open.len(), TAB_MIN_WIDTH, 180, 0);
        self.tab_width = layout.width;
        let mut strip = div()
            .id("tabs-scroll")
            .h_flex()
            .h_full()
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll);
        for (index, tab) in self.open.iter().enumerate() {
            let id = tab.id;
            let title = self
                .book
                .get(id)
                .map_or("메모", |note| note.title.as_str())
                .to_owned();
            let tooltip = title.clone();
            let active = self.session.active == Some(id);
            let renaming = !self.session.sidebar_visible
                && self.rename.as_ref().is_some_and(|(note, _)| *note == id);
            let weak = cx.entity().downgrade();
            strip = strip.child(
                div()
                    .id(("tab", id))
                    .debug_selector(move || {
                        if active {
                            "active-tab".into()
                        } else {
                            format!("tab-{id}")
                        }
                    })
                    .h_flex()
                    .relative()
                    .w(px(layout.width as f32))
                    .h(px(36.))
                    .flex_shrink_0()
                    .px_2()
                    .gap_2()
                    .cursor_pointer()
                    .text_sm()
                    .whitespace_nowrap()
                    .when(active, |row| {
                        row.bg(cx.theme().background)
                            .border_l_1()
                            .border_r_1()
                            .border_t_1()
                            .rounded_t(px(6.))
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .absolute()
                                    .top(px(1.))
                                    .left(px(4.))
                                    .right(px(4.))
                                    .h(px(2.))
                                    .bg(cx.theme().primary),
                            )
                    })
                    .when(!active, |row| {
                        row.hover(|row| row.bg(cx.theme().accent)).child(
                            div()
                                .absolute()
                                .right_0()
                                .top(px(7.))
                                .bottom(px(7.))
                                .w(px(1.))
                                .bg(cx.theme().border),
                        )
                    })
                    .when(self.tab_drop == Some(index), |row| {
                        row.child(
                            div()
                                .absolute()
                                .left_0()
                                .top(px(3.))
                                .bottom(px(3.))
                                .w(px(2.))
                                .bg(cx.theme().primary),
                        )
                    })
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .child(
                        Icon::new(IconName::FileText)
                            .size(px(15.))
                            .text_color(if active {
                                cx.theme().primary
                            } else {
                                cx.theme().muted_foreground
                            }),
                    )
                    .child(if renaming {
                        Input::new(&self.title_input)
                            .small()
                            .h(px(28.))
                            .into_any_element()
                    } else {
                        div()
                            .debug_selector(move || {
                                if active {
                                    "active-tab-title".into()
                                } else {
                                    format!("tab-title-{id}")
                                }
                            })
                            .flex_1()
                            .min_w_0()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(title.clone())
                            .into_any_element()
                    })
                    .when(tab.dirty, |row| {
                        row.child(
                            div()
                                .size(px(5.))
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(cx.theme().primary),
                        )
                    })
                    .child(
                        div()
                            .id(("close-tab", id))
                            .debug_selector(move || format!("close-tab-{id}"))
                            .occlude()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(20.))
                            .rounded_sm()
                            .hover(|s| s.bg(cx.theme().selection).text_color(cx.theme().primary))
                            .active(|s| s.bg(cx.theme().primary.opacity(0.3)))
                            .child(Icon::new(IconName::Close).size(px(13.)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_tabs(id, 0, window, cx);
                                cx.stop_propagation();
                            })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_note(id, true, window, cx);
                    }))
                    .on_drag(TabDrag { id, title }, |drag, _, _, cx| {
                        cx.new(|_| DragLabel(drag.title.clone()))
                    })
                    .on_drag_move::<TabDrag>(cx.listener(
                        move |this, event: &DragMoveEvent<TabDrag>, _, cx| {
                            this.tab_drag_at(event.event.position, cx);
                        },
                    ))
                    .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
                        this.reorder_tab(drag.id, this.tab_drop.unwrap_or(index), cx);
                        this.stop_drag();
                        cx.stop_propagation();
                    }))
                    .context_menu(move |menu, _, cx| {
                        let Some(view) = weak.upgrade() else {
                            return menu;
                        };
                        let count = view.read(cx).open.len();
                        let mut menu = menu;
                        for (mode, label, disabled) in [
                            (0, "탭 닫기    Ctrl+W", false),
                            (1, "다른 탭 닫기", count <= 1),
                            (2, "오른쪽 탭들 닫기", index + 1 >= count),
                        ] {
                            let weak = view.downgrade();
                            menu =
                                menu.item(PopupMenuItem::new(label).disabled(disabled).on_click(
                                    move |_, window, cx| {
                                        let _ = weak.update(cx, |this, cx| {
                                            this.close_tabs(id, mode, window, cx)
                                        });
                                    },
                                ));
                        }
                        menu
                    }),
            );
        }
        div()
            .relative()
            .h_flex()
            .h(px(36.))
            .flex_shrink_0()
            .bg(cx.theme().bar)
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(1.))
                    .bg(cx.theme().border),
            )
            .when(!self.session.sidebar_visible, |row| {
                row.child(
                    tool(
                        "show-sidebar",
                        IconName::ChevronsRight,
                        "사이드바 표시 · Ctrl+B",
                    )
                    .w(px(36.))
                    .h(px(34.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.sidebar_visible = true;
                        cx.notify();
                    })),
                )
            })
            .child(
                div()
                    .relative()
                    .h_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(strip)
                    .child(Scrollbar::horizontal(&self.tab_scroll)),
            )
            .child(
                tool("new-note", IconName::Plus, "새 메모 · Ctrl+N")
                    .w(px(40.))
                    .h(px(34.))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.create_note(None, window, cx)),
                    ),
            )
            .child(
                tool("preferences", IconName::Settings, "설정 · Ctrl+,")
                    .w(px(40.))
                    .h(px(34.))
                    .selected(self.settings_visible)
                    .on_click(cx.listener(|this, _, window, cx| this.show_settings(window, cx))),
            )
            .into_any_element()
    }
    pub(super) fn search_fields(&self, global: bool, cx: &mut Context<Self>) -> AnyElement {
        let panel = if global { &self.global } else { &self.local };
        div()
            .v_flex()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .gap_1()
                    .child(Input::new(&panel.query).small().aria_label(if global {
                        "전체 메모 검색어"
                    } else {
                        "현재 메모 검색어"
                    }))
                    .child(
                        Button::new(if global { "global-case" } else { "local-case" })
                            .ghost()
                            .small()
                            .label("Aa")
                            .tooltip("대소문자 구분")
                            .selected(panel.case)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if global {
                                    this.global.case = !this.global.case;
                                    this.queue_search(window, cx);
                                } else {
                                    this.local.case = !this.local.case;
                                    this.find_next(false, true, window, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new(if global {
                            "global-regex"
                        } else {
                            "local-regex"
                        })
                        .ghost()
                        .small()
                        .label(".*")
                        .tooltip("정규식")
                        .selected(panel.regex)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if global {
                                    this.global.regex = !this.global.regex;
                                    this.queue_search(window, cx);
                                } else {
                                    this.local.regex = !this.local.regex;
                                    this.find_next(false, true, window, cx);
                                }
                            },
                        )),
                    ),
            )
            .when(panel.replace, |view| {
                view.child(
                    Input::new(&panel.replacement)
                        .small()
                        .aria_label("바꿀 내용"),
                )
            })
            .when(panel.replace, |view| {
                view.child(
                    div()
                        .h_flex()
                        .justify_end()
                        .gap_2()
                        .when(!global, |row| {
                            row.child(Button::new("replace-one").small().label("바꾸기").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.replace_local(false, window, cx)
                                }),
                            ))
                        })
                        .child(
                            Button::new(if global {
                                "replace-global-all"
                            } else {
                                "replace-local-all"
                            })
                            .small()
                            .label("모두 바꾸기")
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    if global {
                                        this.replace_global_all(window, cx);
                                    } else {
                                        this.replace_local(true, window, cx);
                                    }
                                },
                            )),
                        ),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(panel.message.clone()),
            )
            .into_any_element()
    }
    pub(super) fn local_view(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .occlude()
            .absolute()
            .right_3()
            .top_2()
            .w(px(440.))
            .max_w_full()
            .p_3()
            .v_flex()
            .gap_2()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .shadow_lg()
            .child(
                div()
                    .h_flex()
                    .gap_1()
                    .child(
                        Button::new("local-replace-toggle")
                            .ghost()
                            .small()
                            .icon(if self.local.replace {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.local.replace = !this.local.replace;
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1().text_sm().child(if self.local.replace {
                        "찾기 및 바꾸기"
                    } else {
                        "찾기"
                    }))
                    .child(
                        tool("local-prev", IconName::ChevronUp, "이전 · Shift+F3").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.find_next(true, false, window, cx)
                            }),
                        ),
                    )
                    .child(
                        tool("local-next", IconName::ChevronDown, "다음 · F3").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.find_next(false, false, window, cx)
                            }),
                        ),
                    )
                    .child(tool("local-close", IconName::Close, "닫기 · Esc").on_click(
                        cx.listener(|this, _, window, cx| {
                            this.find_visible = false;
                            this.focus_editor(window, cx);
                            cx.notify();
                        }),
                    )),
            )
            .child(self.search_fields(false, cx))
            .into_any_element()
    }
    pub(super) fn global_view(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let list = uniform_list(
            "global-results",
            self.result_rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| this.result_row(index, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.result_scroll)
        .size_full();
        div()
            .v_flex()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .p_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .h_flex()
                            .gap_1()
                            .child(
                                tool(
                                    "global-replace-toggle",
                                    if self.global.replace {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    },
                                    "바꾸기 표시",
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.global.replace = !this.global.replace;
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(div().flex_1())
                            .child(
                                tool("global-prev", IconName::ChevronUp, "이전 결과").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.global_next(true, window, cx)
                                    }),
                                ),
                            )
                            .child(
                                tool("global-next", IconName::ChevronDown, "다음 결과").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.global_next(false, window, cx)
                                    }),
                                ),
                            )
                            .child(
                                tool("global-refresh", IconName::RefreshCw, "새로 고침").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.queue_search(window, cx)
                                    }),
                                ),
                            )
                            .child(
                                tool(
                                    "global-collapse",
                                    IconName::ChevronsUpDown,
                                    "모두 접기 / 펼치기",
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        if this.result_collapsed.len() == this.rows.len() {
                                            this.result_collapsed.clear();
                                        } else {
                                            this.result_collapsed =
                                                this.rows.iter().map(|r| r.id).collect();
                                        }
                                        this.rebuild_results();
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(tool("global-close", IconName::Close, "검색 닫기").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.search_mode = false;
                                    this.tree_focus.focus(window, cx);
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(self.search_fields(true, cx)),
            )
            .child(
                div()
                    .id("result-host")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .track_focus(&self.results_focus)
                    .child(list)
                    .child(Scrollbar::vertical(&self.result_scroll)),
            )
            .into_any_element()
    }
    pub(super) fn result_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let row = self.result_rows[index];
        let group = &self.rows[row.group];
        let id = group.id;
        let selected = self.result_cursor == Some(index);
        let mut item = div()
            .id(("result", index))
            .h_flex()
            .h(px(global_search::ROW_HEIGHT as f32))
            .w_full()
            .px_2()
            .gap_1()
            .text_xs()
            .cursor_pointer()
            .when(selected, |row| row.bg(cx.theme().selection))
            .when(!selected, |row| row.hover(|s| s.bg(cx.theme().accent)))
            .on_click(
                cx.listener(move |this, _, window, cx| this.activate_result(index, window, cx)),
            );
        if let Some(hit) = row.hit {
            let result = &group.matches[hit];
            let range = result.range;
            let line = result.line;
            let column = result.column;
            let key = MatchKey { id, range };
            let h = &result.highlight;
            let preview = result.preview.to_string();
            item = item
                .child(
                    div()
                        .w(px(29.))
                        .flex_shrink_0()
                        .text_right()
                        .text_color(cx.theme().muted_foreground)
                        .child(result.line.to_string()),
                )
                .child(
                    div()
                        .h_flex()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(preview[..h.start].to_owned())
                        .child(
                            div()
                                .bg(cx.theme().selection)
                                .text_color(cx.theme().primary)
                                .child(if h.is_empty() {
                                    "│".to_owned()
                                } else {
                                    preview[h.clone()].to_owned()
                                }),
                        )
                        .child(preview[h.end..].to_owned()),
                )
                .tooltip(move |window, cx| {
                    Tooltip::new(format!("{line}:{column}  {preview}")).build(window, cx)
                });
            if self.global.replace {
                item = item.child(
                    Button::new(("replace-hit", index))
                        .ghost()
                        .xsmall()
                        .icon(IconName::ArrowRight)
                        .tooltip("이 결과 바꾸기")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.replace_result(key, window, cx);
                            cx.stop_propagation();
                        })),
                );
            }
        } else {
            let title = self
                .book
                .get(id)
                .map_or("메모", |n| n.title.as_str())
                .to_owned();
            item = item
                .child(
                    Icon::new(if self.result_collapsed.contains(&id) {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    })
                    .size(px(13.)),
                )
                .child(Icon::new(IconName::FileText).size(px(14.)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .overflow_hidden()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(group.count.to_string()),
                );
        }
        item.into_any_element()
    }
    pub(super) fn settings_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let prefs = self.preferences.as_ref().unwrap();
        div()
            .id("settings-panel")
            .debug_selector(|| "settings-panel".into())
            .track_focus(&prefs.focus)
            .occlude()
            .absolute()
            .top_2()
            .right_3()
            .w(px(360.))
            .max_w_full()
            .v_flex()
            .gap_5()
            .p_5()
            .text_left()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .shadow_lg()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.settings_visible = false;
                    this.focus_editor(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(
                div()
                    .h_flex()
                    .child(
                        div()
                            .flex_1()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("설정"),
                    )
                    .child(
                        tool("settings-close", IconName::Close, "설정 닫기").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.settings_visible = false;
                                this.focus_editor(window, cx);
                                cx.notify();
                            },
                        )),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(div().text_sm().child("앱 테마"))
                    .child(
                        Select::new(&prefs.theme)
                            .w_full()
                            .accessibility_label("테마"),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .debug_selector(|| "settings-font".into())
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_sm().child("본문 글꼴"))
                            .child(
                                Select::new(&prefs.font)
                                    .w_full()
                                    .search_placeholder("글꼴 검색")
                                    .accessibility_label("본문 글꼴"),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "settings-size".into())
                            .v_flex()
                            .w(px(92.))
                            .flex_shrink_0()
                            .gap_1()
                            .child(div().text_sm().child("글자 크기"))
                            .child(
                                Select::new(&prefs.size)
                                    .w_full()
                                    .accessibility_label("글자 크기"),
                            ),
                    ),
            )
            .child(
                Switch::new("word-wrap")
                    .track_focus(&prefs.wrap_focus)
                    .label("자동 줄 바꿈")
                    .checked(self.settings.wrap)
                    .on_change(cx.listener(|this, value, window, cx| {
                        let mut next = this.settings.clone();
                        next.wrap = *value;
                        this.change_settings(next, window, cx);
                    })),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("변경 사항은 바로 적용되고 저장됩니다."),
            )
            .into_any_element()
    }
    pub(super) fn body_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = self.active_editor() else {
            return div()
                .size_full()
                .v_flex()
                .justify_center()
                .items_center()
                .gap_3()
                .child(
                    Icon::new(IconName::FileText)
                        .size(px(40.))
                        .text_color(cx.theme().primary),
                )
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("생각을 가볍게 기록하세요"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("메모를 선택하거나 Ctrl + N으로 새 메모를 만드세요"),
                )
                .into_any_element();
        };
        let font_size = self.settings.font_size as f32 * self.zoom as f32 / 100.;
        let editor_menu = editor.clone();
        let weak = cx.entity().downgrade();
        let wheel_view = weak.clone();
        div()
            .id("body-editor")
            .debug_selector(|| "body-editor".into())
            .relative()
            .size_full()
            .min_h_0()
            .font_family(self.settings.font.clone())
            .text_size(px(font_size))
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |bounds, _, window, _| {
                        let weak = wheel_view.clone();
                        window.on_mouse_event(
                            move |event: &ScrollWheelEvent, phase, window, cx| {
                                if phase != DispatchPhase::Capture
                                    || !event.modifiers.control
                                    || !bounds.contains(&event.position)
                                    || window.has_active_dialog(cx)
                                {
                                    return;
                                }
                                let _ = weak.update(cx, |this, cx| {
                                    let panel_width = if this.settings_visible {
                                        372.
                                    } else if this.find_visible {
                                        452.
                                    } else {
                                        0.
                                    };
                                    if this.error.is_some()
                                        || panel_width > 0.
                                            && event.position.x > bounds.right() - px(panel_width)
                                            && event.position.y < bounds.top() + px(340.)
                                    {
                                        return;
                                    }
                                    let delta = event.delta.pixel_delta(px(20.)).y;
                                    if delta > px(0.) {
                                        this.zoom = (this.zoom + 10).min(500);
                                    } else if delta < px(0.) {
                                        this.zoom = this.zoom.saturating_sub(10).max(10);
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                });
                            },
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                // Keep the gutter outside the scrollable editor and its clip.
                div()
                    .debug_selector(|| "body-text-viewport".into())
                    .absolute()
                    .left(px(16.))
                    .right(px(4.))
                    .top(px(6.))
                    .bottom_0()
                    .overflow_hidden()
                    .child(
                        Textarea::new(&editor)
                            .h(relative(1.))
                            .appearance(false)
                            .font_family(self.settings.font.clone())
                            .text_size(px(font_size))
                            .p_0()
                            .aria_label("메모 본문"),
                    ),
            )
            .context_menu(move |mut menu, _, _| {
                for (label, action) in [
                    (
                        "실행 취소    Ctrl+Z",
                        Box::new(component::input::Undo) as Box<dyn Action>,
                    ),
                    ("다시 실행    Ctrl+Y", Box::new(component::input::Redo)),
                    ("잘라내기    Ctrl+X", Box::new(component::input::Cut)),
                    ("복사    Ctrl+C", Box::new(component::input::Copy)),
                    ("붙여넣기    Ctrl+V", Box::new(component::input::Paste)),
                    ("전체 선택    Ctrl+A", Box::new(component::input::SelectAll)),
                ] {
                    let editor = editor_menu.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        editor.update(cx, |state, cx| state.focus(window, cx));
                        window.dispatch_action(action.boxed_clone(), cx);
                    }));
                }
                menu = menu.separator();
                for (label, replace) in [("찾기    Ctrl+F", false), ("바꾸기    Ctrl+H", true)]
                {
                    let weak = weak.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let _ =
                            weak.update(cx, |this, cx| this.show_find(false, replace, window, cx));
                    }));
                }
                menu
            })
            .into_any_element()
    }
    pub(super) fn status_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut position = String::new();
        if let Some(editor) = self.active_editor() {
            let state = editor.read(cx);
            let tab = self
                .open
                .iter()
                .find(|tab| Some(tab.id) == self.session.active)
                .unwrap();
            let selected =
                editing::paragraph_units_iter(state.value()[state.selected_range()].chars());
            // Input events are deferred; rendering can precede the cached metrics update.
            let current_metrics;
            let metrics = if tab.metrics_revision == state.revision {
                &tab.metrics
            } else {
                current_metrics = editing::TextMetrics::new(&state.value());
                &current_metrics
            };
            let (line, start) = metrics.line(state.cursor());
            let column =
                editing::paragraph_units_iter(state.value()[start..state.cursor()].chars()) + 1;
            position = format!(
                "Ln {line}, Col {column}   {}{}자",
                if selected > 0 {
                    format!("{selected}/")
                } else {
                    String::new()
                },
                metrics.total
            );
        }
        div()
            .h_flex()
            .h(px(32.))
            .px_3()
            .gap_3()
            .flex_shrink_0()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().bar)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(self.status.clone()),
            )
            .child(position)
            .child(
                Button::new("reset-zoom")
                    .ghost()
                    .xsmall()
                    .label(format!("{}%", self.zoom))
                    .tooltip("확대율 초기화 · Ctrl+0")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.zoom = 100;
                        cx.notify();
                    })),
            )
            .child("UTF-8 · Plain Text")
            .into_any_element()
    }
}

impl Render for Edot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(cx.theme().background)
            .child(self.body_view(cx));
        if self.find_visible {
            body = body.child(self.local_view(cx));
        }
        if self.settings_visible {
            body = body.child(self.settings_view(cx));
        }
        let main = div()
            .v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(self.tabs_view(window, cx))
            .child(body)
            .child(self.status_view(cx));
        let mut root = div()
            .id("edot")
            .relative()
            .h_flex()
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family("Segoe UI")
            .text_size(px(15.))
            .capture_key_down(cx.listener(Self::key_down))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.resizing = None;
                    this.stop_drag();
                    cx.notify();
                }),
            );
        if self.session.sidebar_visible {
            root = root.child(self.sidebar(window, cx)).child(
                div()
                    .w(px(SIDEBAR_RESIZE_WIDTH))
                    .h_full()
                    .flex_shrink_0()
                    .bg(cx.theme().sidebar),
            );
        }
        root = root.child(main);
        if self.session.sidebar_visible {
            let edge = (self.session.sidebar_width as f32)
                .min(f32::from(window.viewport_size().width) - 220.)
                .max(150.)
                + SIDEBAR_RESIZE_WIDTH;
            let dragging = self.resizing.is_some();
            let view = cx.entity().downgrade();
            let entity_id = cx.entity_id();
            #[cfg(test)]
            let probe = self.splitter_probe.clone();
            root = root.child(
                div()
                    .id("sidebar-resize")
                    .debug_selector(|| "sidebar-resize".into())
                    .absolute()
                    .left(px(edge - SIDEBAR_RESIZE_WIDTH / 2.))
                    .top_0()
                    .bottom_0()
                    .w(px(SIDEBAR_RESIZE_WIDTH))
                    // This marker has no style or hitbox of its own.
                    .child(
                        div()
                            .debug_selector(|| "sidebar-resize-line".into())
                            .absolute()
                            .left(px(SIDEBAR_RESIZE_WIDTH / 2. - 0.5))
                            .top_0()
                            .bottom_0()
                            .w(px(1.)),
                    )
                    .child(
                        canvas(
                            |bounds, window, _| {
                                window.insert_hitbox(bounds, HitboxBehavior::BlockMouse)
                            },
                            move |bounds, hitbox, window, cx| {
                                // One hitbox owns color, cursor, and mouse-down. Avoid
                                // a separate group-hover element and its hover cache.
                                let hovered = hitbox.is_hovered(window);
                                let cursor = CursorStyle::ResizeColumn;
                                window.set_cursor_style(cursor, &hitbox);
                                let highlighted = dragging || hovered;
                                let line = Bounds::new(
                                    point(bounds.center().x - px(0.5), bounds.top()),
                                    size(px(1.), bounds.size.height),
                                );
                                window.paint_quad(fill(
                                    line,
                                    if highlighted {
                                        cx.theme().primary
                                    } else {
                                        cx.theme().border
                                    },
                                ));
                                #[cfg(test)]
                                probe.replace(Some((hitbox.clone(), highlighted, cursor)));
                                let move_hitbox = hitbox.clone();
                                window.on_mouse_event(
                                    move |_: &MouseMoveEvent, phase, window, cx| {
                                        if phase == DispatchPhase::Capture
                                            && move_hitbox.is_hovered(window) != hovered
                                        {
                                            cx.notify(entity_id);
                                            window.refresh();
                                        }
                                    },
                                );
                                let view = view.clone();
                                window.on_mouse_event(
                                    move |e: &MouseDownEvent, phase, window, cx| {
                                        if phase != DispatchPhase::Bubble
                                            || e.button != MouseButton::Left
                                            || !hitbox.is_hovered(window)
                                        {
                                            return;
                                        }
                                        let _ = view.update(cx, |this, cx| {
                                            let width = (this.session.sidebar_width as f32)
                                                .min(f32::from(window.viewport_size().width) - 220.)
                                                .max(150.);
                                            this.resizing = Some((e.position.x, width as i32));
                                            cx.notify();
                                        });
                                        cx.stop_propagation();
                                    },
                                );
                            },
                        )
                        .absolute()
                        .size_full(),
                    ),
            );
        }
        if self.resizing.is_some() {
            root = root.child(
                div()
                    .id("sidebar-resize-capture")
                    .debug_selector(|| "sidebar-resize-capture".into())
                    .absolute()
                    .inset_0()
                    .occlude()
                    .cursor_col_resize()
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                        if let Some((start, width)) = this.resizing {
                            let max = (f32::from(window.viewport_size().width) as i32 - 220)
                                .clamp(150, 600);
                            this.session.sidebar_width = (width
                                + f32::from(event.position.x - start) as i32)
                                .clamp(150, max);
                            cx.notify();
                            cx.stop_propagation();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.resizing = None;
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.resizing = None;
                            cx.notify();
                        }),
                    ),
            );
        }
        if let Some(menu) = &self.keyboard_menu {
            root = root.child(
                deferred(
                    anchored()
                        .position(self.menu_position)
                        .snap_to_window()
                        .child(menu.clone()),
                )
                .with_priority(1),
            );
        }
        if let Some(error) = &self.error {
            root = root.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::black().opacity(0.35))
                    .occlude()
                    .child(
                        div()
                            .v_flex()
                            .w(px(460.))
                            .max_w_full()
                            .p_5()
                            .gap_4()
                            .rounded_lg()
                            .bg(cx.theme().background)
                            .border_1()
                            .border_color(cx.theme().border)
                            .shadow_lg()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Edot"))
                            .child(div().text_sm().child(error.clone()))
                            .child(
                                Button::new("dismiss-error")
                                    .primary()
                                    .label(if self.startup_failed {
                                        "닫기"
                                    } else {
                                        "확인"
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if this.startup_failed {
                                            window.remove_window();
                                        } else {
                                            this.error = None;
                                            cx.notify();
                                        }
                                    })),
                            ),
                    ),
            );
        }
        root
    }
}
