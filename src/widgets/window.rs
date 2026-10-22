use super::*;

#[cfg(any(target_os = "linux", test))]
pub(crate) mod linux;

#[cfg(target_os = "windows")]
const CAPTION_BUTTON_WIDTH: f32 = 46.;
#[cfg(target_os = "windows")]
pub const WINDOW_CONTROLS_WIDTH: f32 = CAPTION_BUTTON_WIDTH * 3.;
#[cfg(not(target_os = "windows"))]
pub const WINDOW_CONTROLS_WIDTH: f32 = 0.;

pub fn has_inline_window_controls(window: &Window) -> bool {
    cfg!(target_os = "windows") && !window.is_fullscreen()
}

#[derive(IntoElement)]
pub struct WindowControls;

impl RenderOnce for WindowControls {
    #[cfg(target_os = "windows")]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let maximized = window.is_maximized();
        let colors = cx.theme().colors;
        let button = |id, area, icon, label: &'static str| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .h_flex()
                .justify_center()
                .w(px(CAPTION_BUTTON_WIDTH))
                .h_full()
                .flex_shrink_0()
                .occlude()
                // Native non-client handling keeps Snap Layouts, system commands,
                // and WM_CLOSE's save/veto callback on the same path as Alt+F4.
                .window_control_area(area)
                .hover(move |s| {
                    if area == WindowControlArea::Close {
                        s.bg(rgb(0xe81123)).text_color(gpui::white())
                    } else {
                        s.bg(colors.accent)
                    }
                })
                .active(move |s| s.bg(colors.selection))
                .font_family("Segoe MDL2 Assets")
                .text_size(px(10.))
                .line_height(px(10.))
                .child(icon)
        };
        div()
            .h_flex()
            .h_full()
            .flex_shrink_0()
            .text_color(colors.foreground)
            .child(button(
                "window-minimize",
                WindowControlArea::Min,
                "\u{e921}",
                "최소화",
            ))
            .child(button(
                "window-maximize",
                WindowControlArea::Max,
                if maximized { "\u{e923}" } else { "\u{e922}" },
                if maximized {
                    "이전 크기로 복원"
                } else {
                    "최대화"
                },
            ))
            .child(button(
                "window-close",
                WindowControlArea::Close,
                "\u{e8bb}",
                "닫기 · Alt+F4",
            ))
    }

    #[cfg(not(target_os = "windows"))]
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
    }
}

pub fn open_window<V: Render + 'static>(
    options: WindowOptions,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> Result<(AnyWindowHandle, Entity<V>)> {
    let mut view = None;
    let window = cx.open_window(options, |window, cx| {
        #[cfg(target_os = "windows")]
        if let Err(error) = crate::platform::suppress_caption_tooltips(window) {
            eprintln!("창 컨트롤 툴팁을 비활성화하지 못했습니다: {error}");
        }
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
            .font_family(theme::UI_FONT)
            .text_size(px(theme::UI_FONT_SIZE))
            .capture_key_down(|event, window, cx| {
                if event.keystroke.key == "f11" && event.keystroke.modifiers == Modifiers::default()
                {
                    if !event.is_held {
                        window.toggle_fullscreen();
                    }
                    cx.stop_propagation();
                }
            })
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
