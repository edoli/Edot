//! Shortcut reference, grouped by the context that handles each key.
use super::*;
use crate::shortcuts::{ActiveShortcuts, Command};
use Command::*;
use ShortcutKeys::{Commands as C, Fixed as F};
enum ShortcutKeys {
    Fixed(&'static str),
    Commands(&'static [Command]),
}
impl ShortcutKeys {
    fn label(&self, cx: &App) -> String {
        match self {
            Self::Fixed(text) => component::shortcuts::label(*text).to_string(),
            Self::Commands(commands) => commands
                .iter()
                .map(|&command| {
                    let label = crate::shortcuts::label(command, cx);
                    if label.is_empty() {
                        format!("{}: 미지정", command.label())
                    } else {
                        label
                    }
                })
                .collect::<Vec<_>>()
                .join(" / "),
        }
    }
}

struct ShortcutSection {
    title: &'static str,
    rows: &'static [(ShortcutKeys, &'static str)],
}

const SECTIONS: &[ShortcutSection] = &[
    ShortcutSection {
        title: "앱 · 노트 · 탭",
        rows: &[
            (C(&[Help]), "단축키 열기 / 닫기"),
            (C(&[Fullscreen]), "전체화면 켜기 / 끄기"),
            #[cfg(target_os = "macos")]
            (F("Cmd+Q / Cmd+Shift+W"), "저장 후 앱 종료 / 창 닫기"),
            (C(&[NewNote]), "새 노트 만들기"),
            (C(&[NewFolder]), "새 폴더 만들기"),
            (C(&[Save]), "수정한 노트 저장"),
            (C(&[CloseTab]), "현재 탭 닫기"),
            (C(&[ReopenTab]), "최근 닫은 탭 다시 열기"),
            (C(&[NextTab]), "다음 탭으로 이동"),
            (C(&[PreviousTab]), "이전 탭으로 이동"),
            (
                C(&[Tab1, Tab2, Tab3, Tab4, Tab5, Tab6, Tab7, Tab8, LastTab]),
                "해당 순번 탭 / 마지막 탭으로 이동",
            ),
            (C(&[Sidebar]), "사이드바 표시 / 숨기기"),
            (C(&[ReadOnly]), "읽기 / 편집 모드 전환"),
            (C(&[Settings]), "설정 열기 / 닫기"),
            (C(&[Wrap]), "자동 줄 바꿈 켜기 / 끄기"),
            (C(&[Rename]), "항목 이름 바꾸기"),
            (F("Shift+F10 / Menu 키"), "현재 대상의 메뉴 열기"),
            (F("Esc"), "드래그·이름 편집 취소 또는 열린 패널·팝업 닫기"),
            (
                F("Tab / Shift+Tab"),
                "다음 / 이전 컨트롤로 이동 (본문에서는 탭 문자 입력)",
            ),
        ],
    },
    ShortcutSection {
        title: "본문 · 입력창 편집",
        rows: &[
            (C(&[Undo]), "실행 취소"),
            (C(&[Redo]), "다시 실행"),
            (
                C(&[Cut, Copy]),
                "잘라내기 / 복사. 본문에서 선택이 없으면 현재 행 전체",
            ),
            (C(&[Paste]), "텍스트 붙여넣기"),
            (C(&[SelectAll]), "입력 내용 전체 선택"),
            (C(&[Bold, Italic]), "Markdown 본문: 굵게 / 기울임 적용·해제"),
            (
                C(&[Strike, Code]),
                "Markdown 본문: 취소선 / 인라인 코드 적용·해제",
            ),
            (
                C(&[NextOccurrence]),
                "본문의 단어 선택 / 다음 일치 항목 추가 선택·동시 편집",
            ),
            (F("Esc"), "본문의 추가 선택·커서를 해제하고 첫 선택 유지"),
            (
                F("Alt+Shift+드래그"),
                "본문 사각형(세로) 선택 / 같은 열에 여러 커서 배치",
            ),
            (F("← / →"), "이전 / 다음 문자로 이동"),
            (F("Ctrl+← / Ctrl+→"), "이전 / 다음 단어로 이동"),
            (F("↑ / ↓"), "본문에서 위 / 아래 표시 줄로 이동"),
            (F("Home / End"), "현재 표시 줄의 처음 / 끝으로 이동"),
            (F("Ctrl+Home / Ctrl+End"), "문서 처음 / 끝으로 이동"),
            #[cfg(target_os = "macos")]
            (
                F("Cmd+Backspace / Cmd+Delete"),
                "표시 줄의 처음 / 끝까지 삭제",
            ),
            (F("PageUp / PageDown"), "본문에서 한 화면 위 / 아래로 이동"),
            (F("Shift+이동 키"), "커서 이동 범위를 선택 (Ctrl 조합 포함)"),
            (F("Backspace / Delete"), "선택 영역 또는 앞 / 뒤 문자 삭제"),
            (
                F("Ctrl+Backspace / Ctrl+Delete"),
                "선택 영역 또는 앞 / 뒤 단어 삭제",
            ),
            (F("Enter / Tab"), "본문에 줄 바꿈 / 탭 문자 입력"),
            (
                C(&[DateTime, Date]),
                "시간+날짜 / 날짜 삽입 (노트·본문·이름 편집에서)",
            ),
            (C(&[ZoomIn]), "본문 확대"),
            (C(&[ZoomOut]), "본문 축소"),
            (C(&[ZoomReset]), "본문 확대율 100%로 초기화"),
            (F("Ctrl+휠"), "본문 확대 / 축소"),
            (F("Ctrl+클릭"), "본문의 웹 링크를 기본 브라우저에서 열기"),
        ],
    },
    ShortcutSection {
        title: "검색 · 바꾸기",
        rows: &[
            (C(&[Find, Replace]), "현재 노트 찾기 / 바꾸기"),
            (C(&[GlobalFind, GlobalReplace]), "전체 노트 찾기 / 바꾸기"),
            (C(&[NextMatch, PreviousMatch]), "다음 / 이전 검색 결과"),
            (
                F("Enter / Shift+Enter"),
                "검색어 입력창에서 다음 / 이전 결과",
            ),
            (F("Enter"), "현재 노트의 바꿀 내용 입력창에서 한 곳 바꾸기"),
            (
                F("Enter / Shift+Enter"),
                "전체 검색의 바꿀 내용 입력창에서 다음 / 이전 결과",
            ),
            (F("↑ / ↓ / Home / End"), "전체 검색 결과 목록에서 항목 이동"),
            (F("← / →"), "전체 검색 결과 그룹 접기 / 펼치기"),
            (
                F("Enter / Space"),
                "선택한 전체 검색 결과 열기 또는 그룹 접기 / 펼치기",
            ),
            (
                F("Esc"),
                "현재 찾기 닫기. 전체 검색에서는 본문으로 포커스 이동",
            ),
        ],
    },
    ShortcutSection {
        title: "노트 트리 · 이름 편집",
        rows: &[
            (F("↑ / ↓ / Home / End"), "이전·다음·처음·마지막 노트로 이동"),
            (F("← / →"), "접기·부모로 이동 / 펼치기·첫 하위 노트로 이동"),
            (F("Ctrl+이동 키"), "기존 선택을 유지하며 트리 커서만 이동"),
            (
                F("Shift+이동 키"),
                "기준 노트부터 이동한 노트까지 범위 선택",
            ),
            (F("Ctrl+Shift+이동 키"), "기존 선택에 이동 범위 추가"),
            (C(&[SelectAll]), "트리에 보이는 노트 전체 선택"),
            (F("Ctrl+Space"), "현재 트리 노트 선택 / 해제"),
            (
                F("Ctrl+클릭 / Shift+클릭"),
                "노트 선택 전환 / 기준 노트부터 범위 선택",
            ),
            (F("Ctrl+Shift+클릭"), "기존 노트 선택에 클릭한 범위 추가"),
            (F("Enter"), "노트를 열고 본문에 포커스"),
            (F("Delete"), "선택한 노트 삭제 확인"),
            (F("Enter / Tab / Esc"), "이름 편집 중 확정 / 확정 / 취소"),
        ],
    },
    ShortcutSection {
        title: "설정 · 메뉴 · 확인창",
        rows: &[
            (F("Enter / Space"), "포커스한 버튼 실행 또는 스위치 전환"),
            (F("Enter / Space / ↑ / ↓"), "포커스한 드롭다운 열기"),
            (F("↑ / ↓"), "드롭다운·메뉴에서 이전 / 다음 항목"),
            (F("PageUp / PageDown"), "드롭다운에서 7개 항목씩 이동"),
            (
                F("Home / End"),
                "드롭다운 처음 / 끝 (글꼴 검색 입력창 제외)",
            ),
            (
                F("Enter / Space"),
                "드롭다운·메뉴 항목 선택 (검색 입력창의 Space는 공백 입력)",
            ),
            (
                F("Tab / Shift+Tab"),
                "드롭다운을 닫고 다음 / 이전 컨트롤로 이동",
            ),
            (
                F("Tab / Shift+Tab"),
                "확인창의 버튼·다시 묻지 않기 선택 전환",
            ),
            (F("← / →"), "확인창에서 취소 / 확인 버튼 선택 전환"),
            (
                F("Enter / Space"),
                "확인창에서 선택한 버튼 실행 또는 체크 전환 (삭제는 확인이 기본)",
            ),
            (F("Esc"), "설정·드롭다운·메뉴 닫기 또는 확인창 취소"),
            (F("Enter / Esc"), "오류 안내 닫기"),
        ],
    },
    ShortcutSection {
        title: "단축키",
        rows: &[
            (F("Ctrl+F"), "단축키 검색창으로 이동·전체 선택"),
            (F("Tab / Shift+Tab"), "검색창·표·설정 버튼으로 포커스 이동"),
            (F("↑ / ↓ / 휠"), "표 위 / 아래로 스크롤"),
            (F("PageUp / PageDown"), "표 한 화면 위 / 아래로 스크롤"),
            (
                F("Home / End"),
                "표 처음 / 끝으로 이동 (검색창에서는 커서 이동)",
            ),
            (F("Esc"), "단축키 닫기"),
            (F("Enter"), "포커스한 버튼 실행 (표에서는 닫기)"),
        ],
    },
];

fn search_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) struct ShortcutHelp {
    focus: FocusHandle,
    table_focus: FocusHandle,
    restore: Option<FocusHandle>,
    owner: WeakEntity<Edot>,
    pub(super) scroll: ScrollHandle,
    key_scroll: HashMap<usize, ScrollHandle>,
    pub(super) search: Entity<InputState>,
    pub(super) recording: Option<Command>,
    pub(super) message: Option<String>,
    message_error: bool,
    _subscriptions: Vec<Subscription>,
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
        cx.global_mut::<ActiveShortcuts>().recording = false;
        let owner = cx.entity().downgrade();
        let help = cx.new(|cx| {
            let focus = cx.focus_handle();
            let search =
                cx.new(|cx| InputState::new(window, cx).placeholder("동작 또는 단축키 검색"));
            let subscriptions = vec![
                cx.subscribe(&search, |this: &mut ShortcutHelp, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    }
                }),
                cx.on_focus_out(&focus, window, |this, _, _, cx| this.cancel_recording(cx)),
                cx.observe_window_activation(window, |this, window, cx| {
                    if !window.is_window_active() {
                        this.cancel_recording(cx);
                    }
                }),
                cx.on_release(|_, cx| {
                    cx.global_mut::<ActiveShortcuts>().recording = false;
                }),
            ];
            ShortcutHelp {
                focus,
                table_focus: cx.focus_handle().tab_stop(true),
                restore,
                owner,
                scroll: ScrollHandle::new(),
                key_scroll: HashMap::new(),
                search,
                recording: None,
                message: None,
                message_error: false,
                _subscriptions: subscriptions,
            }
        });
        let search_focus = help.read(cx).search.read(cx).focus_handle(cx);
        self.shortcut_help = Some(help);
        search_focus.focus(window, cx);
        cx.notify();
    }

    fn save_shortcuts(
        &mut self,
        bindings: crate::shortcuts::Bindings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        let mut next = cx.global::<AppState>().settings.clone();
        next.shortcuts = bindings;
        next.save(&cx.global::<AppState>().config_dir)?;
        self.apply_settings(next, window, cx);
        Ok(())
    }
}

impl ShortcutHelp {
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_recording(cx);
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

    pub(super) fn begin_recording(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.recording = Some(command);
        self.message = None;
        self.table_focus.focus(window, cx);
        cx.global_mut::<ActiveShortcuts>().recording = true;
        cx.notify();
    }

    fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording.take().is_some() {
            cx.global_mut::<ActiveShortcuts>().recording = false;
            cx.notify();
        }
    }

    fn show_message(&mut self, message: String, error: bool, cx: &mut Context<Self>) {
        self.message = Some(message);
        self.message_error = error;
        cx.notify();
    }

    fn save(
        &mut self,
        bindings: crate::shortcuts::Bindings,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_recording(cx);
        match self
            .owner
            .update(cx, |this, cx| this.save_shortcuts(bindings, window, cx))
        {
            Ok(Ok(())) => self.show_message(message.into(), false, cx),
            Ok(Err(error)) => {
                self.show_message(format!("설정을 저장하지 못했습니다: {error}"), true, cx)
            }
            Err(_) => self.show_message("설정 화면을 사용할 수 없습니다.".into(), true, cx),
        }
    }

    pub(super) fn remove(
        &mut self,
        command: Command,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut bindings = cx.global::<AppState>().settings.shortcuts.clone();
        match bindings.remove(command, index) {
            Ok(()) => self.save(bindings, "단축키를 제거했습니다.", window, cx),
            Err(message) => self.show_message(message, true, cx),
        }
    }

    pub(super) fn reset(
        &mut self,
        command: Option<Command>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_recording(cx);
        let mut bindings = cx.global::<AppState>().settings.shortcuts.clone();
        if let Some(command) = command {
            bindings.0.remove(&command);
        } else {
            bindings.0.clear();
        }
        if let Err(message) = bindings.validate() {
            self.show_message(message, true, cx);
        } else {
            self.save(bindings, "기본 단축키로 복원했습니다.", window, cx);
        }
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if let Some(command) = self.recording {
            cx.stop_propagation();
            if event.is_held
                || event.prefer_character_input
                || matches!(key, "control" | "shift" | "alt" | "platform" | "function")
            {
                return;
            }
            if key == "escape" {
                self.cancel_recording(cx);
                return;
            }
            let mut bindings = cx.global::<AppState>().settings.shortcuts.clone();
            let clear = matches!(key, "backspace" | "delete")
                && event.keystroke.modifiers == Modifiers::default();
            if clear {
                self.cancel_recording(cx);
            } else {
                match bindings.edit(command, None, &event.keystroke) {
                    Ok(()) => self.save(bindings, "단축키를 저장했습니다.", window, cx),
                    Err(message) => self.show_message(message, true, cx),
                }
            }
            return;
        }
        let search_focus = self.search.read(cx).focus_handle(cx);
        let search_focused = search_focus.is_focused(window);
        let table_focused = self.table_focus.is_focused(window);
        if event.prefer_character_input || search_focused && self.search.read(cx).is_composing() {
            return;
        }
        let command = cx
            .global::<ActiveShortcuts>()
            .command(&event.keystroke, cfg!(target_os = "macos"));
        if command == Some(Help) {
            if !event.is_held {
                self.close(window, cx);
            }
            cx.stop_propagation();
            return;
        }
        match key {
            "escape" => self.close(window, cx),
            "enter" if table_focused => self.close(window, cx),
            "f" if component::shortcuts::primary(event.keystroke.modifiers)
                && !event.keystroke.modifiers.shift =>
            {
                self.search.update(cx, |search, cx| {
                    search.select_all(window, cx);
                    search.focus(window, cx);
                });
            }
            "tab"
                if !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.alt
                    && !event.keystroke.modifiers.platform =>
            {
                if event.keystroke.modifiers.shift {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
                if !self.focus.contains_focused(window, cx) {
                    search_focus.focus(window, cx);
                }
            }
            "up" | "down" | "pageup" | "pagedown" | "home" | "end"
                if table_focused || search_focused && !matches!(key, "home" | "end") =>
            {
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
            _ if !table_focused => return,
            _ => {}
        }
        cx.stop_propagation();
    }

    fn table_row(
        &mut self,
        id: usize,
        description: String,
        keys: String,
        command: Option<Command>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut row = table_row_frame(id, cx).child(description_cell().child(description));
        if let Some(command) = command {
            let keys = cx
                .global::<AppState>()
                .settings
                .shortcuts
                .keys(command, cfg!(target_os = "macos"));
            let scroll = self.key_scroll.entry(id).or_default().clone();
            let mut list = div()
                .id(("shortcut-key-list", id))
                .w_full()
                .overflow_x_scroll()
                .track_scroll(&scroll)
                .pb(px(10.))
                .debug_selector(move || format!("shortcut-record-{}", command.id()))
                .h_flex()
                .gap_1();
            for (index, key) in keys.iter().enumerate() {
                let label = crate::shortcuts::display_key(key);
                list = list.child(
                    div()
                        .flex_shrink_0()
                        .h_flex()
                        .h(px(27.))
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().bar)
                        .overflow_hidden()
                        .debug_selector(move || format!("shortcut-key-{}-{index}", command.id()))
                        .child(
                            div().flex_shrink_0().child(
                                div()
                                    .id(("shortcut-key-label", index))
                                    .h_flex()
                                    .h(px(25.))
                                    .px_2()
                                    .child(label.clone()),
                            ),
                        )
                        .child(
                            div()
                                .debug_selector(move || {
                                    format!("shortcut-remove-{}-{index}", command.id())
                                })
                                .child(
                                    Button::new(("shortcut-remove-key", index))
                                        .small()
                                        .icon(IconName::Close)
                                        .w(px(24.))
                                        .h(px(25.))
                                        .px_0()
                                        .rounded_none()
                                        .border_l_1()
                                        .border_color(cx.theme().border)
                                        .text_color(cx.theme().destructive)
                                        .tooltip(format!("{}: {label} 제거", command.label()))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.remove(command, index, window, cx)
                                        })),
                                ),
                        ),
                );
            }
            let adding = self.recording == Some(command);
            if keys.is_empty() && !adding {
                list = list.child(
                    div()
                        .py_1()
                        .text_color(cx.theme().muted_foreground)
                        .child("미지정"),
                );
            }
            let add = div()
                .flex_shrink_0()
                .debug_selector(move || format!("shortcut-add-{}", command.id()))
                .child(
                    Button::new(("shortcut-add", id))
                        .small()
                        .icon(IconName::Plus)
                        .border_1()
                        .border_color(cx.theme().border)
                        .selected(adding)
                        .label(if adding {
                            "키 조합 입력…"
                        } else {
                            "추가"
                        })
                        .tooltip(format!("{} 단축키 추가", command.label()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.begin_recording(command, window, cx)
                        })),
                );
            row = row
                .child(
                    key_cell()
                        .h_flex()
                        .items_start()
                        .gap_1()
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .child(list)
                                .child(component::scroll::Scrollbar::horizontal(&scroll)),
                        )
                        .child(add),
                )
                .child(
                    reset_cell().child(
                        div()
                            .debug_selector(move || format!("shortcut-reset-{}", command.id()))
                            .child(
                                Button::new(("shortcut-reset", id))
                                    .small()
                                    .label("기본값")
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.reset(Some(command), window, cx)
                                    })),
                            ),
                    ),
                );
        } else {
            row = row.child(key_cell().py_2().child(keys)).child(
                reset_cell()
                    .py_2()
                    .text_color(cx.theme().muted_foreground)
                    .child("—"),
            );
        }
        row
    }
}

fn table_row_frame(id: usize, cx: &App) -> Stateful<Div> {
    div()
        .id(("shortcut-row", id))
        .debug_selector(move || {
            if id == 0 {
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
}
fn description_cell() -> Stateful<Div> {
    div()
        .id("shortcut-description")
        .flex_1()
        .min_w_0()
        .px_3()
        .py_2()
        .role(Role::Cell)
}
fn key_cell() -> Stateful<Div> {
    div()
        .id("shortcut-key")
        .w(relative(0.46))
        .flex_shrink_0()
        .min_w_0()
        .px_2()
        .py_1()
        .role(Role::Cell)
}
fn reset_cell() -> Stateful<Div> {
    div()
        .id("shortcut-reset-cell")
        .w(px(68.))
        .flex_shrink_0()
        .px_1()
        .py_1()
        .role(Role::Cell)
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
        let query = search_text(&self.search.read(cx).value());
        let mut any_matches = false;
        for section in SECTIONS {
            let section_matches = search_text(section.title).contains(&query);
            let mut section_rows = Vec::new();
            for (index, (keys, description)) in section.rows.iter().enumerate() {
                match keys {
                    ShortcutKeys::Commands(commands) => {
                        for (i, &command) in commands.iter().enumerate() {
                            let label = crate::shortcuts::label(command, cx);
                            let description = if commands.len() == 1 {
                                description.to_string()
                            } else {
                                match command {
                                    Cut | Copy => format!(
                                        "{} · 본문에서 선택이 없으면 현재 행 전체",
                                        command.label()
                                    ),
                                    _ => command.label().to_string(),
                                }
                            };
                            if section_matches
                                || search_text(&label).contains(&query)
                                || search_text(&description).contains(&query)
                            {
                                section_rows.push((
                                    if i == 0 {
                                        row_id + index
                                    } else {
                                        1000 + command as usize
                                    },
                                    description,
                                    label,
                                    Some(command),
                                ));
                            }
                        }
                    }
                    ShortcutKeys::Fixed(_) => {
                        let label = keys.label(cx);
                        if section_matches
                            || search_text(&label).contains(&query)
                            || search_text(description).contains(&query)
                        {
                            section_rows.push((
                                row_id + index,
                                description.to_string(),
                                label,
                                None,
                            ));
                        }
                    }
                }
            }
            row_id += section.rows.len();
            if section_rows.is_empty() {
                continue;
            }
            any_matches = true;
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
            for (id, description, keys, command) in section_rows {
                rows = rows.child(self.table_row(id, description, keys, command, cx));
            }
        }
        if !any_matches {
            rows = rows.child(
                div()
                    .debug_selector(|| "shortcut-empty".into())
                    .p_3()
                    .text_color(cx.theme().muted_foreground)
                    .child("검색 결과가 없습니다."),
            );
        }
        let status = self.message.clone().unwrap_or_else(|| {
            if let Some(command) = self.recording {
                format!(
                    "{}: 새 키 조합을 누르세요. Esc/Backspace/Delete: 추가 취소",
                    command.label()
                )
            } else {
                "X로 제거하거나 추가 버튼으로 등록하세요. 변경 즉시 저장됩니다.".into()
            }
        });
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
                    .aria_label("단축키")
                    .v_flex()
                    .w(px(
                        (f32::from(window.viewport_size().width) - 32.).clamp(0., 1080.)
                    ))
                    .h(px(
                        (f32::from(window.viewport_size().height) - 32.).clamp(0., 680.)
                    ))
                    .p_4()
                    .gap_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .text_color(cx.theme().foreground)
                    .font_family(component::theme::UI_FONT)
                    .shadow_lg()
                    .child(
                        div()
                            .h_flex()
                            .flex_shrink_0()
                            .gap_3()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("단축키"),
                            )
                            .child(
                                div()
                                    .debug_selector(|| "shortcut-search".into())
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&self.search).aria_label("단축키 검색")),
                            )
                            .child(
                                div()
                                    .debug_selector(|| "shortcut-close".into())
                                    .flex_shrink_0()
                                    .child(
                                        Button::new("shortcut-close-button")
                                            .icon(IconName::Close)
                                            .tooltip("닫기 · Esc")
                                            .w(px(32.))
                                            .px_0()
                                            .tab_stop(false)
                                            .on_click(
                                                cx.listener(|this, _, w, cx| this.close(w, cx)),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("shortcut-table")
                            .track_focus(&self.table_focus)
                            .tab_index(0)
                            .tab_stop(true)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.table_focus.focus(window, cx)
                                }),
                            )
                            .v_flex()
                            .role(Role::Table)
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(
                                table_row_frame(0, cx)
                                    .bg(cx.theme().bar)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(
                                        description_cell().role(Role::ColumnHeader).child("동작"),
                                    )
                                    .child(
                                        key_cell().py_2().role(Role::ColumnHeader).child("단축키"),
                                    )
                                    .child(
                                        reset_cell()
                                            .py_2()
                                            .role(Role::ColumnHeader)
                                            .child("기본값"),
                                    ),
                            )
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
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_start()
                            .flex_shrink_0()
                            .gap_3()
                            .child(
                                div()
                                    .debug_selector(|| "shortcut-message".into())
                                    .flex_1()
                                    .min_w_0()
                                    .text_color(if self.message.is_some() && self.message_error {
                                        cx.theme().warning
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .child(status),
                            )
                            .child(
                                div().debug_selector(|| "shortcut-reset-all".into()).child(
                                    Button::new("shortcuts-reset-all")
                                        .small()
                                        .label("모두 기본값")
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.reset(None, window, cx)
                                        })),
                                ),
                            ),
                    ),
            )
    }
}
