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
        self.0.update(cx, |s, cx| {
            s.layout(bounds, window, cx);
            s.drop_hitbox = Some(window.insert_hitbox(bounds, HitboxBehavior::Normal));
            let modifiers = window.modifiers();
            s.link_hover = !s.selecting
                && s.scroll_drag.is_none()
                && modifiers.control
                && !modifiers.alt
                && !modifiers.shift
                && !modifiers.platform
                && s.link_at(window.mouse_position()).is_some();
        });
        let state = self.0.read(cx);
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
        let link = state
            .link_hover
            .then(|| window.insert_hitbox(bounds, HitboxBehavior::Normal));
        (bars, link)
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
        if let Some(hitbox) = &bars.1 {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        }
        for (_, hitbox) in &bars.0 {
            window.set_cursor_style(CursorStyle::Arrow, hitbox);
        }
        let entity = self.0.clone();
        window.on_modifiers_changed(move |_, _, cx| entity.update(cx, |_, cx| cx.notify()));
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
        window.on_mouse_event(move |_: &FileDropEvent, _, window, cx| {
            entity.update(cx, |s, cx| s.show_image_drop(None, window, cx));
        });
        let entity = self.0.clone();
        let hitboxes = bars.0.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }
            entity.update(cx, |s, cx| {
                if s.drop_position.is_some()
                    && (!cx.has_active_drag() || !s.accepts_image_drop(e.position, window))
                {
                    s.show_image_drop(None, window, cx);
                }
                let hover = hitboxes
                    .iter()
                    .find(|(_, h)| h.is_hovered(window))
                    .map(|(axis, _)| *axis);
                if s.scroll_hover != hover {
                    s.scroll_hover = hover;
                    cx.notify();
                }
                let link_hover = !s.selecting
                    && s.scroll_drag.is_none()
                    && e.modifiers.control
                    && !e.modifiers.alt
                    && !e.modifiers.shift
                    && !e.modifiers.platform
                    && s.link_at(e.position).is_some();
                if s.link_hover != link_hover {
                    s.link_hover = link_hover;
                    cx.notify();
                }
                if s.scroll_drag.is_some() || s.selecting {
                    if e.pressed_button == Some(MouseButton::Left) {
                        s.mouse_move(e, window, cx);
                    } else {
                        s.stop_pointer_drag();
                        cx.notify();
                    }
                    cx.stop_propagation();
                }
            });
        });
        let entity = self.0.clone();
        window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
            if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
                entity.update(cx, |s, cx| {
                    s.show_image_drop(None, window, cx);
                    if s.scroll_drag.is_some() || s.selecting {
                        // Use the release position even if no move event preceded it.
                        if s.scroll_drag.is_some()
                            || s.selection_auto_scroll.last_drag_position != Some(e.position)
                        {
                            s.mouse_move(
                                &MouseMoveEvent {
                                    position: e.position,
                                    modifiers: e.modifiers,
                                    pressed_button: Some(MouseButton::Left),
                                },
                                window,
                                cx,
                            );
                        }
                        s.stop_pointer_drag();
                        cx.notify();
                        cx.stop_propagation();
                    }
                });
            }
        });
        let focus = self.0.read(cx).focus.clone();
        if !self.0.read(cx).is_read_only() {
            window.handle_input(&focus, ElementInputHandler::new(bounds, self.0.clone()), cx);
        }
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.0.update(cx, |s, cx| s.paint(window, cx))
        });
    }
}
impl Render for InputState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.is_markdown_reading() {
            return self.reading_view(window, cx);
        }
        let drop_entity = cx.entity();
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
            .when(self.multiline, |div| {
                div.can_drop(move |drag, _, cx| {
                    drop_entity.read(cx).can_insert_images()
                        && drag.downcast_ref::<ExternalPaths>().is_some_and(|paths| {
                            paths
                                .paths()
                                .iter()
                                .any(|path| Self::supports_image_path(path))
                        })
                })
                .on_drag_move(
                    cx.listener(|s, event: &DragMoveEvent<ExternalPaths>, window, cx| {
                        let supported = event
                            .drag(cx)
                            .paths()
                            .iter()
                            .any(|path| Self::supports_image_path(path));
                        s.show_image_drop(supported.then_some(event.event.position), window, cx);
                    }),
                )
                .on_drop(cx.listener(|_, paths: &ExternalPaths, window, cx| {
                    cx.emit(InputEvent::DropImages {
                        paths: paths.paths().to_vec(),
                        position: window.mouse_position(),
                    });
                }))
            })
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(|s, _: &SelectAll, w, cx| s.select_all(w, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
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
            .children(self.pointer_drag_capture())
            .into_any_element()
    }
}

impl InputState {
    /// Deferred prepaint escapes the input's clipping and places this hitbox
    /// above the rest of the window. Stopping propagation alone cannot suppress
    /// GPUI's hover styles, cursor changes and tooltips.
    pub(super) fn pointer_drag_capture(&self) -> Option<impl IntoElement> {
        (self.selecting || self.scroll_drag.is_some() || self.is_reading_dragging()).then(|| {
            let cursor = if self.scroll_drag.is_some() {
                CursorStyle::Arrow
            } else {
                CursorStyle::IBeam
            };
            deferred(
                canvas(
                    |_, window, _| {
                        window.insert_hitbox(
                            Bounds::new(point(px(0.), px(0.)), window.viewport_size()),
                            HitboxBehavior::BlockMouse,
                        )
                    },
                    move |_, hitbox, window, _| window.set_cursor_style(cursor, &hitbox),
                )
                .absolute()
                .size_full(),
            )
        })
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
