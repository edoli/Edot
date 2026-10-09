//! Shortcut reference, grouped by the context that handles each key.
use super::*;

struct ShortcutSection {
    title: &'static str,
    rows: &'static [(&'static str, &'static str)],
}

const SECTIONS: &[ShortcutSection] = &[
    ShortcutSection {
        title: "앱 · 노트 · 탭",
        rows: &[
            ("F1", "단축키 안내 열기 / 닫기"),
            ("Ctrl+N", "새 노트 만들기"),
            ("Ctrl+S", "수정한 노트 저장"),
            ("Ctrl+W / Ctrl+F4", "현재 탭 닫기"),
            ("Ctrl+Shift+T", "최근 닫은 탭 다시 열기"),
            ("Ctrl+Tab / Ctrl+PageDown", "다음 탭으로 이동"),
            ("Ctrl+Shift+Tab / Ctrl+PageUp", "이전 탭으로 이동"),
            ("Ctrl+1~8 / Ctrl+9", "해당 순번 탭 / 마지막 탭으로 이동"),
            ("Ctrl+B", "사이드바 표시 / 숨기기"),
            ("Ctrl+,", "설정 열기 / 닫기"),
            ("Alt+Z", "자동 줄 바꿈 켜기 / 끄기"),
            ("F2", "노트 이름 바꾸기"),
            ("Shift+F10 / Menu 키", "현재 대상의 메뉴 열기"),
            ("Esc", "드래그·이름 편집 취소 또는 열린 패널·팝업 닫기"),
            (
                "Tab / Shift+Tab",
                "다음 / 이전 컨트롤로 이동 (본문에서는 탭 문자 입력)",
            ),
        ],
    },
    ShortcutSection {
        title: "본문 · 입력창 편집",
        rows: &[
            ("Ctrl+Z", "실행 취소"),
            ("Ctrl+Y / Ctrl+Shift+Z", "다시 실행"),
            (
                "Ctrl+X / Ctrl+C",
                "잘라내기 / 복사. 본문에서 선택이 없으면 현재 행 전체",
            ),
            ("Ctrl+V", "텍스트 붙여넣기"),
            ("Ctrl+A", "입력 내용 전체 선택"),
            (
                "Ctrl+D",
                "본문의 단어 선택 / 다음 일치 항목 추가 선택·동시 편집",
            ),
            ("Esc", "본문의 추가 선택·커서를 해제하고 첫 선택 유지"),
            (
                "Alt+Shift+드래그",
                "본문 사각형(세로) 선택 / 같은 열에 여러 커서 배치",
            ),
            ("← / →", "이전 / 다음 문자로 이동"),
            ("Ctrl+← / Ctrl+→", "이전 / 다음 단어로 이동"),
            ("↑ / ↓", "본문에서 위 / 아래 표시 줄로 이동"),
            ("Home / End", "현재 표시 줄의 처음 / 끝으로 이동"),
            ("Ctrl+Home / Ctrl+End", "문서 처음 / 끝으로 이동"),
            ("PageUp / PageDown", "본문에서 한 화면 위 / 아래로 이동"),
            ("Shift+이동 키", "커서 이동 범위를 선택 (Ctrl 조합 포함)"),
            ("Backspace / Delete", "선택 영역 또는 앞 / 뒤 문자 삭제"),
            (
                "Ctrl+Backspace / Ctrl+Delete",
                "선택 영역 또는 앞 / 뒤 단어 삭제",
            ),
            ("Enter / Tab", "본문에 줄 바꿈 / 탭 문자 입력"),
            ("F5 / F6", "시간+날짜 / 날짜 삽입 (노트·본문·이름 편집에서)"),
            ("Ctrl++ / Ctrl+= / Ctrl+숫자패드 +", "본문 확대"),
            ("Ctrl+- / Ctrl+숫자패드 -", "본문 축소"),
            ("Ctrl+0", "본문 확대율 100%로 초기화"),
            ("Ctrl+휠", "본문 확대 / 축소"),
        ],
    },
    ShortcutSection {
        title: "검색 · 바꾸기",
        rows: &[
            ("Ctrl+F / Ctrl+H", "현재 노트 찾기 / 바꾸기"),
            ("Ctrl+Shift+F / Ctrl+Shift+H", "전체 노트 찾기 / 바꾸기"),
            ("F3 / Shift+F3", "다음 / 이전 검색 결과"),
            ("Enter / Shift+Enter", "검색어 입력창에서 다음 / 이전 결과"),
            ("Enter", "현재 노트의 바꿀 내용 입력창에서 한 곳 바꾸기"),
            (
                "Enter / Shift+Enter",
                "전체 검색의 바꿀 내용 입력창에서 다음 / 이전 결과",
            ),
            ("↑ / ↓ / Home / End", "전체 검색 결과 목록에서 항목 이동"),
            ("← / →", "전체 검색 결과 그룹 접기 / 펼치기"),
            (
                "Enter / Space",
                "선택한 전체 검색 결과 열기 또는 그룹 접기 / 펼치기",
            ),
            (
                "Esc",
                "현재 찾기 닫기. 전체 검색에서는 본문으로 포커스 이동",
            ),
        ],
    },
    ShortcutSection {
        title: "노트 트리 · 이름 편집",
        rows: &[
            ("↑ / ↓ / Home / End", "이전·다음·처음·마지막 노트로 이동"),
            ("← / →", "접기·부모로 이동 / 펼치기·첫 하위 노트로 이동"),
            ("Ctrl+이동 키", "기존 선택을 유지하며 트리 커서만 이동"),
            ("Shift+이동 키", "기준 노트부터 이동한 노트까지 범위 선택"),
            ("Ctrl+Shift+이동 키", "기존 선택에 이동 범위 추가"),
            ("Ctrl+A", "트리에 보이는 노트 전체 선택"),
            ("Ctrl+Space", "현재 트리 노트 선택 / 해제"),
            (
                "Ctrl+클릭 / Shift+클릭",
                "노트 선택 전환 / 기준 노트부터 범위 선택",
            ),
            ("Ctrl+Shift+클릭", "기존 노트 선택에 클릭한 범위 추가"),
            ("Enter", "노트를 열고 본문에 포커스"),
            ("Delete", "선택한 노트 삭제 확인"),
            ("Enter / Tab / Esc", "이름 편집 중 확정 / 확정 / 취소"),
        ],
    },
    ShortcutSection {
        title: "설정 · 메뉴 · 확인창",
        rows: &[
            ("Enter / Space", "포커스한 버튼 실행 또는 스위치 전환"),
            ("Enter / Space / ↑ / ↓", "포커스한 드롭다운 열기"),
            ("↑ / ↓", "드롭다운·메뉴에서 이전 / 다음 항목"),
            ("PageUp / PageDown", "드롭다운에서 7개 항목씩 이동"),
            ("Home / End", "드롭다운 처음 / 끝 (글꼴 검색 입력창 제외)"),
            (
                "Enter / Space",
                "드롭다운·메뉴 항목 선택 (검색 입력창의 Space는 공백 입력)",
            ),
            (
                "Tab / Shift+Tab",
                "드롭다운을 닫고 다음 / 이전 컨트롤로 이동",
            ),
            ("Tab / Shift+Tab", "확인창의 버튼·다시 묻지 않기 선택 전환"),
            ("← / →", "확인창에서 취소 / 확인 버튼 선택 전환"),
            (
                "Enter / Space",
                "확인창에서 선택한 버튼 실행 또는 체크 전환 (삭제는 확인이 기본)",
            ),
            ("Esc", "설정·드롭다운·메뉴 닫기 또는 확인창 취소"),
            ("Enter / Esc", "오류 안내 닫기"),
        ],
    },
    ShortcutSection {
        title: "단축키 안내",
        rows: &[
            ("↑ / ↓ / 휠", "표 위 / 아래로 스크롤"),
            ("PageUp / PageDown", "표 한 화면 위 / 아래로 스크롤"),
            ("Home / End", "표 처음 / 끝으로 이동"),
            ("F1 / Esc / Enter", "단축키 안내 닫기"),
        ],
    },
];

pub(super) struct ShortcutHelp {
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    owner: WeakEntity<Edot>,
    pub(super) scroll: ScrollHandle,
}

impl Edot {
    pub(super) fn show_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let temporary_focus = self.settings_visible || self.rename.is_some();
        if !self.finish_rename(true, false, window, cx) {
            return;
        }
        let restore = if temporary_focus {
            Some(
                self.active_editor()
                    .map_or_else(|| self.focus.clone(), |e| e.read(cx).focus_handle(cx)),
            )
        } else {
            window.focused(cx)
        };
        self.dismiss_settings(cx);
        cx.stop_active_drag(window);
        self.stop_drag();
        self.resizing = None;
        let focus = cx.focus_handle();
        let owner = cx.entity().downgrade();
        self.shortcut_help = Some(cx.new(|_| ShortcutHelp {
            focus: focus.clone(),
            restore,
            owner,
            scroll: ScrollHandle::new(),
        }));
        focus.focus(window, cx);
        cx.notify();
    }
}

impl ShortcutHelp {
    fn close(&self, window: &mut Window, cx: &mut Context<Self>) {
        let restore = self.restore.clone();
        let _ = self.owner.update(cx, |this, cx| {
            this.shortcut_help = None;
            if let Some(focus) = restore {
                focus.focus(window, cx);
            } else {
                this.focus_editor(window, cx);
            }
            cx.notify();
        });
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        match key {
            "escape" | "enter" => self.close(window, cx),
            "f1" if !event.is_held => self.close(window, cx),
            "up" | "down" | "pageup" | "pagedown" | "home" | "end" => {
                let current = self.scroll.offset().y;
                let page = self.scroll.bounds().size.height;
                let max = self.scroll.max_offset().y;
                let next = match key {
                    "up" => current + px(36.),
                    "down" => current - px(36.),
                    "pageup" => current + page,
                    "pagedown" => current - page,
                    "home" => px(0.),
                    _ => -max,
                };
                self.scroll
                    .set_offset(point(px(0.), next.clamp(-max, px(0.))));
                cx.notify();
            }
            _ => {}
        }
        cx.stop_propagation();
    }
}

fn table_row(
    id: usize,
    keys: &'static str,
    description: &'static str,
    header: bool,
    cx: &App,
) -> impl IntoElement {
    div()
        .id(("shortcut-row", id))
        .debug_selector(move || {
            if header {
                "shortcut-table-header".into()
            } else {
                format!("shortcut-row-{id}")
            }
        })
        .flex()
        .items_start()
        .flex_shrink_0()
        .w_full()
        .role(Role::Row)
        .border_b_1()
        .border_color(cx.theme().border)
        .when(header, |row| {
            row.bg(cx.theme().bar).font_weight(FontWeight::SEMIBOLD)
        })
        .child(
            div()
                .id("shortcut-key")
                .w(relative(0.44))
                .flex_shrink_0()
                .min_w_0()
                .px_3()
                .py_2()
                .role(if header {
                    Role::ColumnHeader
                } else {
                    Role::Cell
                })
                .child(keys),
        )
        .child(
            div()
                .id("shortcut-description")
                .flex_1()
                .min_w_0()
                .px_3()
                .py_2()
                .role(if header {
                    Role::ColumnHeader
                } else {
                    Role::Cell
                })
                .child(description),
        )
}

impl Render for ShortcutHelp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = div()
            .id("shortcut-rows")
            .debug_selector(|| "shortcut-rows".into())
            .v_flex()
            .size_full()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll);
        let mut row_id = 1;
        for section in SECTIONS {
            rows = rows.child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_2()
                    .mt_2()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().primary)
                    .child(section.title),
            );
            for &(keys, description) in section.rows {
                rows = rows.child(table_row(row_id, keys, description, false, cx));
                row_id += 1;
            }
        }
        div()
            .id("shortcut-overlay")
            .debug_selector(|| "shortcut-overlay".into())
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(black().opacity(0.35))
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::key))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("shortcut-help")
                    .debug_selector(|| "shortcut-help".into())
                    .role(Role::Dialog)
                    .aria_label("단축키 안내")
                    .v_flex()
                    .w(px(
                        (f32::from(window.viewport_size().width) - 32.).clamp(0., 760.)
                    ))
                    .h(px(
                        (f32::from(window.viewport_size().height) - 32.).clamp(0., 640.)
                    ))
                    .p_4()
                    .gap_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .text_color(cx.theme().foreground)
                    .font_family(component::theme::UI_FONT)
                    .text_size(px(14.))
                    .shadow_lg()
                    .child(
                        div()
                            .h_flex()
                            .flex_shrink_0()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("단축키 안내"),
                            )
                            .child(
                                div().debug_selector(|| "shortcut-close".into()).child(
                                    Button::new("shortcut-close-button")
                                        .label("닫기")
                                        .tab_stop(false)
                                        .on_click(cx.listener(|this, _, w, cx| this.close(w, cx))),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .id("shortcut-table")
                            .v_flex()
                            .role(Role::Table)
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(table_row(0, "단축키", "동작", true, cx))
                            .child(
                                div()
                                    .relative()
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .child(rows)
                                    .child(component::scroll::Scrollbar::vertical_handle(
                                        &self.scroll,
                                    )),
                            ),
                    ),
            )
    }
}
