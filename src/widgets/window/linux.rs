//! Prefer compositor decorations; provide a header and resize edges when unavailable.
use super::*;

type CloseHandler = Rc<dyn Fn(&mut Window, &mut App)>;

pub fn frame(
    content: AnyElement,
    title: SharedString,
    window: &mut Window,
    cx: &mut App,
    on_close: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    if !window.is_fullscreen()
        && let Decorations::Client { tiling } = window.window_decorations()
    {
        client_frame(content, title, tiling, window, cx, Rc::new(on_close))
    } else {
        content
    }
}

fn client_frame(
    content: AnyElement,
    title: SharedString,
    tiling: Tiling,
    window: &Window,
    cx: &App,
    on_close: CloseHandler,
) -> AnyElement {
    let colors = cx.theme().colors;
    let layout = cx.button_layout().unwrap_or(WindowButtonLayout {
        left: [None; MAX_BUTTONS_PER_SIDE],
        right: [
            Some(WindowButton::Minimize),
            Some(WindowButton::Maximize),
            Some(WindowButton::Close),
        ],
    });
    let group = |buttons: [Option<WindowButton>; MAX_BUTTONS_PER_SIDE]| {
        let supported = window.window_controls();
        div()
            .h_flex()
            .flex_1()
            .gap(px(4.))
            .children(buttons.into_iter().flatten().filter_map(|button| {
                if matches!(button, WindowButton::Minimize) && !supported.minimize
                    || matches!(button, WindowButton::Maximize) && !supported.maximize
                {
                    return None;
                }
                Some(control(
                    button,
                    window.is_maximized(),
                    colors,
                    on_close.clone(),
                ))
            }))
    };
    let titlebar = div()
        .id("linux-titlebar")
        .debug_selector(|| "linux-titlebar".into())
        .h_flex()
        .h(px(40.))
        .flex_shrink_0()
        .px(px(8.))
        .gap(px(8.))
        .bg(colors.bar)
        .text_color(colors.foreground)
        .border_b_1()
        .border_color(colors.border)
        .on_mouse_down(MouseButton::Left, |event, window, cx| {
            if event.click_count == 2 && window.window_controls().maximize {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
            cx.stop_propagation();
        })
        .on_mouse_down(MouseButton::Right, |event, window, cx| {
            if window.window_controls().window_menu {
                window.show_window_menu(event.position);
            }
            cx.stop_propagation();
        })
        .child(group(layout.left).justify_start())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_center()
                .text_ellipsis()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(group(layout.right).justify_end());
    let mut frame = div()
        .id("linux-window-frame")
        .size_full()
        .relative()
        .v_flex()
        .bg(colors.background)
        .child(titlebar)
        .child(div().flex_1().min_h_0().overflow_hidden().child(content));
    if window.is_resizable() {
        for (edge, enabled) in [
            (ResizeEdge::Top, !tiling.top),
            (ResizeEdge::Bottom, !tiling.bottom),
            (ResizeEdge::Left, !tiling.left),
            (ResizeEdge::Right, !tiling.right),
            (ResizeEdge::TopLeft, !tiling.top && !tiling.left),
            (ResizeEdge::TopRight, !tiling.top && !tiling.right),
            (ResizeEdge::BottomLeft, !tiling.bottom && !tiling.left),
            (ResizeEdge::BottomRight, !tiling.bottom && !tiling.right),
        ] {
            if enabled {
                frame = frame.child(resize_handle(edge));
            }
        }
    }
    frame.into_any_element()
}

fn control(
    button: WindowButton,
    maximized: bool,
    colors: theme::Colors,
    close: CloseHandler,
) -> Stateful<Div> {
    let (id, label, path) = match button {
        WindowButton::Minimize => ("linux-minimize", "최소화", "linux-window-minimize"),
        WindowButton::Maximize if maximized => {
            ("linux-maximize", "이전 크기로 복원", "linux-window-restore")
        }
        WindowButton::Maximize => ("linux-maximize", "최대화", "linux-window-maximize"),
        WindowButton::Close => ("linux-close", "닫기", "x"),
    };
    div()
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Button)
        .aria_label(label)
        .h_flex()
        .justify_center()
        .size(px(28.))
        .flex_shrink_0()
        .rounded_full()
        .occlude()
        .when(button == WindowButton::Close, |button| {
            button.bg(colors.accent)
        })
        .hover(move |button| button.bg(colors.selection))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            match button {
                WindowButton::Minimize => window.minimize_window(),
                WindowButton::Maximize => window.zoom_window(),
                WindowButton::Close => close(window, cx),
            }
            cx.stop_propagation();
        })
        .child(svg().path(path).size(px(14.)).text_color(colors.foreground))
}

fn resize_handle(edge: ResizeEdge) -> Div {
    let (handle, cursor) = match edge {
        ResizeEdge::Top => (
            div().top_0().left_0().right_0().h(px(4.)),
            CursorStyle::ResizeUpDown,
        ),
        ResizeEdge::Bottom => (
            div().bottom_0().left_0().right_0().h(px(4.)),
            CursorStyle::ResizeUpDown,
        ),
        ResizeEdge::Left => (
            div().left_0().top_0().bottom_0().w(px(4.)),
            CursorStyle::ResizeLeftRight,
        ),
        ResizeEdge::Right => (
            div().right_0().top_0().bottom_0().w(px(4.)),
            CursorStyle::ResizeLeftRight,
        ),
        ResizeEdge::TopLeft => (
            div().top_0().left_0().size(px(8.)),
            CursorStyle::ResizeUpLeftDownRight,
        ),
        ResizeEdge::TopRight => (
            div().top_0().right_0().size(px(8.)),
            CursorStyle::ResizeUpRightDownLeft,
        ),
        ResizeEdge::BottomLeft => (
            div().bottom_0().left_0().size(px(8.)),
            CursorStyle::ResizeUpRightDownLeft,
        ),
        ResizeEdge::BottomRight => (
            div().bottom_0().right_0().size(px(8.)),
            CursorStyle::ResizeUpLeftDownRight,
        ),
    };
    handle.absolute().occlude().cursor(cursor).on_mouse_down(
        MouseButton::Left,
        move |_, window, cx| {
            window.start_window_resize(edge);
            cx.stop_propagation();
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{client_frame, frame};
    use gpui::{
        AppContext, Context, Modifiers, Render, TestAppContext, Tiling, VisualTestContext, Window,
        WindowOptions, div, prelude::*,
    };
    use std::{cell::Cell, rc::Rc};

    struct Preview {
        closed: Rc<Cell<bool>>,
        client: bool,
    }

    impl Render for Preview {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let content = div().id("test-body").size_full().into_any_element();
            let closed = self.closed.clone();
            if self.client {
                client_frame(
                    content,
                    "Edot — Test".into(),
                    Tiling::default(),
                    window,
                    cx,
                    Rc::new(move |_, _| closed.set(true)),
                )
            } else {
                frame(content, "Edot — Test".into(), window, cx, move |_, _| {
                    closed.set(true)
                })
            }
        }
    }

    #[gpui::test]
    fn client_header_owns_close_and_server_frame_has_no_duplicate_header(cx: &mut TestAppContext) {
        let closed = Rc::new(Cell::new(false));
        let window = cx.update(|cx| {
            crate::widgets::init(cx);
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|_| Preview {
                    closed: closed.clone(),
                    client: true,
                })
            })
            .unwrap()
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let close = visual.debug_bounds("linux-close").unwrap().center();
        visual.simulate_click(close, Modifiers::default());
        assert!(closed.get());
        window
            .update(cx, |preview, _, cx| {
                preview.client = false;
                cx.notify();
            })
            .unwrap();
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        assert!(visual.debug_bounds("linux-titlebar").is_none());
    }
}
