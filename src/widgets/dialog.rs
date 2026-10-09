//! Modal confirmation dialogs and their focus/default-selection behavior.
use super::menu::{OverlayHost, overlays};
use super::*;

#[derive(Default)]
pub struct DialogButtonProps {
    ok: SharedString,
    cancel: SharedString,
    destructive: bool,
}
impl DialogButtonProps {
    pub fn ok_text(mut self, value: impl Into<SharedString>) -> Self {
        self.ok = value.into();
        self
    }
    pub fn cancel_text(mut self, value: impl Into<SharedString>) -> Self {
        self.cancel = value.into();
        self
    }
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }
}
type ConfirmHandler = Rc<dyn Fn(&ClickEvent, bool, &mut Window, &mut App) -> bool>;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    Cancel,
    Confirm,
    DontAskAgain,
}
pub struct Dialog {
    title: SharedString,
    description: SharedString,
    warning: Option<SharedString>,
    props: DialogButtonProps,
    callback: Option<ConfirmHandler>,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    selection: Selection,
    dont_ask_again: Option<bool>,
}
impl Dialog {
    pub fn title(mut self, value: impl Into<SharedString>) -> Self {
        self.title = value.into();
        self
    }
    pub fn description(mut self, value: impl Into<SharedString>) -> Self {
        self.description = value.into();
        self
    }
    pub fn warning(mut self, value: impl Into<SharedString>) -> Self {
        self.warning = Some(value.into());
        self
    }
    pub fn confirm(self) -> Self {
        self
    }
    pub fn default_confirm(mut self) -> Self {
        self.selection = Selection::Confirm;
        self
    }
    pub fn dont_ask_again(mut self) -> Self {
        self.dont_ask_again = Some(false);
        self
    }
    pub fn button_props(mut self, value: DialogButtonProps) -> Self {
        self.props = value;
        self
    }
    pub fn on_ok(
        mut self,
        callback: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.callback = Some(Rc::new(move |event, _, w, cx| callback(event, w, cx)));
        self
    }
    pub fn on_ok_with_dont_ask_again(
        mut self,
        callback: impl Fn(&ClickEvent, bool, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.callback = Some(Rc::new(callback));
        self
    }
    fn close(&self, w: &mut Window, cx: &mut App) {
        w.close_dialog(cx);
        if let Some(f) = &self.restore {
            f.focus(w, cx);
        }
    }
    fn confirm_action(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.callback.as_ref().is_none_or(|f| {
            f(
                &ClickEvent::default(),
                self.dont_ask_again.unwrap_or(false),
                w,
                cx,
            )
        }) {
            self.close(w, cx);
        }
    }
}
impl Render for Dialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("confirmation")
            .occlude()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(black().opacity(0.35))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|s, e: &KeyDownEvent, w, cx| {
                match e.keystroke.key.as_str() {
                    "escape" => s.close(w, cx),
                    "tab" => {
                        let order = [
                            Selection::Cancel,
                            Selection::DontAskAgain,
                            Selection::Confirm,
                        ];
                        let order: &[Selection] = if s.dont_ask_again.is_some() {
                            &order
                        } else {
                            &[Selection::Cancel, Selection::Confirm]
                        };
                        let index = order.iter().position(|item| *item == s.selection).unwrap();
                        let step = if e.keystroke.modifiers.shift {
                            order.len() - 1
                        } else {
                            1
                        };
                        s.selection = order[(index + step) % order.len()];
                        cx.notify();
                    }
                    "left" | "right" => {
                        s.selection = if s.selection == Selection::Confirm {
                            Selection::Cancel
                        } else {
                            Selection::Confirm
                        };
                        cx.notify();
                    }
                    "enter" | "space" => match s.selection {
                        Selection::Confirm => s.confirm_action(w, cx),
                        Selection::Cancel => s.close(w, cx),
                        Selection::DontAskAgain => {
                            if let Some(checked) = &mut s.dont_ask_again {
                                *checked = !*checked;
                            }
                            cx.notify();
                        }
                    },
                    _ => {}
                }
                cx.stop_propagation();
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .v_flex()
                    .w(px(460.))
                    .max_w_full()
                    .p_5()
                    .gap_4()
                    .rounded_lg()
                    .shadow_lg()
                    .bg(cx.theme().background)
                    .text_color(cx.theme().foreground)
                    .text_size(px(15.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.title.clone()),
                    )
                    .child(self.description.clone())
                    .when_some(self.warning.clone(), |d, warning| {
                        d.child(
                            div()
                                .debug_selector(|| "delete-warning".into())
                                .text_color(cx.theme().warning)
                                .child(warning),
                        )
                    })
                    .child(
                        div()
                            .debug_selector(|| "confirmation-footer".into())
                            .h_flex()
                            .justify_end()
                            .gap_2()
                            .when_some(self.dont_ask_again, |d, checked| {
                                d.child(
                                    div()
                                        .id("dont-ask-again")
                                        .role(Role::CheckBox)
                                        .aria_label("다시 묻지 않기")
                                        .aria_toggled(if checked {
                                            Toggled::True
                                        } else {
                                            Toggled::False
                                        })
                                        .debug_selector(|| "delete-dont-ask-again".into())
                                        .h_flex()
                                        .gap_2()
                                        .p_1()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .border_1()
                                        .border_color(
                                            if self.selection == Selection::DontAskAgain {
                                                cx.theme().primary
                                            } else {
                                                transparent_black()
                                            },
                                        )
                                        .child(
                                            div()
                                                .size(px(18.))
                                                .border_1()
                                                .border_color(cx.theme().foreground)
                                                .rounded_sm()
                                                .when(checked, |d| {
                                                    d.child(Icon::new(IconName::Check))
                                                }),
                                        )
                                        .child("다시 묻지 않기")
                                        .on_click(cx.listener(|s, _, _, cx| {
                                            if let Some(checked) = &mut s.dont_ask_again {
                                                *checked = !*checked;
                                            }
                                            s.selection = Selection::DontAskAgain;
                                            cx.stop_propagation();
                                            cx.notify();
                                        })),
                                )
                            })
                            .when(self.dont_ask_again.is_some(), |d| d.child(div().flex_1()))
                            .child(
                                Button::new("cancel")
                                    .label(self.props.cancel.clone())
                                    .tab_stop(false)
                                    .selected(self.selection == Selection::Cancel)
                                    .on_click(cx.listener(|s, _, w, cx| s.close(w, cx))),
                            )
                            .child(
                                Button::new("confirm")
                                    .label(self.props.ok.clone())
                                    .tab_stop(false)
                                    .primary()
                                    .when(self.props.destructive, |b| b.destructive())
                                    .when(self.selection == Selection::Confirm, |b| {
                                        b.border_2().border_color(cx.theme().foreground)
                                    })
                                    .on_click(cx.listener(|s, _, w, cx| s.confirm_action(w, cx))),
                            ),
                    ),
            )
    }
}
pub trait WindowExt {
    fn open_alert_dialog(
        &mut self,
        cx: &mut App,
        build: impl FnOnce(Dialog, &mut Window, &mut App) -> Dialog,
    );
    fn close_dialog(&mut self, cx: &mut App);
    fn has_active_dialog(&self, cx: &App) -> bool;
}
impl WindowExt for Window {
    fn open_alert_dialog(
        &mut self,
        cx: &mut App,
        build: impl FnOnce(Dialog, &mut Window, &mut App) -> Dialog,
    ) {
        let focus = cx.focus_handle();
        let restore = self.focused(cx);
        let dialog = build(
            Dialog {
                title: "".into(),
                description: "".into(),
                warning: None,
                props: DialogButtonProps::default()
                    .ok_text("확인")
                    .cancel_text("취소"),
                callback: None,
                focus: focus.clone(),
                restore,
                selection: Selection::Cancel,
                dont_ask_again: None,
            },
            self,
            cx,
        );
        let entity = cx.new(|_| dialog);
        overlays(cx).update(cx, |s, cx| {
            s.dialog = Some(entity);
            cx.notify();
        });
        focus.focus(self, cx);
    }
    fn close_dialog(&mut self, cx: &mut App) {
        overlays(cx).update(cx, |s, cx| {
            s.dialog = None;
            cx.notify();
        });
    }
    fn has_active_dialog(&self, cx: &App) -> bool {
        cx.has_global::<OverlayHost>() && overlays(cx).read(cx).dialog.is_some()
    }
}
