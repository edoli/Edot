//! Sidebar Space switcher and management popup.
use super::*;

impl Edot {
    pub(in crate::desktop) fn space_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let bounds = self.space_ui.bounds.clone();
        div()
            .id("space-footer")
            .debug_selector(|| "space-footer".into())
            .relative()
            .h(px(44.))
            .flex_shrink_0()
            .border_t_1()
            .border_color(cx.theme().border)
            .p_1()
            .child(
                canvas(move |rect, _, _| bounds.set(rect), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .child(
                Button::new("space-toggle")
                    .icon(IconName::ChevronsUpDown)
                    .label(self.current_space_name())
                    .tooltip(format!("Space · {}", self.current_space_name()))
                    .selected(self.space_ui.open)
                    .w_full()
                    .h_full()
                    .justify_start()
                    .text_ellipsis()
                    .overflow_hidden()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_space_list(window, cx)),
                    ),
            )
            .into_any_element()
    }

    pub(in crate::desktop) fn space_popup(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let width = (f32::from(window.viewport_size().width) - 16.).clamp(0., 380.);
        let height = (f32::from(window.viewport_size().height) - 132.).clamp(0., 360.);
        let mut list = div()
            .id("space-list")
            .v_flex()
            .gap_1()
            .max_h(px(height))
            .overflow_y_scroll()
            .track_scroll(&self.space_ui.scroll);
        if self.space_ui.registry.entries.is_empty() {
            list = list.child(
                div()
                    .p_3()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("등록된 Space가 없습니다."),
            );
        }
        for (index, entry) in self.space_ui.registry.entries.iter().enumerate() {
            let path = entry.path.clone();
            let current = space::same_path(&path, &self.dir);
            let rename = self
                .space_ui
                .rename
                .as_ref()
                .filter(|(target, _)| space::same_path(target, &path));
            let mut row = div()
                .id(("space-row", index))
                .debug_selector(move || format!("space-row-{index}"))
                .h_flex()
                .gap_1()
                .p_1()
                .flex_shrink_0()
                .rounded_md()
                .border_1()
                .border_color(if index == self.space_ui.cursor && !current {
                    cx.theme().primary
                } else {
                    transparent_black()
                })
                .child(
                    div()
                        .h_flex()
                        .w(px(16.))
                        .h(px(32.))
                        .flex_shrink_0()
                        .when(current, |mark| mark.child(Icon::new(IconName::Check))),
                );
            if let Some((_, input)) = rename {
                row = row.child(
                    div()
                        .h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            Input::new(input)
                                .aria_label("Space 이름")
                                .flex_1()
                                .min_w_0(),
                        )
                        .child(
                            Button::new(("space-name-save", index))
                                .small()
                                .label("저장")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.finish_space_rename(window, cx)
                                })),
                        ),
                );
            } else {
                let select_path = path.clone();
                row = row.child(
                    Button::new(("space-select", index))
                        .label(entry.name.clone())
                        .tooltip(entry.name.clone())
                        .flex_1()
                        .min_w_0()
                        .justify_start()
                        .text_ellipsis()
                        .overflow_hidden()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_registered_space(select_path.clone(), window, cx);
                        })),
                );
            }
            let folder_path = path.clone();
            let rename_path = path.clone();
            row = row.child(
                div()
                    .h_flex()
                    .flex_shrink_0()
                    .gap_1()
                    .child(
                        Button::new(("space-folder", index))
                            .small()
                            .icon(IconName::FolderOpen)
                            .tooltip("폴더 열기")
                            .w(px(28.))
                            .h(px(28.))
                            .px_0()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_space_folder(&folder_path, cx);
                            })),
                    )
                    .child(
                        Button::new(("space-rename", index))
                            .small()
                            .icon(IconName::Pencil)
                            .tooltip("이름 바꾸기")
                            .w(px(28.))
                            .h(px(28.))
                            .px_0()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.begin_space_rename(rename_path.clone(), window, cx);
                            })),
                    )
                    .child(
                        Button::new(("space-remove", index))
                            .small()
                            .icon(IconName::Trash)
                            .tooltip("목록에서 삭제")
                            .w(px(28.))
                            .h(px(28.))
                            .px_0()
                            .text_color(cx.theme().destructive)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_remove_space(path.clone(), window, cx);
                            })),
                    ),
            );
            list = list.child(row);
        }
        div()
            .id("space-popup")
            .debug_selector(|| "space-popup".into())
            .absolute()
            .left(px(8.))
            .bottom(px(48.))
            .w(px(width))
            .v_flex()
            .gap_2()
            .p_2()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .shadow_lg()
            .occlude()
            .track_focus(&self.space_ui.focus)
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if !window.has_active_dialog(cx)
                    && !this.space_ui.bounds.get().contains(&event.position)
                {
                    this.close_space_list(window, cx);
                }
            }))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if window.has_active_dialog(cx) {
                    return;
                }
                let key = event.keystroke.key.as_str();
                if let Some((_, input)) = &this.space_ui.rename {
                    if input.read(cx).is_composing() {
                        return;
                    }
                    match key {
                        "enter" => this.finish_space_rename(window, cx),
                        "escape" => {
                            this.space_ui.rename = None;
                            this.space_ui.focus.focus(window, cx);
                            cx.notify();
                        }
                        _ => return,
                    }
                } else {
                    match key {
                        "escape" => this.close_space_list(window, cx),
                        "up" | "down" if this.space_ui.focus.is_focused(window) => {
                            let count = this.space_ui.registry.entries.len();
                            if count > 0 {
                                this.space_ui.cursor = if key == "up" {
                                    (this.space_ui.cursor + count - 1) % count
                                } else {
                                    (this.space_ui.cursor + 1) % count
                                };
                                this.space_ui.scroll.scroll_to_item(this.space_ui.cursor);
                                cx.notify();
                            }
                        }
                        "enter" if this.space_ui.focus.is_focused(window) => {
                            if let Some(entry) =
                                this.space_ui.registry.entries.get(this.space_ui.cursor)
                            {
                                this.select_registered_space(entry.path.clone(), window, cx);
                            }
                        }
                        _ => return,
                    }
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Space"),
            )
            .child(list)
            .child(
                Button::new("space-add")
                    .icon(IconName::Plus)
                    .label("Space 추가")
                    .w_full()
                    .justify_start()
                    .on_click(cx.listener(|this, _, window, cx| this.open_space(window, cx))),
            )
            .into_any_element()
    }
}
