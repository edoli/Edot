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
        if self.delta == delta && self.task.is_some() {
            return;
        }
        self.task = None;
        self.delta = delta;
        if let Some(delta) = delta {
            self.task = Some(cx.spawn(async move |entity, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(16))
                        .await;
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
    handle: ScrollHandle,
    horizontal: bool,
}
impl Scrollbar {
    pub fn vertical(handle: &UniformListScrollHandle) -> Self {
        Self {
            handle: handle.0.borrow().base_handle.clone(),
            horizontal: false,
        }
    }
    pub fn horizontal(handle: &ScrollHandle) -> Self {
        Self {
            handle: handle.clone(),
            horizontal: true,
        }
    }
}
#[derive(Clone)]
struct ScrollDrag {
    start: Rc<std::cell::Cell<Pixels>>,
    offset: Pixels,
}
struct EmptyDrag;
impl Render for EmptyDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
impl RenderOnce for Scrollbar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let handle = self.handle;
        let bounds = handle.bounds();
        let offset = handle.offset();
        let horizontal = self.horizontal;
        let max = handle.max_offset();
        let viewport = if horizontal {
            bounds.size.width
        } else {
            bounds.size.height
        };
        let extent = if horizontal { max.x } else { max.y };
        let thumb = (viewport * (viewport / (viewport + extent).max(px(1.))))
            .max(px(22.))
            .min(viewport);
        let available = (viewport - thumb).max(px(1.));
        let current = if horizontal { offset.x } else { offset.y };
        let pos = available * (-current / extent.max(px(1.)));
        let click_handle = handle.clone();
        let drag_handle = handle.clone();
        div()
            .id(if horizontal {
                "h-scrollbar"
            } else {
                "v-scrollbar"
            })
            .absolute()
            .when(horizontal, |d| d.bottom_0().left_0().w_full().h(px(8.)))
            .when(!horizontal, |d| d.right_0().top_0().h_full().w(px(8.)))
            .when(extent <= px(0.), |d| d.invisible())
            .on_mouse_down(MouseButton::Left, move |e, w, _| {
                let at = if horizontal {
                    e.position.x - bounds.left()
                } else {
                    e.position.y - bounds.top()
                };
                let value = -(extent * ((at - thumb / 2.) / available)).clamp(px(0.), extent);
                let mut p = click_handle.offset();
                if horizontal {
                    p.x = value
                } else {
                    p.y = value
                }
                click_handle.set_offset(p);
                w.refresh();
            })
            .child(
                div()
                    .id("thumb")
                    .absolute()
                    .rounded_md()
                    .bg(cx.theme().muted_foreground.opacity(0.45))
                    .when(horizontal, |d| d.left(pos).w(thumb).h(px(6.)))
                    .when(!horizontal, |d| d.top(pos).h(thumb).w(px(6.)))
                    .on_drag(
                        ScrollDrag {
                            start: Rc::new(std::cell::Cell::new(px(0.))),
                            offset: current,
                        },
                        move |drag, _, w, cx| {
                            let p = w.mouse_position();
                            drag.start.set(if horizontal { p.x } else { p.y });
                            cx.new(|_| EmptyDrag)
                        },
                    )
                    .on_drag_move(move |e: &DragMoveEvent<ScrollDrag>, w, cx| {
                        let drag = e.drag(cx);
                        let at = if horizontal {
                            e.event.position.x
                        } else {
                            e.event.position.y
                        };
                        let value = (drag.offset - extent * ((at - drag.start.get()) / available))
                            .clamp(-extent, px(0.));
                        let mut p = drag_handle.offset();
                        if horizontal {
                            p.x = value
                        } else {
                            p.y = value
                        }
                        drag_handle.set_offset(p);
                        w.refresh();
                    }),
            )
    }
}
