//! Search presentation.
use super::*;

impl Edot {
    pub(in crate::desktop) fn search_fields(
        &self,
        global: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panel = if global { &self.global } else { &self.local };
        let query = Input::new(&panel.query)
            .small()
            .text_size(px(SEARCH_INPUT_FONT_SIZE))
            .aria_label(if global {
                "전체 메모 검색어"
            } else {
                "현재 메모 검색어"
            })
            .suffix(
                Button::new(if global { "global-case" } else { "local-case" })
                    .small()
                    .w(px(26.))
                    .px_0()
                    .label("Aa")
                    .tooltip("대소문자 구분")
                    .selected(panel.case)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if global {
                            this.global.case = !this.global.case;
                            this.queue_search(window, cx);
                        } else {
                            this.local.case = !this.local.case;
                            this.find_next(false, true, window, cx);
                        }
                    })),
            )
            .suffix(
                Button::new(if global {
                    "global-regex"
                } else {
                    "local-regex"
                })
                .small()
                .w(px(26.))
                .px_0()
                .label(".*")
                .tooltip("정규식")
                .selected(panel.regex)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if global {
                        this.global.regex = !this.global.regex;
                        this.queue_search(window, cx);
                    } else {
                        this.local.regex = !this.local.regex;
                        this.find_next(false, true, window, cx);
                    }
                })),
            );
        div()
            .v_flex()
            .gap(px(4.))
            .child(
                div()
                    .h_flex()
                    .when(global, |row| {
                        row.child(
                            tool(
                                "global-replace-toggle",
                                if panel.replace {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                },
                                "바꾸기 표시",
                            )
                            .w(px(18.))
                            .flex_shrink_0()
                            .px_0()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.global.replace = !this.global.replace;
                                cx.notify();
                            })),
                        )
                    })
                    .child(query),
            )
            .when(panel.replace, |view| {
                view.child(
                    div().when(global, |d| d.pl(px(18.))).child(
                        Input::new(&panel.replacement)
                            .small()
                            .text_size(px(SEARCH_INPUT_FONT_SIZE))
                            .aria_label(if global {
                                "전체 메모 바꿀 내용"
                            } else {
                                "바꿀 내용"
                            })
                            .when(global, |input| {
                                input.suffix(
                                    Button::new("replace-global-all")
                                        .small()
                                        .w(px(26.))
                                        .px_0()
                                        .icon(IconName::ArrowRight)
                                        .tooltip("모두 바꾸기")
                                        .on_click(cx.listener(|this, _, w, cx| {
                                            this.replace_global_all(w, cx)
                                        })),
                                )
                            }),
                    ),
                )
            })
            .when(panel.replace && !global, |view| {
                view.child(
                    div()
                        .h_flex()
                        .justify_end()
                        .gap_2()
                        .child(Button::new("replace-one").small().label("바꾸기").on_click(
                            cx.listener(|this, _, w, cx| this.replace_local(false, w, cx)),
                        ))
                        .child(
                            Button::new("replace-local-all")
                                .small()
                                .label("모두 바꾸기")
                                .on_click(
                                    cx.listener(|this, _, w, cx| this.replace_local(true, w, cx)),
                                ),
                        ),
                )
            })
            .child(
                div()
                    .debug_selector(move || {
                        if global {
                            "global-message"
                        } else {
                            "local-message"
                        }
                        .into()
                    })
                    .px_1()
                    .py_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(panel.message.clone()),
            )
            .into_any_element()
    }
    pub(in crate::desktop) fn local_view(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .font_family(component::theme::UI_FONT)
            .text_size(px(component::theme::UI_FONT_SIZE))
            .occlude()
            .absolute()
            .right_3()
            .top_2()
            .w(px(440.))
            .max_w_full()
            .p_3()
            .v_flex()
            .gap_2()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .shadow_lg()
            .child(
                div()
                    .h_flex()
                    .gap_1()
                    .child(
                        Button::new("local-replace-toggle")
                            .small()
                            .icon(if self.local.replace {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.local.replace = !this.local.replace;
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1().text_sm().child(if self.local.replace {
                        "찾기 및 바꾸기"
                    } else {
                        "찾기"
                    }))
                    .child(
                        tool("local-prev", IconName::ChevronUp, "이전 · Shift+F3").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.find_next(true, false, window, cx)
                            }),
                        ),
                    )
                    .child(
                        tool("local-next", IconName::ChevronDown, "다음 · F3").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.find_next(false, false, window, cx)
                            }),
                        ),
                    )
                    .child(tool("local-close", IconName::Close, "닫기 · Esc").on_click(
                        cx.listener(|this, _, window, cx| {
                            this.find_visible = false;
                            this.focus_editor(window, cx);
                            cx.notify();
                        }),
                    )),
            )
            .child(self.search_fields(false, cx))
            .into_any_element()
    }
    pub(in crate::desktop) fn global_view(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let list = uniform_list(
            "global-results",
            self.search_results.visible.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| this.result_row(index, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.search_results.scroll)
        .size_full();
        div()
            .v_flex()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .v_flex()
                    .gap(px(2.))
                    .p_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .h_flex()
                            .gap_0()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_sm()
                                    .child("검색"),
                            )
                            .child(
                                tool("global-prev", IconName::ChevronUp, "이전 결과").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.global_next(true, window, cx)
                                    }),
                                ),
                            )
                            .child(
                                tool("global-next", IconName::ChevronDown, "다음 결과").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.global_next(false, window, cx)
                                    }),
                                ),
                            )
                            .child(
                                tool("global-refresh", IconName::RefreshCw, "새로 고침").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.queue_search(window, cx)
                                    }),
                                ),
                            )
                            .child(
                                tool(
                                    "global-collapse",
                                    IconName::ChevronsUpDown,
                                    "모두 접기 / 펼치기",
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        if this.search_results.collapsed.len()
                                            == this.search_results.groups.len()
                                        {
                                            this.search_results.collapsed.clear();
                                        } else {
                                            this.search_results.collapsed = this
                                                .search_results
                                                .groups
                                                .iter()
                                                .map(|r| r.id)
                                                .collect();
                                        }
                                        this.rebuild_results();
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .child(self.search_fields(true, cx)),
            )
            .child(
                div()
                    .id("result-host")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .track_focus(&self.results_focus)
                    .when(
                        self.search_results.visible.is_empty() && !self.search_results.scanning,
                        |host| {
                            host.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .p_3()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if self.global.query.read(cx).value().is_empty() {
                                        "메모 본문에서 검색합니다"
                                    } else {
                                        "일치하는 텍스트가 없습니다"
                                    }),
                            )
                        },
                    )
                    .child(list)
                    .child(Scrollbar::vertical(&self.search_results.scroll)),
            )
            .into_any_element()
    }
    pub(in crate::desktop) fn result_row(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = self.search_results.visible[index];
        let group = &self.search_results.groups[row.group];
        let id = group.id;
        let selected = self.search_results.cursor == Some(index);
        let mut item = div()
            .id(SharedString::from(format!(
                "result-{id}-{:?}",
                row.hit.map(|hit| group.matches[hit].range)
            )))
            .debug_selector(move || format!("result-{index}"))
            .h_flex()
            .h(px(SEARCH_ROW_HEIGHT))
            .w_full()
            .px_2()
            .gap_1()
            .text_xs()
            .cursor_pointer()
            .when(selected, |row| row.bg(cx.theme().selection))
            .when(!selected, |row| row.hover(|s| s.bg(cx.theme().accent)))
            .on_click(cx.listener({
                let identity = (id, row.hit.map(|hit| group.matches[hit].range));
                move |this, _, window, cx| {
                    if let Some(index) = (0..this.search_results.visible.len())
                        .find(|&i| this.result_identity(i) == Some(identity))
                    {
                        this.activate_result(index, window, cx);
                    }
                }
            }));
        if let Some(hit) = row.hit {
            let result = &group.matches[hit];
            let range = result.range;
            let line = result.line;
            let column = result.column;
            let key = MatchKey { id, range };
            let h = &result.highlight;
            let preview = result.preview.to_string();
            item = item
                .child(
                    div()
                        .w(px(29.))
                        .flex_shrink_0()
                        .text_right()
                        .text_color(cx.theme().muted_foreground)
                        .child(result.line.to_string()),
                )
                .child(
                    div()
                        .h_flex()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(preview[..h.start].to_owned())
                        .child(
                            div()
                                .bg(cx.theme().selection)
                                .text_color(cx.theme().primary)
                                .child(if h.is_empty() {
                                    "│".to_owned()
                                } else {
                                    preview[h.clone()].to_owned()
                                }),
                        )
                        .child(preview[h.end..].to_owned()),
                )
                .tooltip(move |window, cx| {
                    Tooltip::new(format!("{line}:{column}  {preview}")).build(window, cx)
                });
            if self.global.replace {
                item = item.child(
                    Button::new(("replace-hit", index))
                        .small()
                        .icon(IconName::ArrowRight)
                        .tooltip("이 결과 바꾸기")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.replace_result(key, window, cx);
                            cx.stop_propagation();
                        })),
                );
            }
        } else {
            let title = self
                .book
                .get(id)
                .map_or("메모", |n| n.title.as_str())
                .to_owned();
            item = item
                .child(
                    Icon::new(if self.search_results.collapsed.contains(&id) {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    })
                    .size(px(13.)),
                )
                .child(Icon::new(IconName::FileText).size(px(14.)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .overflow_hidden()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(group.count.to_string()),
                );
        }
        item.into_any_element()
    }
}
