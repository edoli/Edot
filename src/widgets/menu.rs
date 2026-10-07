use super::*;
pub fn init(_: &mut App) {}
pub struct OverlayHost(pub Entity<Overlays>);
impl Global for OverlayHost {}
#[derive(Default)]
pub struct Overlays {
    pub menu: Option<(Entity<PopupMenu>, Point<Pixels>)>,
    dialog: Option<Entity<Dialog>>,
}
impl Render for Overlays {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut el = div().absolute().inset_0();
        if let Some((menu, position)) = &self.menu {
            el = el.child(
                deferred(
                    anchored()
                        .position(*position)
                        .snap_to_window()
                        .child(menu.clone()),
                )
                .with_priority(10),
            );
        }
        if let Some(dialog) = &self.dialog {
            el = el.child(dialog.clone());
        }
        el
    }
}
fn overlays(cx: &App) -> Entity<Overlays> {
    cx.global::<OverlayHost>().0.clone()
}
fn clear_menu(cx: &mut App) {
    if cx.has_global::<OverlayHost>() {
        overlays(cx).update(cx, |s, cx| {
            s.menu = None;
            cx.notify();
        });
    }
}

#[derive(Clone)]
pub struct PopupMenuItem {
    label: SharedString,
    disabled: bool,
    shortcut: Option<SharedString>,
    destructive: bool,
    callback: Option<ClickHandler>,
}
impl PopupMenuItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            disabled: false,
            shortcut: None,
            destructive: false,
            callback: None,
        }
    }
    pub fn shortcut(mut self, value: impl Into<SharedString>) -> Self {
        self.shortcut = Some(value.into());
        self
    }
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }
    pub fn disabled(mut self, value: bool) -> Self {
        self.disabled = value;
        self
    }
    pub fn on_click(
        mut self,
        callback: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.callback = Some(Rc::new(callback));
        self
    }
}
pub struct PopupMenu {
    items: Vec<Option<PopupMenuItem>>,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    selected: usize,
}
impl EventEmitter<DismissEvent> for PopupMenu {}
impl Focusable for PopupMenu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl PopupMenu {
    pub fn build(
        window: &mut Window,
        cx: &mut App,
        build: impl FnOnce(Self, &mut Window, &mut App) -> Self,
    ) -> Entity<Self> {
        let focus = cx.focus_handle();
        let restore = window.focused(cx);
        let value = build(
            Self {
                items: vec![],
                focus,
                restore,
                selected: 0,
            },
            window,
            cx,
        );
        cx.new(|_| value)
    }
    pub fn action_context(mut self, focus: FocusHandle) -> Self {
        self.restore = Some(focus);
        self
    }
    pub fn item(mut self, item: PopupMenuItem) -> Self {
        self.items.push(Some(item));
        self
    }
    pub fn separator(mut self) -> Self {
        self.items.push(None);
        self
    }
    pub fn menu(
        self,
        label: impl Into<SharedString>,
        shortcut: impl Into<SharedString>,
        action: Box<dyn Action>,
    ) -> Self {
        self.item(
            PopupMenuItem::new(label)
                .shortcut(shortcut)
                .on_click(move |_, w, cx| w.dispatch_action(action.boxed_clone(), cx)),
        )
    }
    fn dismiss(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = &self.restore {
            f.focus(w, cx);
        }
        clear_menu(cx);
        cx.emit(DismissEvent);
        cx.notify();
    }
    fn activate(&mut self, index: usize, w: &mut Window, cx: &mut Context<Self>) {
        let item = self.items.get(index).and_then(|i| i.clone());
        if let Some(item) = item
            && !item.disabled
        {
            self.dismiss(w, cx);
            if let Some(callback) = item.callback {
                callback(&ClickEvent::default(), w, cx);
            }
        }
    }
}
impl Render for PopupMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut el = div()
            .id("popup")
            .occlude()
            .v_flex()
            .min_w(px(200.))
            .max_w(px(380.))
            .py_1()
            .rounded_md()
            .shadow_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_size(px(14.))
            .track_focus(&self.focus)
            .on_mouse_down_out(cx.listener(|s, _, w, cx| s.dismiss(w, cx)))
            .on_key_down(cx.listener(|s, e: &KeyDownEvent, w, cx| {
                match e.keystroke.key.as_str() {
                    "escape" => s.dismiss(w, cx),
                    "enter" | "space" => s.activate(s.selected, w, cx),
                    "up" | "down" => {
                        let len = s.items.len();
                        for _ in 0..len {
                            s.selected = if e.keystroke.key == "up" {
                                (s.selected + len - 1) % len
                            } else {
                                (s.selected + 1) % len
                            };
                            if s.items[s.selected].as_ref().is_some_and(|i| !i.disabled) {
                                break;
                            }
                        }
                        cx.notify();
                    }
                    _ => {}
                }
                cx.stop_propagation();
            }));
        for (i, item) in self.items.iter().enumerate() {
            el = if let Some(item) = item {
                let hover = cx.theme().accent;
                el.child(
                    div()
                        .id(i)
                        .px_3()
                        .py_1()
                        .cursor_pointer()
                        .h_flex()
                        .justify_between()
                        .gap_6()
                        .whitespace_nowrap()
                        .when(item.destructive, |d| d.text_color(cx.theme().destructive))
                        .child(div().child(item.label.clone()))
                        .when_some(item.shortcut.clone(), |d, shortcut| {
                            d.child(
                                div()
                                    .debug_selector(|| format!("menu-shortcut-{shortcut}"))
                                    .flex_shrink_0()
                                    .text_color(if item.destructive {
                                        cx.theme().destructive
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .child(shortcut),
                            )
                        })
                        .when(i == self.selected, |d| d.bg(hover))
                        .when(item.disabled, |d| d.opacity(0.4))
                        .hover(move |s| s.bg(hover))
                        .on_mouse_move(cx.listener(move |s, _, _, cx| {
                            if s.selected != i {
                                s.selected = i;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |s, _, w, cx| s.activate(i, w, cx))),
                )
            } else {
                el.child(div().h(px(1.)).my_1().bg(cx.theme().border))
            };
        }
        el
    }
}
pub trait ContextMenuExt: InteractiveElement + Sized {
    fn context_menu(
        self,
        build: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> Self {
        self.on_mouse_down(MouseButton::Right, move |e, w, cx| {
            let menu = PopupMenu::build(w, cx, |m, w, cx| build(m, w, cx));
            menu.focus_handle(cx).focus(w, cx);
            overlays(cx).update(cx, |s, cx| {
                s.menu = Some((menu, e.position));
                cx.notify();
            });
            cx.stop_propagation();
        })
    }
}
impl<T: InteractiveElement> ContextMenuExt for T {}

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
