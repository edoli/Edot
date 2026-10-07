//! Headless GPUI event tests. They never send input to the user's desktop.
use super::{Edot, Settings, StoreLock};
use crate::{
    editing::CharRange, global_search::MatchKey, model::Notebook, session::Session,
    settings::ThemeMode,
};
use gpui::{
    AnyWindowHandle, AppContext, Bounds, Context, ElementInputHandler, Entity, InputHandler,
    TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};
use std::{fs, path::PathBuf};

trait TestWindowExt {
    fn render_frame(&mut self, cx: &mut gpui::App);
    fn press(&mut self, key: &str, cx: &mut gpui::App);
    fn input(&mut self, text: &str, cx: &mut gpui::App);
}
impl TestWindowExt for Window {
    fn render_frame(&mut self, cx: &mut gpui::App) {
        self.refresh();
        self.draw(cx).clear(cx);
    }
    fn press(&mut self, key: &str, cx: &mut gpui::App) {
        use gpui::InputEvent;
        let stroke = gpui::Keystroke::parse(key).unwrap();
        if matches!(stroke.key.as_str(), "enter" | "tab" | "space") {
            self.dispatch_event(
                gpui::KeyDownEvent {
                    keystroke: stroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                }
                .to_platform_input(),
                cx,
            );
        } else {
            self.dispatch_keystroke(stroke.clone().with_simulated_ime(), cx);
        }
        self.dispatch_event(
            gpui::KeyUpEvent { keystroke: stroke }.to_platform_input(),
            cx,
        );
    }
    fn input(&mut self, text: &str, cx: &mut gpui::App) {
        for ch in text.chars() {
            self.render_frame(cx);
            let mut key = gpui::Keystroke::parse(&ch.to_string()).unwrap();
            key.key_char = Some(ch.to_string());
            self.dispatch_keystroke(key, cx);
        }
        self.render_frame(cx);
    }
}

struct Fixture {
    window: AnyWindowHandle,
    view: Entity<Edot>,
    dir: PathBuf,
    ids: [u64; 3],
}
impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "edot-gpui-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let lock = StoreLock::acquire(&dir).unwrap();
        let mut book = Notebook::load(&dir).unwrap();
        let first = book.add(0, "첫 메모".into());
        let second = book.add(0, "둘째 메모".into());
        let third = book.add(first, "하위 메모".into());
        for (id, text) in [
            (first, "🙂 한글12\r\n한글3\rkeep\n"),
            (second, "closed needle\n"),
            (third, "child needle\n"),
        ] {
            fs::write(book.file(&dir, id), text).unwrap();
        }
        book.save(&dir).unwrap();
        Session {
            tabs: vec![first, second],
            active: Some(first),
            ..Default::default()
        }
        .save(&dir)
        .unwrap();
        let (window, view) = cx.update(|cx| {
            crate::widgets::init(cx);
            crate::widgets::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(1100.), px(760.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        Edot::new(
                            dir.clone(),
                            Ok((lock, book, Settings::default())),
                            window,
                            cx,
                        )
                    })
                },
            )
            .unwrap()
        });
        Self {
            window,
            view,
            dir,
            ids: [first, second, third],
        }
    }
    fn update<R>(
        &self,
        cx: &mut TestAppContext,
        update: impl FnOnce(&mut Edot, &mut Window, &mut Context<Edot>) -> R,
    ) -> R {
        cx.update_window(self.window, |_, window, cx| {
            self.view.update(cx, |this, cx| update(this, window, cx))
        })
        .unwrap()
    }
    fn press(&self, cx: &mut TestAppContext, key: &str) {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.simulate_next_frame(cx);
            window.render_frame(cx);
            window.press(key, cx);
        })
        .unwrap();
    }
    fn input(&self, cx: &mut TestAppContext, text: &str) {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.input(text, cx);
        })
        .unwrap();
    }
}

mod documents;
mod editor;
mod layout;
mod menus;
mod preferences;
mod search;
mod tree;
