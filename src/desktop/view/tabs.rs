//! Tabs presentation.
use super::*;
use crate::shortcuts::{Command, tip};

impl Edot {
    pub(in crate::desktop) fn tabs_view(
        &mut self,
        sidebar_extent: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sidebar_width = sidebar_extent
            + if self.session.sidebar_visible {
                0.
            } else {
                36.
            };
        let inline_controls = component::has_inline_window_controls(window);
        let controls_width = if inline_controls {
            component::WINDOW_CONTROLS_WIDTH + WINDOW_DRAG_WIDTH
        } else {
            0.
        };
        // Reserve 40px each for Settings and the trailing new-note button.
        let available =
            (f32::from(window.viewport_size().width) - sidebar_width - 80. - controls_width)
                .max(1.);
        let layout = crate::tabs::TabLayout::new(
            available as i32,
            self.open.len(),
            TAB_MIN_WIDTH,
            TAB_PREFERRED_WIDTH,
            0,
        );
        let width = px(layout.width as f32);
        self.tab_width = width;
        let tabs_extent = (width * self.open.len() as f32).min(px(available));
        let now = cx.background_executor().now();
        let reduced = cx.reduce_motion();
        let drag_layout = self.preview_tab_drag(now, reduced);
        self.tick_tab_slides(now, reduced, window);
        let mut floating_tab = None;
        let mut strip = div()
            .id("tabs-scroll")
            .h_flex()
            .h_full()
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll);
        for slot in 0..self.open.len() {
            let index = drag_layout.map_or(slot, |(movement, _)| movement.index_at(slot));
            let floating_left =
                drag_layout.and_then(|(movement, left)| (slot == movement.to).then_some(left));
            let id = self.open[index].id;
            let slide = self.tab_slide_offset(id, now);
            let row = self.tab_row(index, width * slot as f32 + slide, cx);
            if let Some(left) = floating_left.or((slide != px(0.)).then_some(slide)) {
                let moving = div()
                    .absolute()
                    .left(left)
                    .top_0()
                    .w(width)
                    .h_full()
                    .when(self.session.active != Some(id), |tab| {
                        // Cover overlapping labels without hiding the strip's bottom border.
                        tab.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top_0()
                                .bottom(px(1.))
                                .bg(cx.theme().bar),
                        )
                    })
                    .child(row);
                let placeholder = div().relative().w(width).h_full().flex_shrink_0();
                // Keep the scroll bounds tied to the slot while the tab moves across it.
                strip = strip.child(if floating_left.is_some() {
                    floating_tab = Some(moving);
                    placeholder
                } else {
                    placeholder.child(moving)
                });
            } else {
                strip = strip.child(row);
            }
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
                        tip(Command::Sidebar, "사이드바 표시", cx),
                    )
                    .w(px(36.))
                    .h(px(34.))
                    .flex_shrink_0()
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
                    .w(tabs_extent)
                    .flex_shrink_0()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(strip)
                    .children(floating_tab)
                    .child(Scrollbar::horizontal(&self.tab_scroll)),
            )
            .child(
                tool(
                    "new-note",
                    IconName::Plus,
                    tip(Command::NewNote, "새 노트", cx),
                )
                .w(px(40.))
                .h(px(34.))
                .flex_shrink_0()
                .on_click(cx.listener(|this, _, window, cx| this.create_note(None, window, cx))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when(cfg!(target_os = "windows"), |area| {
                        area.window_control_area(WindowControlArea::Drag)
                    }),
            )
            .child(
                tool(
                    "preferences",
                    IconName::Settings,
                    tip(Command::Settings, "설정", cx),
                )
                .w(px(40.))
                .h(px(34.))
                .flex_shrink_0()
                .selected(self.settings_visible)
                .on_click(cx.listener(|this, _, window, cx| this.show_settings(window, cx))),
            )
            .when(inline_controls, |row| {
                row.child(
                    div()
                        .w(px(WINDOW_DRAG_WIDTH))
                        .h_full()
                        .flex_shrink_0()
                        .window_control_area(WindowControlArea::Drag),
                )
                .child(component::WindowControls)
            })
            .into_any_element()
    }

    fn tab_row(&self, index: usize, content_left: Pixels, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = self.open[index].id;
        let title = self
            .book
            .get(id)
            .map_or("노트", |note| note.title.as_str())
            .to_owned();
        let tooltip = title.clone();
        let active = self.session.active == Some(id);
        let renaming = !self.session.sidebar_visible
            && self.rename.as_ref().is_some_and(|(note, _)| *note == id);
        let weak = cx.entity().downgrade();
        let drag_owner = weak.clone();
        div()
            .id(("tab", id))
            .block_mouse_except_scroll()
            .text_size(px(component::theme::TAB_FONT_SIZE))
            .debug_selector(move || {
                if active {
                    "active-tab".into()
                } else {
                    format!("tab-{id}")
                }
            })
            .h_flex()
            .relative()
            .w(self.tab_width)
            .h(px(36.))
            .flex_shrink_0()
            .px_2()
            .gap_2()
            .cursor_pointer()
            .whitespace_nowrap()
            .child(
                div()
                    .absolute()
                    .right_0()
                    .top(px(0.))
                    .bottom(px(0.))
                    .w(px(1.))
                    .bg(cx.theme().border),
            )
            .when(active, |row| {
                row.bg(cx.theme().background)
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .absolute()
                            .top(px(1.))
                            .left(px(-1.))
                            .right(px(0.))
                            .h(px(2.))
                            .bg(cx.theme().primary),
                    )
            })
            .when(!active, |row| {
                row.hover(|row| row.bg(cx.theme().accent))
                    // A slide can move the tab under a stationary pointer.
                    .on_hover(|_, window, _| window.refresh())
            })
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
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
                    .child(title)
                    .into_any_element()
            })
            .child(
                div()
                    .id(("close-tab", id))
                    .debug_selector(move || format!("close-tab-{id}"))
                    .block_mouse_except_scroll()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(20.))
                    .rounded_sm()
                    .hover(|s| s.bg(cx.theme().selection).text_color(cx.theme().primary))
                    .on_hover(|_, window, _| window.refresh())
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, _| {
                    let left =
                        this.tab_scroll.bounds().left() + this.tab_scroll.offset().x + content_left;
                    this.tab_grab = (event.position.x - left).clamp(px(0.), this.tab_width);
                }),
            )
            .on_drag(TabDrag, move |_, _, window, cx| {
                let _ = drag_owner.update(cx, |this, cx| {
                    this.begin_tab_drag(id, window.mouse_position(), cx);
                });
                cx.new(|_| EmptyDragPreview)
            })
            .context_menu(move |menu, _, cx| menus::tab_menu(menu, weak.clone(), id, index, cx))
    }
}
