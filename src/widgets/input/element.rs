//! GPUI elements that host the editor and single-line fields.
use super::*;

struct EditorElement(Entity<InputState>);
impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = (Vec<(bool, Hitbox)>, Option<Hitbox>);
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.0.update(cx, |s, _| s.layout(bounds, window));
        let state = self.0.read(cx);
        let capture = state.scroll_drag.map(|_| {
            window.insert_hitbox(
                Bounds::new(point(px(0.), px(0.)), window.viewport_size()),
                HitboxBehavior::BlockMouse,
            )
        });
        let bars = [true, false]
            .into_iter()
            .filter_map(|axis| {
                state.scrollbar_geometry(axis).map(|(track, _)| {
                    (
                        axis,
                        window.insert_hitbox(track, HitboxBehavior::BlockMouseExceptScroll),
                    )
                })
            })
            .collect();
        (bars, capture)
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        bars: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let hovered = bars
            .0
            .iter()
            .find(|(_, hitbox)| hitbox.is_hovered(window))
            .map(|(axis, _)| *axis);
        self.0.update(cx, |s, _| s.scroll_hover = hovered);
        for (_, hitbox) in &bars.0 {
            window.set_cursor_style(CursorStyle::Arrow, hitbox);
        }
        if let Some(hitbox) = &bars.1 {
            window.set_cursor_style(CursorStyle::Arrow, hitbox);
        }
        let entity = self.0.clone();
        let hitboxes = bars.0.clone();
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && e.button == MouseButton::Left
                && hitboxes.iter().any(|(_, h)| h.is_hovered(window))
            {
                entity.update(cx, |s, cx| s.mouse_down(e, window, cx));
            }
        });
        let entity = self.0.clone();
        let hitboxes = bars.0.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }
            entity.update(cx, |s, cx| {
                let hover = hitboxes
                    .iter()
                    .find(|(_, h)| h.is_hovered(window))
                    .map(|(axis, _)| *axis);
                if s.scroll_hover != hover {
                    s.scroll_hover = hover;
                    cx.notify();
                }
                if s.scroll_drag.is_some() {
                    s.mouse_move(e, window, cx);
                    cx.stop_propagation();
                }
            });
        });
        let entity = self.0.clone();
        window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
                entity.update(cx, |s, cx| {
                    if s.scroll_drag.take().is_some() {
                        cx.notify();
                        cx.stop_propagation();
                    }
                });
            }
        });
        let focus = self.0.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.0.clone()), cx);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.0.update(cx, |s, cx| s.paint(window, cx))
        });
    }
}
impl Render for InputState {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("text-input")
            .debug_selector(|| "text-input".into())
            .role(if self.multiline {
                Role::MultilineTextInput
            } else {
                Role::TextInput
            })
            .aria_label(self.label.clone())
            .aria_value(self.content.clone())
            .aria_placeholder(self.placeholder.clone())
            .size_full()
            .overflow_hidden()
            .key_context("EdotInput")
            .track_focus(&self.focus)
            .tab_index(0)
            .tab_stop(true)
            .cursor_text()
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(|s, _: &SelectAll, w, cx| s.select_all(w, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|s, _, _, _| {
                    s.selecting = false;
                    s.scroll_drag = None;
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|s, _, _, _| {
                    s.selecting = false;
                    s.scroll_drag = None;
                }),
            )
            .on_scroll_wheel(cx.listener(|s, e: &ScrollWheelEvent, _, cx| {
                if !e.modifiers.control {
                    let mut delta = e.delta.pixel_delta(s.line_height);
                    if e.modifiers.shift {
                        // Windows already maps Shift+wheel to x. Only convert
                        // platforms/events that still provide a vertical delta.
                        if delta.x == px(0.) {
                            delta.x = delta.y;
                        }
                        delta.y = px(0.);
                        s.wheel_gesture = OngoingScroll::default();
                    } else if e.delta.precise() {
                        // Preserve the left edge during a mostly vertical
                        // trackpad gesture instead of accumulating sideways noise.
                        s.wheel_gesture.filter(&mut delta, e.touch_phase);
                    }
                    s.scroll += delta;
                    s.clamp_scroll();
                    s.reveal = false;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child(EditorElement(cx.entity()))
    }
}

#[derive(IntoElement)]
pub struct Input {
    state: Entity<InputState>,
    style: StyleRefinement,
    appearance: bool,
    label: SharedString,
    suffix: Vec<AnyElement>,
}
pub type Textarea = Input;
impl Input {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            appearance: true,
            label: "".into(),
            suffix: vec![],
        }
    }
    pub fn suffix(mut self, child: impl IntoElement) -> Self {
        self.suffix.push(child.into_any_element());
        self
    }
    pub fn small(self) -> Self {
        self.h(px(32.))
    }
    pub fn appearance(mut self, value: bool) -> Self {
        self.appearance = value;
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }
}
impl Styled for Input {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl RenderOnce for Input {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state
            .update(cx, |state, _| state.label = self.label.clone());
        let focus = self.state.read(cx).focus.clone();
        let label = self.label.clone();
        let mut el = div()
            .id(("input", self.state.entity_id()))
            .debug_selector(move || label.to_string())
            .track_focus(&focus)
            .h_flex()
            .w_full()
            .h(px(34.))
            .min_w_0()
            .px_2()
            .py_0()
            .aria_label(self.label)
            .when(self.appearance, |d| {
                d.border_1()
                    .rounded_md()
                    .border_color(cx.theme().border)
                    .focus(|s| s.border_color(cx.theme().primary))
                    .bg(cx.theme().background)
            });
        el.style().refine(&self.style);
        el.child(div().flex_1().min_w_0().h_full().child(self.state))
            .children(self.suffix)
    }
}
