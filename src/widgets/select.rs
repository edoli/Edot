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
    suffix: SharedString,
    open: bool,
    cursor: usize,
    scroll: ScrollHandle,
    trigger_bounds: Rc<std::cell::Cell<Bounds<Pixels>>>,
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
        initial: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("검색"));
        let subscription = cx.subscribe_in(&search, window, |s, _, e, _, cx| {
            if matches!(e, InputEvent::Change) {
                s.restore_cursor(cx);
                cx.notify();
            }
        });
        let selected = initial.and_then(|index| items.get(index).cloned());
        Self {
            items,
            selected,
            focus: cx.focus_handle().tab_stop(true),
            search,
            searchable: false,
            suffix: "".into(),
            open: false,
            cursor: 0,
            scroll: ScrollHandle::new(),
            trigger_bounds: Rc::new(std::cell::Cell::new(Bounds::default())),
            _subscription: subscription,
        }
    }
    pub fn searchable(mut self, v: bool) -> Self {
        self.searchable = v;
        self
    }
    pub fn suffix(mut self, suffix: impl Into<SharedString>) -> Self {
        self.suffix = suffix.into();
        self
    }
    pub fn set_selected_value(
        &mut self,
        value: &SharedString,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = Some(value.clone());
        if self.open {
            self.restore_cursor(cx);
        }
        cx.notify();
    }
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
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
    fn restore_cursor(&mut self, cx: &App) {
        let values = self.filtered(cx);
        self.cursor = values
            .iter()
            .position(|value| Some(value) == self.selected.as_ref())
            .unwrap_or(0);
        // Explicit row metrics also work on the first frame, before GPUI has
        // initialized this scroll handle's viewport and overflow mode.
        self.scroll
            .set_offset(point(px(0.), -px(self.cursor as f32 * 32.)));
    }
    fn dismiss(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.focus(w, cx);
        cx.notify();
    }
    fn choose(&mut self, index: usize, w: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        let Some(value) = self.filtered(cx).get(index).cloned() else {
            return;
        };
        self.selected = Some(value.clone());
        self.dismiss(w, cx);
        cx.emit(SelectEvent::Confirm(Some(value)));
    }
    fn toggle(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.dismiss(w, cx);
            return;
        }
        self.open = true;
        self.focus(w, cx);
        self.search.update(cx, |search, cx| {
            if !search.value().is_empty() {
                search.set_value("", w, cx);
            }
            if self.searchable {
                search.focus(w, cx);
            }
        });
        self.restore_cursor(cx);
        cx.notify();
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let search_focused = self.search.read(cx).focus_handle(cx).is_focused(window);
        match key {
            "escape" if self.open => self.dismiss(window, cx),
            "tab" if self.open => {
                self.dismiss(window, cx);
                if event.keystroke.modifiers.shift {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
            }
            "enter" if self.open => self.choose(self.cursor, window, cx),
            "space" if self.open && !search_focused => self.choose(self.cursor, window, cx),
            "enter" | "space" | "down" | "up" if !self.open => self.toggle(window, cx),
            "up" | "down" | "pageup" | "pagedown" if self.open => {
                let last = self.filtered(cx).len().saturating_sub(1);
                self.cursor = match key {
                    "up" => self.cursor.saturating_sub(1),
                    "down" => (self.cursor + 1).min(last),
                    "pageup" => self.cursor.saturating_sub(7),
                    _ => (self.cursor + 7).min(last),
                };
                self.scroll.scroll_to_item(self.cursor);
                cx.notify();
            }
            "home" | "end" if self.open && !search_focused => {
                self.cursor = if key == "home" {
                    0
                } else {
                    self.filtered(cx).len().saturating_sub(1)
                };
                self.scroll.scroll_to_item(self.cursor);
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
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
            .capture_key_down(cx.listener(Self::key))
            .child(
                canvas(
                    {
                        let bounds = self.trigger_bounds.clone();
                        move |rect, _, _| bounds.set(rect)
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(
                Button::new("toggle")
                    .label(self.selected.as_ref().map_or_else(
                        || "선택".to_string(),
                        |value| format!("{value}{}", self.suffix),
                    ))
                    .icon(IconName::ChevronDown)
                    .w_full()
                    .justify_start()
                    .text_left()
                    .text_ellipsis()
                    .overflow_hidden()
                    .tab_stop(false)
                    .border_1()
                    .border_color(cx.theme().border)
                    .on_click(cx.listener(|s, _, w, cx| s.toggle(w, cx))),
            );
        if self.open {
            let values = self.filtered(cx);
            let cursor = self.cursor;
            let popup = div()
                .id("select-popup")
                .debug_selector(|| "select-popup".into())
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
                .on_mouse_down_out(cx.listener(|s, event: &MouseDownEvent, w, cx| {
                    // The trigger is part of the same control. Let its click
                    // toggle once instead of closing on down and reopening on up.
                    if !s.trigger_bounds.get().contains(&event.position) {
                        s.dismiss(w, cx);
                    }
                }))
                .when(self.searchable, |d| d.child(Input::new(&self.search)))
                .child(
                    div()
                        .id("options")
                        .debug_selector(|| "select-options".into())
                        .v_flex()
                        .track_scroll(&self.scroll)
                        .max_h(px(230.))
                        .overflow_y_scroll()
                        .when(values.is_empty(), |list| {
                            list.child(
                                div()
                                    .p_2()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("검색 결과가 없습니다."),
                            )
                        })
                        .children(values.into_iter().enumerate().map(|(i, v)| {
                            div()
                                .when(i == cursor, |row| {
                                    row.debug_selector(|| "select-highlight".into())
                                })
                                .flex_shrink_0()
                                .h(px(32.))
                                .child(
                                    Button::new(i)
                                        .label(format!("{v}{}", self.suffix))
                                        .w_full()
                                        .flex_shrink_0()
                                        .justify_start()
                                        .text_left()
                                        .overflow_hidden()
                                        .selected(i == cursor)
                                        .tab_stop(false)
                                        .on_click(
                                            cx.listener(move |s, _, w, cx| s.choose(i, w, cx)),
                                        ),
                                )
                        })),
                );
            el = el.child(
                deferred(div().absolute().top(px(34.)).right_0().child(popup)).with_priority(5),
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
