use super::*;
pub enum SelectEvent {
    Confirm(Option<SharedString>),
}
pub struct SelectState<T> {
    items: T,
    selected: Option<SharedString>,
    focus: FocusHandle,
    search: Entity<InputState>,
    searchable: bool,
    open: bool,
    cursor: usize,
    _subscription: Subscription,
}
impl EventEmitter<SelectEvent> for SelectState<Vec<SharedString>> {}
impl Focusable for SelectState<Vec<SharedString>> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl SelectState<Vec<SharedString>> {
    pub fn new(
        items: Vec<SharedString>,
        _: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("검색"));
        let subscription = cx.subscribe_in(&search, window, |s, _, e, w, cx| match e {
            InputEvent::Change => {
                s.cursor = 0;
                cx.notify();
            }
            InputEvent::PressEnter { .. } => s.choose(s.cursor, w, cx),
            _ => {}
        });
        Self {
            items,
            selected: None,
            focus: cx.focus_handle().tab_stop(true),
            search,
            searchable: false,
            open: false,
            cursor: 0,
            _subscription: subscription,
        }
    }
    pub fn searchable(mut self, v: bool) -> Self {
        self.searchable = v;
        self
    }
    pub fn set_selected_value(
        &mut self,
        value: &SharedString,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = Some(value.clone());
        cx.notify();
    }
    pub fn focus(&self, w: &mut Window, cx: &mut App) {
        self.focus.focus(w, cx);
    }
    fn filtered(&self, cx: &App) -> Vec<SharedString> {
        let q = self.search.read(cx).value().to_lowercase();
        self.items
            .iter()
            .filter(|v| v.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }
    fn choose(&mut self, index: usize, w: &mut Window, cx: &mut Context<Self>) {
        if let Some(v) = self.filtered(cx).get(index) {
            self.selected = Some(v.clone());
            cx.emit(SelectEvent::Confirm(Some(v.clone())));
        }
        self.open = false;
        self.focus(w, cx);
        cx.notify();
    }
    fn toggle(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.focus(w, cx);
        self.open = !self.open;
        if self.open {
            self.cursor = 0;
            self.search.update(cx, |s, cx| {
                s.set_value("", w, cx);
                if self.searchable {
                    s.focus(w, cx);
                }
            });
        }
        cx.notify();
    }
}
impl Render for SelectState<Vec<SharedString>> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut el = div()
            .id("select")
            .relative()
            .w_full()
            .track_focus(&self.focus)
            .tab_index(0)
            .tab_stop(true)
            .on_key_down(cx.listener(
                |s, e: &KeyDownEvent, w, cx| match e.keystroke.key.as_str() {
                    "escape" if s.open => {
                        s.open = false;
                        s.focus(w, cx);
                        cx.notify();
                        cx.stop_propagation();
                    }
                    "enter" | "space" if !s.open => {
                        s.toggle(w, cx);
                        cx.stop_propagation();
                    }
                    "enter" if s.open => {
                        s.choose(s.cursor, w, cx);
                        cx.stop_propagation();
                    }
                    "up" | "down" if s.open => {
                        let len = s.filtered(cx).len();
                        s.cursor = if e.keystroke.key == "up" {
                            s.cursor.saturating_sub(1)
                        } else {
                            (s.cursor + 1).min(len.saturating_sub(1))
                        };
                        cx.notify();
                        cx.stop_propagation();
                    }
                    _ => {}
                },
            ))
            .child(
                Button::new("toggle")
                    .label(self.selected.clone().unwrap_or_else(|| "선택".into()))
                    .icon(IconName::ChevronDown)
                    .w_full()
                    .border_1()
                    .border_color(cx.theme().border)
                    .on_click(cx.listener(|s, _, w, cx| s.toggle(w, cx))),
            );
        if self.open {
            let values = self.filtered(cx);
            let cursor = self.cursor;
            let popup = div()
                .id("select-popup")
                .occlude()
                .w(px(290.))
                .v_flex()
                .gap_1()
                .p_1()
                .rounded_md()
                .shadow_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .on_mouse_down_out(cx.listener(|s, _, w, cx| {
                    s.open = false;
                    s.focus(w, cx);
                    cx.notify();
                }))
                .when(self.searchable, |d| d.child(Input::new(&self.search)))
                .child(
                    div()
                        .id("options")
                        .max_h(px(230.))
                        .overflow_y_scroll()
                        .children(values.into_iter().enumerate().map(|(i, v)| {
                            Button::new(i)
                                .label(v)
                                .w_full()
                                .selected(i == cursor)
                                .tab_stop(false)
                                .on_click(cx.listener(move |s, _, w, cx| s.choose(i, w, cx)))
                        })),
                );
            el = el.child(
                deferred(div().absolute().top(px(34.)).left_0().child(popup)).with_priority(5),
            );
        }
        el
    }
}
#[derive(IntoElement)]
pub struct Select {
    state: Entity<SelectState<Vec<SharedString>>>,
    style: StyleRefinement,
    label: SharedString,
}
impl Select {
    pub fn new(state: &Entity<SelectState<Vec<SharedString>>>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            label: "".into(),
        }
    }
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }
    pub fn search_placeholder(self, _: &str) -> Self {
        self
    }
}
impl Styled for Select {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl RenderOnce for Select {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut el = div()
            .id(("select-wrapper", self.state.entity_id()))
            .aria_label(self.label);
        el.style().refine(&self.style);
        el.child(self.state)
    }
}
