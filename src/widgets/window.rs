use super::*;

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
