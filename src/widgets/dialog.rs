//! Modal confirmation dialogs and their focus/default-cancel behavior.
use super::menu::{OverlayHost, overlays};
use super::*;

#[derive(Default)]
pub struct DialogButtonProps {
    ok: SharedString,
    cancel: SharedString,
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
}
type ConfirmHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) -> bool>;
pub struct Dialog {
    title: SharedString,
    description: SharedString,
    props: DialogButtonProps,
    callback: Option<ConfirmHandler>,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    confirm_selected: bool,
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
    pub fn confirm(self) -> Self {
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
        if self
            .callback
            .as_ref()
            .is_none_or(|f| f(&ClickEvent::default(), w, cx))
        {
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
                    "tab" | "left" | "right" => {
                        s.confirm_selected = !s.confirm_selected;
                        cx.notify();
                    }
                    "enter" | "space" => {
                        if s.confirm_selected {
                            s.confirm_action(w, cx)
                        } else {
                            s.close(w, cx)
                        }
                    }
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
                    .child(
                        div()
                            .h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("cancel")
                                    .label(self.props.cancel.clone())
                                    .tab_stop(false)
                                    .selected(!self.confirm_selected)
                                    .on_click(cx.listener(|s, _, w, cx| s.close(w, cx))),
                            )
                            .child(
                                Button::new("confirm")
                                    .label(self.props.ok.clone())
                                    .tab_stop(false)
                                    .primary()
                                    .when(self.confirm_selected, |b| {
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
                props: DialogButtonProps::default()
                    .ok_text("확인")
                    .cancel_text("취소"),
                callback: None,
                focus: focus.clone(),
                restore,
                confirm_selected: false,
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
