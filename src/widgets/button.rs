use super::*;

pub(super) type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    tip: Option<SharedString>,
    click: Option<ClickHandler>,
    style: StyleRefinement,
    primary: bool,
    destructive: bool,
    selected: bool,
    tab_stop: bool,
    compact: bool,
}
impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: "".into(),
            icon: None,
            tip: None,
            click: None,
            style: StyleRefinement::default(),
            primary: false,
            destructive: false,
            selected: false,
            tab_stop: true,
            compact: false,
        }
    }
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn tooltip(mut self, tip: impl Into<SharedString>) -> Self {
        self.tip = Some(tip.into());
        self
    }
    pub fn on_click(
        mut self,
        click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.click = Some(Rc::new(click));
        self
    }
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }
    pub fn destructive(mut self) -> Self {
        self.primary = true;
        self.destructive = true;
        self
    }
    pub fn small(mut self) -> Self {
        self.compact = true;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn tab_stop(mut self, value: bool) -> Self {
        self.tab_stop = value;
        self
    }
}
impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().colors;
        let primary = if self.destructive {
            theme.destructive
        } else {
            theme.primary
        };
        let primary_hover = if self.destructive {
            theme.destructive.blend(theme.foreground.opacity(0.12))
        } else {
            theme.primary_hover
        };
        let click = self.click;
        let mut el = div()
            .id(self.id)
            .role(Role::Button)
            .h_flex()
            .justify_center()
            .gap_1()
            .h(px(if self.compact { 27. } else { 32. }))
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .tab_index(0)
            .tab_stop(self.tab_stop)
            .text_size(px(theme::UI_FONT_SIZE))
            .aria_label(self.tip.clone().unwrap_or(self.label.clone()))
            .text_color(if self.primary {
                theme.primary_foreground
            } else {
                theme.foreground
            })
            .when(self.primary, |d| d.bg(primary))
            .when(self.selected, |d| {
                d.bg(theme.selection).text_color(theme.primary)
            })
            .hover(move |d| {
                d.bg(if self.primary {
                    primary_hover
                } else {
                    theme.accent
                })
            })
            .focus_visible(|s| s.border_1().border_color(primary))
            .on_click(move |e, w, cx| {
                cx.stop_propagation();
                if let Some(f) = &click {
                    f(e, w, cx);
                }
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when_some(self.icon, |d, i| d.child(Icon::new(i)))
            .when(!self.label.is_empty(), |d| d.child(self.label));
        if let Some(tip) = self.tip {
            el = el.tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx));
        }
        el.style().refine(&self.style);
        el
    }
}
pub struct Tooltip(SharedString);
impl Tooltip {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self(text.into())
    }
    pub fn build(self, _: &mut Window, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }
}
impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .font_family(theme::UI_FONT)
            .text_size(px(theme::UI_FONT_SIZE))
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.0.clone())
    }
}
