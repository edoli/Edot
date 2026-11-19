//! Interactive Windows IME regression probe; driven by tools/test_ime_cursor.py.
use crate::widgets::{InputState, Textarea};
use gpui::{
    AppContext, Context, Entity, EntityInputHandler, KeyDownEvent, Render, Window, WindowBounds,
    WindowOptions, div, point, prelude::*, px, size,
};
use std::path::PathBuf;

struct ImeProbe {
    input: Entity<InputState>,
    directory: PathBuf,
}

impl Render for ImeProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .text_size(px(16.))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "f2" => this.input.update(cx, |input, cx| {
                        input.set_value("FIRST\nLAST ", window, cx);
                        let end = input.value().len();
                        input.set_selected_range(end..end, cx);
                        // Model a missed completion: the editor retains a preedit
                        // although the native IME no longer has a composition.
                        input.replace_and_mark_text_in_range(None, "가", Some(1..1), window, cx);
                        input.focus(window, cx);
                    }),
                    "f4" => this.input.update(cx, |input, cx| {
                        input.set_selected_range(0..0, cx);
                    }),
                    "f6" => {
                        let state = this.input.update(cx, |input, cx| {
                            let bounds = input.bounds_for_range(0..0, Default::default(), window, cx).unwrap();
                            serde_json::json!({
                                "text": input.value().as_ref(),
                                "selection": [input.selected_range().start, input.selected_range().end],
                                "composing": input.is_composing(),
                                "start": [bounds.left().as_f32() + 2., bounds.top().as_f32() + bounds.size.height.as_f32() / 2.],
                            })
                        });
                        std::fs::write(this.directory.join("state.json"), state.to_string()).unwrap();
                    }
                    "f8" => cx.quit(),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(Textarea::new(&self.input).h_full().appearance(false))
    }
}

#[test]
#[ignore = "interactive Windows probe; run with tools/test_ime_cursor.py"]
fn native_windows_ime_cursor() {
    let directory = PathBuf::from(std::env::var_os("EDOT_IME_PROBE_DIR").unwrap());
    assert!(directory.is_absolute() && directory.is_dir());
    gpui_platform::application().run(move |cx| {
        crate::widgets::init(cx);
        cx.open_window(
            WindowOptions {
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Edot IME regression probe".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(gpui::Bounds::new(
                    point(px(30.), px(30.)),
                    size(px(700.), px(400.)),
                ))),
                ..Default::default()
            },
            move |window, cx| {
                let input = cx.new(|cx| InputState::new(window, cx).soft_wrap(false));
                input.update(cx, |input, cx| input.focus(window, cx));
                cx.new(|_| ImeProbe { input, directory })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
