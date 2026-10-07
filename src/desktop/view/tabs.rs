//! Tabs presentation.
use super::*;

impl Edot {
    pub(in crate::desktop) fn tabs_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sidebar_width = if self.session.sidebar_visible {
            sidebar_width(
                self.session.sidebar_width,
                f32::from(window.viewport_size().width),
            ) + SIDEBAR_RESIZE_WIDTH
        } else {
            36.
        };
        let available = (f32::from(window.viewport_size().width) - sidebar_width - 80.).max(1.);
        let layout = crate::tabs::TabLayout::new(
            available as i32,
            self.open.len(),
            TAB_MIN_WIDTH,
            TAB_PREFERRED_WIDTH,
            0,
        );
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
                                this.close_tabs(id, crate::tabs::CloseMode::Current, window, cx);
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
                        menus::tab_menu(menu, weak.clone(), id, index, cx)
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
}
