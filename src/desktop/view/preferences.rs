//! Preferences presentation.
use super::*;

impl Edot {
    pub(in crate::desktop) fn settings_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let prefs = self.preferences.as_ref().unwrap();
        div()
            .id("settings-panel")
            .debug_selector(|| "settings-panel".into())
            .track_focus(&prefs.focus)
            .occlude()
            .absolute()
            .top_2()
            .right_3()
            .w(px(360.))
            .max_w_full()
            .max_h(relative(0.9))
            .overflow_y_scroll()
            .v_flex()
            .gap_5()
            .p_5()
            .text_left()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .shadow_lg()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.settings_visible = false;
                    this.focus_editor(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(
                div()
                    .h_flex()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("설정"),
                    )
                    .child(
                        tool("settings-close", IconName::Close, "설정 닫기").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.settings_visible = false;
                                this.focus_editor(window, cx);
                                cx.notify();
                            },
                        )),
                    ),
            )
            .child(
                div().v_flex().gap_1().child(div().child("앱 테마")).child(
                    Select::new(&prefs.theme)
                        .w_full()
                        .accessibility_label("테마"),
                ),
            )
            .child(
                div()
                    .h_flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .debug_selector(|| "settings-font".into())
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().child("본문 글꼴"))
                            .child(
                                Select::new(&prefs.font)
                                    .w_full()
                                    .accessibility_label("본문 글꼴"),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "settings-size".into())
                            .v_flex()
                            .w(px(92.))
                            .flex_shrink_0()
                            .gap_1()
                            .child(div().child("글자 크기"))
                            .child(
                                Select::new(&prefs.size)
                                    .w_full()
                                    .accessibility_label("글자 크기"),
                            ),
                    ),
            )
            .child(
                Switch::new("word-wrap")
                    .track_focus(&prefs.wrap_focus)
                    .label("자동 줄 바꿈")
                    .checked(cx.global::<AppState>().settings.wrap)
                    .on_change(cx.listener(|this, value, window, cx| {
                        let mut next = cx.global::<AppState>().settings.clone();
                        next.wrap = *value;
                        this.change_settings(next, window, cx);
                    })),
            )
            .child(self.update_view(cx))
            .into_any_element()
    }

    fn update_view(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::update::{self, State};
        let state = cx.global::<AppState>();
        let version = if update::BUILD_LABEL.is_empty() {
            format!("Edot v{}", update::VERSION)
        } else {
            format!("Edot v{} · {}", update::VERSION, update::BUILD_LABEL)
        };
        let (label, available) = match &state.update_state {
            State::Unchecked => ("업데이트 확인 전".to_owned(), false),
            State::Checking => ("최신 버전 확인 중…".to_owned(), false),
            State::Current => ("최신 버전".to_owned(), false),
            State::NoRelease => ("공개 릴리스 없음".to_owned(), false),
            State::Available(release) => (format!("업데이트 · v{}", release.version), true),
            #[cfg(target_os = "windows")]
            State::Installing => ("업데이트 준비 중…".to_owned(), false),
            State::Failed(_) => ("확인 실패".to_owned(), false),
        };
        let message = match &state.update_state {
            State::Failed(error) => Some(error.clone()),
            _ => state.update_message.clone(),
        };
        div()
            .id("settings-version")
            .debug_selector(|| "settings-version".into())
            .v_flex()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("settings-version-label")
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(cx.theme().muted_foreground)
                            .child(version),
                    )
                    .when(available, |view| {
                        view.child(
                            Button::new("settings-update")
                                .small()
                                .flex_shrink_0()
                                .whitespace_nowrap()
                                .label(label.clone())
                                .primary()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.install_update(window, cx);
                                })),
                        )
                    })
                    .when(!available, |view| {
                        view.child(
                            div()
                                .flex_shrink_0()
                                .whitespace_nowrap()
                                .text_color(cx.theme().muted_foreground)
                                .child(label),
                        )
                    }),
            )
            .when_some(message, |view, message| view.child(div().child(message)))
            .into_any_element()
    }
}
