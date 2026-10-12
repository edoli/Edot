use super::*;

type SwitchHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    focus: Option<FocusHandle>,
    checked: bool,
    label: SharedString,
    on_change: Option<SwitchHandler>,
}
impl Switch {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            focus: None,
            checked: false,
            label: "".into(),
            on_change: None,
        }
    }
    pub fn checked(mut self, value: bool) -> Self {
        self.checked = value;
        self
    }
    pub fn track_focus(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }
    pub fn label(mut self, value: impl Into<SharedString>) -> Self {
        self.label = value.into();
        self
    }
    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}
impl RenderOnce for Switch {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let checked = self.checked;
        let click = self.on_change;
        let colors = cx.theme().colors;
        let dark = colors.background.l < 0.5;
        let off = colors.border;
        let on = if dark {
            colors.primary_active
        } else {
            colors.primary
        };
        let thumb = if dark {
            colors.foreground
        } else {
            gpui::white()
        };
        let control =
            div()
                .id(self.id)
                .role(Role::Switch)
                .debug_selector(|| "settings-wrap".into())
                .aria_label(self.label.clone())
                .aria_toggled(if checked {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .tab_index(0)
                .tab_stop(true)
                .when_some(self.focus, |el, focus| el.track_focus(&focus))
                .h_flex()
                .flex_shrink_0()
                .h(px(32.))
                .px_1()
                .gap_2()
                .text_left()
                .rounded_md()
                .border_1()
                .border_color(gpui::transparent_black())
                .cursor_pointer()
                .focus_visible(|style| style.border_color(cx.theme().primary))
                .child(
                    div()
                        .w(px(28.))
                        .text_color(cx.theme().muted_foreground)
                        .child(if checked { "켜짐" } else { "꺼짐" }),
                )
                .child(
                    div()
                        .id("switch-track")
                        .debug_selector(|| "settings-wrap-track".into())
                        .relative()
                        .flex_shrink_0()
                        .w(px(42.))
                        .h(px(24.))
                        .rounded_full()
                        .with_spring(
                            "switch-transition",
                            // Critically damped: fast, smooth, and no bounce. GPUI
                            // retains position/velocity on reversal and honors reduced motion.
                            SpringAnimation::new(SpringConfig::new(1600., 80., 1.))
                                .to(if checked { 1_f32 } else { 0_f32 }),
                            move |track, progress| {
                                let progress = progress.clamp(0., 1.);
                                let fill = off.blend(on.opacity(progress));
                                track
                                    .bg(fill)
                                    .hover(move |style| style.bg(fill.blend(thumb.opacity(0.08))))
                                    .active(|style| style.opacity(0.88))
                                    .child(
                                        div()
                                            .debug_selector(|| "settings-wrap-thumb".into())
                                            .absolute()
                                            .top(px(3.))
                                            .left(px(3. + 18. * progress))
                                            .size(px(18.))
                                            .rounded_full()
                                            .bg(thumb.blend(gpui::white().opacity(progress)))
                                            .shadow_sm(),
                                    )
                            },
                        ),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    if let Some(handler) = &click {
                        handler(&!checked, window, cx);
                    }
                    cx.stop_propagation();
                });
        div()
            .h_flex()
            .w_full()
            .h(px(40.))
            .gap_2()
            .text_left()
            .child(
                div()
                    .debug_selector(|| "settings-wrap-label".into())
                    .flex_1()
                    .child(self.label),
            )
            .child(control)
    }
}
