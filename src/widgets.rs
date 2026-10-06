//! Small application-specific controls implemented directly with GPUI.
use gpui::{prelude::*, *};
use std::rc::Rc;
pub mod input;
pub mod menu;
pub mod scroll;
mod select;
pub use input::{Input, InputEvent, InputState, Textarea, TextareaState};
pub use menu::{ContextMenuExt, DialogButtonProps, PopupMenuItem, WindowExt};
pub use select::{Select, SelectEvent, SelectState};

pub fn init(cx: &mut App) {
    cx.set_global(Theme::default());
    input::init(cx);
    menu::init(cx);
}
pub fn open_window<V: Render + 'static>(
    options: WindowOptions,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> Result<(AnyWindowHandle, Entity<V>)> {
    let mut view = None;
    let window = cx.open_window(options, |window, cx| {
        let content = build(window, cx);
        view = Some(content.clone());
        let overlays = cx.new(|_| menu::Overlays::default());
        cx.set_global(menu::OverlayHost(overlays.clone()));
        cx.new(|_| Root {
            content: content.into(),
            overlays,
        })
    })?;
    Ok((window.into(), view.unwrap()))
}
struct Root {
    content: AnyView,
    overlays: Entity<menu::Overlays>,
}
impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .on_key_down(|event, window, cx| {
                if event.keystroke.key == "tab" && !event.keystroke.modifiers.control {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            })
            .relative()
            .child(self.content.clone())
            .child(self.overlays.clone())
    }
}

pub trait StyledExt: Styled + Sized {
    fn h_flex(self) -> Self {
        self.flex().flex_row().items_center()
    }
    fn v_flex(self) -> Self {
        self.flex().flex_col()
    }
}
impl<T: Styled> StyledExt for T {}

#[derive(Clone, Copy)]
pub enum ThemeMode {
    Light,
    Dark,
}
#[derive(Clone, Copy)]
pub struct Colors {
    pub background: Hsla,
    pub foreground: Hsla,
    pub sidebar: Hsla,
    pub border: Hsla,
    pub primary: Hsla,
    pub selection: Hsla,
    pub popover: Hsla,
    pub popover_foreground: Hsla,
    pub accent: Hsla,
    pub accent_foreground: Hsla,
    pub muted_foreground: Hsla,
    pub primary_foreground: Hsla,
    pub ring: Hsla,
    pub primary_hover: Hsla,
    pub primary_active: Hsla,
    pub button_primary: Hsla,
    pub button_primary_foreground: Hsla,
    pub button_primary_hover: Hsla,
    pub button_primary_active: Hsla,
    pub list: Hsla,
    pub list_even: Hsla,
    pub list_head: Hsla,
    pub list_hover: Hsla,
    pub list_active: Hsla,
    pub list_active_border: Hsla,
}
impl Default for Colors {
    fn default() -> Self {
        let bg = rgb(0xffffff).into();
        let fg = rgb(0x1f2937).into();
        let primary = rgb(0x2674f5).into();
        Self {
            background: bg,
            foreground: fg,
            sidebar: rgb(0xf5f7fa).into(),
            border: rgb(0xdce2ea).into(),
            primary,
            selection: rgb(0xddebff).into(),
            popover: bg,
            popover_foreground: fg,
            accent: rgb(0xe8edf5).into(),
            accent_foreground: fg,
            muted_foreground: rgb(0x657187).into(),
            primary_foreground: bg,
            ring: primary,
            primary_hover: primary,
            primary_active: primary,
            button_primary: primary,
            button_primary_foreground: bg,
            button_primary_hover: primary,
            button_primary_active: primary,
            list: bg,
            list_even: bg,
            list_head: bg,
            list_hover: bg,
            list_active: bg,
            list_active_border: primary,
        }
    }
}
#[derive(Clone)]
pub struct Theme {
    pub colors: Colors,
    pub font_family: SharedString,
    pub font_size: Pixels,
    pub mono_font_family: SharedString,
    pub mono_font_size: Pixels,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            colors: Colors::default(),
            font_family: "Segoe UI".into(),
            font_size: px(15.),
            mono_font_family: "Consolas".into(),
            mono_font_size: px(18.),
        }
    }
}
impl std::ops::Deref for Theme {
    type Target = Colors;
    fn deref(&self) -> &Colors {
        &self.colors
    }
}
impl Global for Theme {}
impl Theme {
    pub fn change(_: ThemeMode, _: Option<&mut Window>, _: &mut App) {}
    pub fn update(cx: &mut App, f: impl FnOnce(&mut Self)) {
        cx.update_global::<Self, _>(|theme, _| f(theme));
        cx.refresh_windows();
    }
}
pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}
impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

#[derive(Clone, Copy)]
pub enum IconName {
    ChevronsLeft,
    ChevronsRight,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    FolderOpen,
    Folder,
    FileText,
    Close,
    Plus,
    Settings,
    RefreshCw,
    ChevronsUpDown,
    ArrowRight,
}
impl IconName {
    fn path(self) -> &'static str {
        match self {
            Self::ChevronsLeft => "chevrons-left",
            Self::ChevronsRight => "chevrons-right",
            Self::ChevronDown => "chevron-down",
            Self::ChevronRight => "chevron-right",
            Self::ChevronUp => "chevron-up",
            Self::FolderOpen => "folder-open",
            Self::Folder => "folder",
            Self::FileText => "file-text",
            Self::Close => "x",
            Self::Plus => "plus",
            Self::Settings => "settings",
            Self::RefreshCw => "refresh-cw",
            Self::ChevronsUpDown => "chevrons-up-down",
            Self::ArrowRight => "arrow-right",
        }
    }
}
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    style: StyleRefinement,
}
impl Icon {
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            style: StyleRefinement::default(),
        }
    }
}
impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let mut el = svg()
            .path(self.name.path())
            .size(px(16.))
            .flex_shrink_0()
            .text_color(window.text_style().color);
        el.style().refine(&self.style);
        el
    }
}
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "chevrons-left" => include_bytes!("../assets/icons/chevrons-left.svg"),
            "chevrons-right" => include_bytes!("../assets/icons/chevrons-right.svg"),
            "chevron-down" => include_bytes!("../assets/icons/chevron-down.svg"),
            "chevron-right" => include_bytes!("../assets/icons/chevron-right.svg"),
            "chevron-up" => include_bytes!("../assets/icons/chevron-up.svg"),
            "folder-open" => include_bytes!("../assets/icons/folder-open.svg"),
            "folder" => include_bytes!("../assets/icons/folder.svg"),
            "file-text" => include_bytes!("../assets/icons/file-text.svg"),
            "x" => include_bytes!("../assets/icons/x.svg"),
            "plus" => include_bytes!("../assets/icons/plus.svg"),
            "settings" => include_bytes!("../assets/icons/settings.svg"),
            "refresh-cw" => include_bytes!("../assets/icons/refresh-cw.svg"),
            "chevrons-up-down" => include_bytes!("../assets/icons/chevrons-up-down.svg"),
            "arrow-right" => include_bytes!("../assets/icons/arrow-right.svg"),
            _ => return Ok(None),
        };
        Ok(Some(std::borrow::Cow::Borrowed(bytes)))
    }
    fn list(&self, _: &str) -> Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    tip: Option<SharedString>,
    click: Option<ClickHandler>,
    style: StyleRefinement,
    primary: bool,
    selected: bool,
    disabled: bool,
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
            selected: false,
            disabled: false,
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
    pub fn ghost(self) -> Self {
        self
    }
    pub fn small(mut self) -> Self {
        self.compact = true;
        self
    }
    pub fn xsmall(self) -> Self {
        self.small()
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
        let disabled = self.disabled;
        let click = self.click.clone();
        let keyboard = self.click;
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
            .tab_stop(self.tab_stop && !disabled)
            .text_sm()
            .aria_label(self.tip.clone().unwrap_or(self.label.clone()))
            .text_color(if self.primary {
                theme.primary_foreground
            } else {
                theme.foreground
            })
            .when(self.primary, |d| d.bg(theme.primary))
            .when(self.selected, |d| {
                d.bg(theme.selection).text_color(theme.primary)
            })
            .when(disabled, |d| d.opacity(0.4))
            .hover(move |d| {
                d.bg(if self.primary {
                    theme.primary_hover
                } else {
                    theme.accent
                })
            })
            .focus_visible(|s| s.border_1().border_color(theme.primary))
            .on_click(move |e, w, cx| {
                cx.stop_propagation();
                if !disabled && let Some(f) = &click {
                    f(e, w, cx);
                }
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(move |e, w, cx| {
                if !disabled && matches!(e.keystroke.key.as_str(), "enter" | "space") {
                    if let Some(f) = &keyboard {
                        f(&ClickEvent::default(), w, cx);
                    }
                    cx.stop_propagation();
                }
            })
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
            .text_sm()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.0.clone())
    }
}
#[derive(IntoElement)]
pub struct Switch {
    button: Button,
    checked: bool,
    label: SharedString,
}
impl Switch {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            button: Button::new(id),
            checked: false,
            label: "".into(),
        }
    }
    pub fn checked(mut self, value: bool) -> Self {
        self.checked = value;
        self
    }
    pub fn label(mut self, value: impl Into<SharedString>) -> Self {
        self.label = value.into();
        self
    }
    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        let checked = self.checked;
        self.button = self
            .button
            .on_click(move |_, w, cx| handler(&!checked, w, cx));
        self
    }
}
impl RenderOnce for Switch {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.button
            .label(format!(
                "{}  {}",
                if self.checked { "☑" } else { "☐" },
                self.label
            ))
            .selected(self.checked)
    }
}
