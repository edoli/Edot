use super::*;
#[derive(Default)]
pub struct AutoScroll {
    pub last_drag_position: Option<Point<Pixels>>,
    task: Option<Task<()>>,
    delta: Option<Pixels>,
}
impl AutoScroll {
    pub fn compute_delta(value: Pixels, bounds: Bounds<Pixels>) -> Option<Pixels> {
        let edge = px(30.);
        if value < bounds.top() + edge {
            Some(-((bounds.top() + edge - value) * 0.25).clamp(px(1.), px(24.)))
        } else if value > bounds.bottom() - edge {
            Some(((value - bounds.bottom() + edge) * 0.25).clamp(px(1.), px(24.)))
        } else {
            None
        }
    }
    pub fn set<T: 'static>(
        &mut self,
        delta: Option<Pixels>,
        cx: &mut Context<T>,
        callback: impl Fn(Pixels, &mut T, &mut Context<T>) + 'static,
    ) {
        self.set_interval(delta, std::time::Duration::from_millis(16), cx, callback);
    }
    pub fn set_interval<T: 'static>(
        &mut self,
        delta: Option<Pixels>,
        interval: std::time::Duration,
        cx: &mut Context<T>,
        callback: impl Fn(Pixels, &mut T, &mut Context<T>) + 'static,
    ) {
        if self.delta == delta && self.task.is_some() {
            return;
        }
        self.task = None;
        self.delta = delta;
        if let Some(delta) = delta {
            self.task = Some(cx.spawn(async move |entity, cx| {
                loop {
                    cx.background_executor().timer(interval).await;
                    if entity.update(cx, |s, cx| callback(delta, s, cx)).is_err() {
                        break;
                    }
                }
            }));
        }
    }
    pub fn stop(&mut self) {
        self.task = None;
        self.delta = None;
        self.last_drag_position = None;
    }
}
#[derive(IntoElement)]
pub struct Scrollbar {
    handle: Handle,
    horizontal: bool,
}
#[derive(Clone)]
enum Handle {
    Scroll(ScrollHandle),
    List(ListState),
}
impl Handle {
    fn max_offset(&self) -> Point<Pixels> {
        match self {
            Self::Scroll(h) => h.max_offset(),
            Self::List(h) => h.max_offset_for_scrollbar(),
        }
    }
    fn offset(&self) -> Point<Pixels> {
        match self {
            Self::Scroll(h) => h.offset(),
            Self::List(h) => h.scroll_px_offset_for_scrollbar(),
        }
    }
    fn set_offset(&self, value: Point<Pixels>) {
        match self {
            Self::Scroll(h) => h.set_offset(value),
            Self::List(h) => h.set_offset_from_scrollbar(value),
        }
    }
    fn drag_started(&self) {
        if let Self::List(h) = self {
            h.scrollbar_drag_started();
        }
    }
    fn drag_ended(&self) {
        if let Self::List(h) = self {
            h.scrollbar_drag_ended();
        }
    }
}
impl Scrollbar {
    pub fn vertical_list(handle: &ListState) -> Self {
        Self {
            handle: Handle::List(handle.clone()),
            horizontal: false,
        }
    }
    pub fn vertical(handle: &UniformListScrollHandle) -> Self {
        Self::vertical_handle(&handle.0.borrow().base_handle)
    }
    pub fn vertical_handle(handle: &ScrollHandle) -> Self {
        Self {
            handle: Handle::Scroll(handle.clone()),
            horizontal: false,
        }
    }
    pub fn horizontal(handle: &ScrollHandle) -> Self {
        Self {
            handle: Handle::Scroll(handle.clone()),
            horizontal: true,
        }
    }
}
#[derive(Default)]
struct ScrollbarState {
    drag: Option<(Pixels, Pixels)>,
    hovered: bool,
}
impl RenderOnce for Scrollbar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let horizontal = self.horizontal;
        let id = if horizontal {
            "h-scrollbar"
        } else {
            "v-scrollbar"
        };
        let state = window.use_keyed_state(id, cx, |_, _| ScrollbarState::default());
        let handle = self.handle;
        let layout_handle = handle.clone();
        let layout_state = state.clone();
        div()
            .id(id)
            .debug_selector(move || id.into())
            .absolute()
            .when(horizontal, |d| d.bottom_0().left_0().w_full().h(px(10.)))
            .when(!horizontal, |d| d.right_0().top_0().h_full().w(px(10.)))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        // The sibling viewport has completed prepaint, so these metrics
                        // include this frame's layout and scroll offset.
                        let max = layout_handle.max_offset();
                        let extent = if horizontal { max.x } else { max.y };
                        if extent <= px(0.) {
                            return None;
                        }
                        let viewport = if horizontal {
                            bounds.size.width
                        } else {
                            bounds.size.height
                        };
                        let length = (viewport * (viewport / (viewport + extent)))
                            .max(px(22.))
                            .min(viewport);
                        let available = (viewport - length).max(px(1.));
                        let offset = layout_handle.offset();
                        let current = if horizontal { offset.x } else { offset.y };
                        let pos = available * (-current / extent);
                        let thumb = if horizontal {
                            Bounds::new(
                                point(bounds.left() + pos, bounds.top() + px(2.)),
                                size(length, px(6.)),
                            )
                        } else {
                            Bounds::new(
                                point(bounds.left() + px(2.), bounds.top() + pos),
                                size(px(6.), length),
                            )
                        };
                        let capture = layout_state.read(cx).drag.map(|_| {
                            window.insert_hitbox(
                                Bounds::new(point(px(0.), px(0.)), window.viewport_size()),
                                HitboxBehavior::BlockMouse,
                            )
                        });
                        let hitbox =
                            window.insert_hitbox(bounds, HitboxBehavior::BlockMouseExceptScroll);
                        Some((hitbox, capture, thumb, extent, available, current))
                    },
                    move |bounds, layout, window, cx| {
                        let Some((hitbox, capture, thumb, extent, available, current)) = layout
                        else {
                            return;
                        };
                        let hovered = hitbox.is_hovered(window);
                        state.update(cx, |s, _| s.hovered = hovered);
                        let active = state.read(cx).drag.is_some();
                        window.set_cursor_style(CursorStyle::Arrow, &hitbox);
                        if let Some(capture) = &capture {
                            window.set_cursor_style(CursorStyle::Arrow, capture);
                        }
                        if active || hovered {
                            window.paint_quad(fill(bounds, cx.theme().primary.opacity(0.1)));
                        }
                        window.paint_quad(quad(
                            thumb,
                            px(3.),
                            if active || hovered {
                                cx.theme().primary
                            } else {
                                cx.theme().muted_foreground.opacity(0.5)
                            },
                            px(0.),
                            transparent_black(),
                            BorderStyle::Solid,
                        ));
                        let down_state = state.clone();
                        let down_handle = handle.clone();
                        let down_hitbox = hitbox.clone();
                        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                            if phase != DispatchPhase::Bubble
                                || e.button != MouseButton::Left
                                || !down_hitbox.is_hovered(window)
                            {
                                return;
                            }
                            let at = if horizontal {
                                e.position.x
                            } else {
                                e.position.y
                            };
                            let start = if horizontal {
                                thumb.left()
                            } else {
                                thumb.top()
                            };
                            let length = if horizontal {
                                thumb.size.width
                            } else {
                                thumb.size.height
                            };
                            let origin = if horizontal {
                                bounds.left()
                            } else {
                                bounds.top()
                            };
                            let value = if at >= start && at <= start + length {
                                current
                            } else {
                                -(extent * ((at - origin - length / 2.) / available))
                                    .clamp(px(0.), extent)
                            };
                            down_handle.drag_started();
                            let mut offset = down_handle.offset();
                            if horizontal {
                                offset.x = value;
                            } else {
                                offset.y = value;
                            }
                            down_handle.set_offset(offset);
                            down_state.update(cx, |s, cx| {
                                s.drag = Some((at, value));
                                cx.notify();
                            });
                            window.refresh();
                            cx.stop_propagation();
                        });
                        let move_state = state.clone();
                        let move_handle = handle.clone();
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            move_state.update(cx, |s, cx| {
                                let hovered = hitbox.is_hovered(window);
                                if s.hovered != hovered {
                                    s.hovered = hovered;
                                    cx.notify();
                                }
                                if let Some((start, offset)) = s.drag {
                                    let at = if horizontal {
                                        e.position.x
                                    } else {
                                        e.position.y
                                    };
                                    let value = (offset - extent * ((at - start) / available))
                                        .clamp(-extent, px(0.));
                                    let mut p = move_handle.offset();
                                    if horizontal {
                                        p.x = value;
                                    } else {
                                        p.y = value;
                                    }
                                    move_handle.set_offset(p);
                                    window.refresh();
                                    cx.stop_propagation();
                                }
                            });
                        });
                        let up_state = state.clone();
                        let up_handle = handle.clone();
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture
                                && e.button == MouseButton::Left
                                && up_state.read(cx).drag.is_some()
                            {
                                up_handle.drag_ended();
                                up_state.update(cx, |s, cx| {
                                    s.drag = None;
                                    cx.notify();
                                });
                                window.refresh();
                                cx.stop_propagation();
                            }
                        });
                    },
                )
                .size_full(),
            )
    }
}
