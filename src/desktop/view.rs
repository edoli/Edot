use super::*;
use component::scroll::Scrollbar;

#[derive(Clone)]
struct TreeDrag {
    ids: Vec<NoteId>,
}
struct TreeDragPreview;
impl Render for TreeDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.))
    }
}
#[derive(Clone)]
struct TabDrag {
    id: NoteId,
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
    Button::new(id).small().icon(icon).tooltip(tip)
}

mod editor;
mod preferences;
mod search;
mod spaces;
mod tabs;
mod tree;

impl Render for Edot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (sidebar_progress, sidebar_moving) = self.sidebar_motion.tick(
            self.session.sidebar_visible,
            cx.background_executor().now(),
            cx.reduce_motion(),
        );
        if sidebar_moving {
            window.request_animation_frame();
        }
        if !self.session.sidebar_visible {
            self.resizing = None;
        }
        let sidebar_full_width = sidebar_width(
            self.session.sidebar_width,
            f32::from(window.viewport_size().width),
        ) + SIDEBAR_RESIZE_WIDTH;
        let sidebar_extent = sidebar_full_width * sidebar_progress;
        if !self.dragging_notes.is_empty() && cx.has_active_drag() {
            let cursor = if self.drop_target.is_some() {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OperationNotAllowed
            };
            if cx.active_drag_cursor_style() != Some(cursor) {
                cx.set_active_drag_cursor_style(cursor, window);
            }
        }
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
            .child(self.tabs_view(sidebar_extent, window, cx))
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
            .font_family(component::theme::UI_FONT)
            .text_size(px(component::theme::UI_FONT_SIZE))
            .capture_key_down(cx.listener(Self::key_down))
            .on_drag_move::<TreeDrag>(cx.listener(
                |this, event: &DragMoveEvent<TreeDrag>, _, cx| {
                    let ids = event.drag(cx).ids.clone();
                    this.tree_drag_at(event.event.position, &ids, cx);
                },
            ))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.stop_drag();
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.resizing = None;
                    this.stop_drag();
                    cx.notify();
                }),
            );
        if sidebar_extent > 0. {
            let sidebar = self.sidebar(window, cx);
            root = root.child(
                div()
                    .id("sidebar-clip")
                    .relative()
                    .w(px(sidebar_extent))
                    .h_full()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .h_flex()
                            .w(px(sidebar_full_width))
                            .h_full()
                            .flex_shrink_0()
                            .ml(px(sidebar_extent - sidebar_full_width))
                            .child(sidebar)
                            .child(
                                div()
                                    .w(px(SIDEBAR_RESIZE_WIDTH))
                                    .h_full()
                                    .flex_shrink_0()
                                    .bg(cx.theme().sidebar),
                            ),
                    )
                    .when(sidebar_moving, |clip| {
                        clip.child(div().absolute().inset_0().occlude())
                    }),
            );
        }
        root = root.child(main);
        if sidebar_extent > 0. {
            let edge = sidebar_extent;
            let enabled = self.session.sidebar_visible && !sidebar_moving;
            let dragging = self.resizing.is_some();
            let view = cx.entity().downgrade();
            let entity_id = cx.entity_id();
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
                                let hovered = enabled && hitbox.is_hovered(window);
                                let cursor = if enabled {
                                    CursorStyle::ResizeColumn
                                } else {
                                    CursorStyle::Arrow
                                };
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
                                let move_hitbox = hitbox.clone();
                                window.on_mouse_event(
                                    move |_: &MouseMoveEvent, phase, window, cx| {
                                        if enabled
                                            && phase == DispatchPhase::Capture
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
                                        if !enabled
                                            || phase != DispatchPhase::Bubble
                                            || e.button != MouseButton::Left
                                            || !hitbox.is_hovered(window)
                                        {
                                            return;
                                        }
                                        let _ = view.update(cx, |this, cx| {
                                            let width = sidebar_width(
                                                this.session.sidebar_width,
                                                f32::from(window.viewport_size().width),
                                            );
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
                            let max = sidebar_max_width(f32::from(window.viewport_size().width));
                            this.session.sidebar_width = (width
                                + f32::from(event.position.x - start) as i32)
                                .clamp(crate::session::MIN_SIDEBAR_WIDTH, max);
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
                        .anchor(self.menu_anchor)
                        .snap_to_window()
                        .child(menu.clone()),
                )
                .with_priority(1),
            );
        }
        if let Some(help) = &self.shortcut_help {
            root = root.child(help.clone());
        }
        if self.space_ui.open
            && self.session.sidebar_visible
            && self.error.is_none()
            && !window.has_active_dialog(cx)
        {
            root = root.child(deferred(self.space_popup(window, cx)).with_priority(5));
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
