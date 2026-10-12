//! Editor presentation.
use super::*;

impl Edot {
    pub(in crate::desktop) fn body_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = self.active_editor() else {
            return div()
                .size_full()
                .v_flex()
                .justify_center()
                .items_center()
                .gap_3()
                .child(
                    Icon::new(IconName::FileText)
                        .size(px(40.))
                        .text_color(cx.theme().primary),
                )
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("생각을 가볍게 기록하세요"),
                )
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("노트를 선택하거나 Ctrl + N으로 새 노트를 만드세요"),
                )
                .into_any_element();
        };
        let font_size = cx.global::<AppState>().settings.font_size as f32 * self.zoom as f32
            / ZOOM_DEFAULT as f32;
        let editor_menu = editor.clone();
        let weak = cx.entity().downgrade();
        let wheel_view = weak.clone();
        div()
            .id("body-editor")
            .debug_selector(|| "body-editor".into())
            .relative()
            .size_full()
            .min_h_0()
            .font_family(cx.global::<AppState>().settings.font.clone())
            .text_size(px(font_size))
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |bounds, _, window, _| {
                        let weak = wheel_view.clone();
                        window.on_mouse_event(
                            move |event: &ScrollWheelEvent, phase, window, cx| {
                                if phase != DispatchPhase::Capture
                                    || !event.modifiers.control
                                    || !bounds.contains(&event.position)
                                    || window.has_active_dialog(cx)
                                {
                                    return;
                                }
                                let _ = weak.update(cx, |this, cx| {
                                    let panel_width = if this.settings_visible {
                                        372.
                                    } else if this.find_visible {
                                        452.
                                    } else {
                                        0.
                                    };
                                    if this.error.is_some()
                                        || panel_width > 0.
                                            && event.position.x > bounds.right() - px(panel_width)
                                            && event.position.y < bounds.top() + px(340.)
                                    {
                                        return;
                                    }
                                    let delta = event.delta.pixel_delta(px(20.)).y;
                                    if delta > px(0.) {
                                        this.zoom = (this.zoom + ZOOM_STEP).min(ZOOM_MAX);
                                    } else if delta < px(0.) {
                                        this.zoom =
                                            this.zoom.saturating_sub(ZOOM_STEP).max(ZOOM_MIN);
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                });
                            },
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                // The editor owns its content padding. Its viewport and clip
                // cover the whole body, including the initial blank margins.
                Textarea::new(&editor)
                    .h(relative(1.))
                    .appearance(false)
                    .font_family(cx.global::<AppState>().settings.font.clone())
                    .text_size(px(font_size))
                    .p_0()
                    .aria_label("노트 본문"),
            )
            .context_menu(move |menu, _, cx| {
                menus::editor_menu(menu, &editor_menu, weak.clone(), cx)
            })
            .into_any_element()
    }
    pub(in crate::desktop) fn status_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut position = String::new();
        if let Some(editor) = self.active_editor() {
            let state = editor.read(cx);
            let tab = self
                .open
                .iter()
                .find(|tab| Some(tab.id) == self.session.active)
                .unwrap();
            let selected: i32 = state
                .selected_ranges()
                .map(|range| editing::paragraph_units_iter(state.value()[range].chars()))
                .sum();
            // Input events are deferred; rendering can precede the cached metrics update.
            let current_metrics;
            let metrics = if tab.metrics_revision == state.revision {
                &tab.metrics
            } else {
                current_metrics = editing::TextMetrics::new(&state.value());
                &current_metrics
            };
            let (line, start) = metrics.line(state.cursor());
            let column =
                editing::paragraph_units_iter(state.value()[start..state.cursor()].chars()) + 1;
            position = format!(
                "Ln {line}, Col {column}   {}{}자",
                if selected > 0 {
                    format!("{selected}/")
                } else {
                    String::new()
                },
                metrics.total
            );
            if state.is_read_only() {
                position = format!("읽기 전용   {}자", metrics.total);
            }
        }
        div()
            .h_flex()
            .h(px(32.))
            .px_3()
            .gap_3()
            .flex_shrink_0()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().bar)
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(
                        self.tree_drag_description()
                            .unwrap_or_else(|| self.status.clone()),
                    ),
            )
            .child(position)
            .child(
                Button::new("reset-zoom")
                    .small()
                    .label(format!("{}%", self.zoom))
                    .tooltip("확대율 초기화 · Ctrl+0")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.zoom = ZOOM_DEFAULT;
                        cx.notify();
                    })),
            )
            .child(self.text_mode_view(cx))
            .child(self.reading_mode_view(cx))
            .into_any_element()
    }

    fn text_mode_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = self.active_editor() else {
            return div().child(TextMode::default().label()).into_any_element();
        };
        let mode = editor.read(cx).text_mode();
        let bounds = Rc::new(std::cell::Cell::new(Bounds::<Pixels>::default()));
        let trigger = bounds.clone();
        div()
            .id("text-mode-control")
            .debug_selector(|| "text-mode-control".into())
            .relative()
            .flex_shrink_0()
            .child(
                canvas(move |rect, _, _| bounds.set(rect), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .child(
                Button::new("select-text-mode")
                    .small()
                    .label(mode.label())
                    .icon(IconName::ChevronDown)
                    .tooltip("텍스트 모드 선택")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show_text_mode_menu(trigger.get().top_right(), window, cx);
                    })),
            )
            .into_any_element()
    }

    fn reading_mode_view(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.active_editor().is_none() {
            return div().into_any_element();
        }
        let read_only = cx.global::<AppState>().settings.read_only;
        div()
            .id("reading-mode-control")
            .debug_selector(|| "reading-mode-control".into())
            .flex_shrink_0()
            .child(
                Button::new("toggle-reading-mode")
                    .small()
                    .icon(if read_only {
                        IconName::Pencil
                    } else {
                        IconName::BookOpen
                    })
                    .tooltip(if read_only {
                        "편집 모드로 전환 · Ctrl+Shift+P"
                    } else {
                        "읽기 모드로 전환 · Ctrl+Shift+P"
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.set_read_only(
                            !cx.global::<AppState>().settings.read_only,
                            window,
                            cx,
                        ) {
                            this.focus_editor(window, cx);
                        }
                    })),
            )
            .into_any_element()
    }
}
