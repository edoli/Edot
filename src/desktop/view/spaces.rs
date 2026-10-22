//! Sidebar Space switcher and management popup.
use super::*;

impl Edot {
    pub(in crate::desktop) fn space_recovery_view(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let height = (f32::from(window.viewport_size().height) - 300.).clamp(80., 360.);
        let body = div()
            .id("space-recovery")
            .debug_selector(|| "space-recovery".into())
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .p_4()
            .v_flex()
            .items_center()
            .overflow_y_scroll()
            .track_focus(&self.focus)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(component::theme::UI_FONT)
            .child(
                div()
                    .v_flex()
                    .w(px(460.))
                    .max_w_full()
                    .my_auto()
                    .flex_shrink_0()
                    .gap_4()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Space를 열지 못했습니다"),
                    )
                    .child(div().child(self.error.clone().unwrap_or_default()))
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child("다른 Space를 선택하거나 새로 만들어 계속할 수 있습니다."),
                    )
                    .child(self.space_panel(height, cx)),
            )
            .when(self.settings_visible, |body| {
                body.child(self.settings_view(cx))
            });
        div()
            .id("space-recovery-window")
            .size_full()
            .v_flex()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !window.has_active_dialog(cx)
                    && event.keystroke.modifiers.control
                    && event.keystroke.key == ","
                {
                    if !event.is_held {
                        this.show_settings(window, cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .h_flex()
                    .h(px(36.))
                    .flex_shrink_0()
                    .bg(cx.theme().bar)
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .when(component::has_inline_window_controls(window), |area| {
                                area.window_control_area(WindowControlArea::Drag)
                            }),
                    )
                    .child(
                        div()
                            .debug_selector(|| "recovery-preferences".into())
                            .w(px(40.))
                            .h(px(34.))
                            .flex_shrink_0()
                            .child(
                                tool("preferences", IconName::Settings, "설정 · Ctrl+,")
                                    .size_full()
                                    .selected(self.settings_visible)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_settings(window, cx)
                                    })),
                            ),
                    )
                    .when(component::has_inline_window_controls(window), |row| {
                        row.child(
                            div()
                                .w(px(WINDOW_DRAG_WIDTH))
                                .h_full()
                                .flex_shrink_0()
                                .window_control_area(WindowControlArea::Drag),
                        )
                        .child(component::WindowControls)
                    }),
            )
            .child(body)
    }

    pub(in crate::desktop) fn space_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let bounds = self.space_ui.bounds.clone();
        div()
            .id("space-footer")
            .debug_selector(|| "space-footer".into())
            .relative()
            .h_flex()
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
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .justify_start()
                    .text_ellipsis()
                    .overflow_hidden()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_space_list(window, cx)),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "trash-open".into())
                    .w(px(36.))
                    .h_full()
                    .flex_shrink_0()
                    .child(
                        Button::new("open-trash")
                            .icon(IconName::ArchiveRestore)
                            .tooltip("휴지통 열기")
                            .w_full()
                            .h_full()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.show_trash(window, cx)),
                            ),
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
        div()
            .id("space-popup")
            .debug_selector(|| "space-popup".into())
            .absolute()
            .left(px(8.))
            .bottom(px(48.))
            .w(px(width))
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if !window.has_active_dialog(cx)
                    && !this.space_ui.bounds.get().contains(&event.position)
                {
                    this.close_space_list(window, cx);
                }
            }))
            .child(self.space_panel(height, cx))
            .into_any_element()
    }

    fn space_panel(&self, height: f32, cx: &mut Context<Self>) -> Stateful<Div> {
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
                    .text_color(cx.theme().muted_foreground)
                    .child("등록된 Space가 없습니다."),
            );
        }
        for (index, entry) in self.space_ui.registry.entries.iter().enumerate() {
            let path = entry.path.clone();
            let current = !self.startup_failed && space::same_path(&path, &self.dir);
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
            .id("space-panel")
            .debug_selector(|| "space-panel".into())
            .w_full()
            .v_flex()
            .gap_2()
            .p_2()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .track_focus(&self.space_ui.focus)
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
                        "escape" if !this.startup_failed => this.close_space_list(window, cx),
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
                div()
                    .w_full()
                    .h(px(1.))
                    .flex_shrink_0()
                    .bg(cx.theme().border),
            )
            .child(
                Button::new("space-add")
                    .icon(IconName::Plus)
                    .label("Space 추가")
                    .w_full()
                    .justify_start()
                    .on_click(cx.listener(|this, _, window, cx| this.open_space(window, cx))),
            )
            .child(
                Button::new("space-create")
                    .icon(IconName::Folder)
                    .label("새 Space 만들기")
                    .w_full()
                    .justify_start()
                    .on_click(cx.listener(|this, _, window, cx| this.new_space(window, cx))),
            )
    }
}
