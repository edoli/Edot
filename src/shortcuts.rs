//! Command catalog and configurable, single-stroke shortcuts shared by the UI.
use gpui::{App, Global, Keystroke};
use std::collections::BTreeMap;

macro_rules! commands {
    ($( $name:ident, $id:literal, $group:literal, $label:literal, $keys:literal, $mac:literal; )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Command { $( $name, )* }
        pub const COMMANDS: &[Command] = &[$(Command::$name,)*];
        impl Command {
            pub fn id(self) -> &'static str { match self { $(Self::$name => $id,)* } }
            pub fn group(self) -> &'static str { match self { $(Self::$name => $group,)* } }
            pub fn label(self) -> &'static str { match self { $(Self::$name => $label,)* } }
            fn defaults(self, macos: bool) -> String {
                match self { $(Self::$name => if macos {
                    if $mac.is_empty() { $keys.replace("ctrl-", "cmd-") } else { $mac.into() }
                } else { $keys.into() },)* }
            }
        }
    }
}
commands! {
    Help, "help", "앱", "단축키 안내", "f1", "";
    Fullscreen, "fullscreen", "앱", "전체화면 전환", "f11", "f11|ctrl-cmd-f";
    Settings, "settings", "앱", "설정 열기 / 닫기", "ctrl-,", "";
    NewNote, "new_note", "노트 · 탭", "새 노트", "ctrl-n", "";
    NewFolder, "new_folder", "노트 · 탭", "새 폴더", "ctrl-shift-n", "";
    Save, "save", "노트 · 탭", "수정한 노트 저장", "ctrl-s", "";
    CloseTab, "close_tab", "노트 · 탭", "현재 탭 닫기", "ctrl-w|ctrl-f4", "cmd-w";
    ReopenTab, "reopen_tab", "노트 · 탭", "닫은 탭 다시 열기", "ctrl-shift-t", "";
    NextTab, "next_tab", "노트 · 탭", "다음 탭", "ctrl-tab|ctrl-pagedown", "ctrl-tab|ctrl-pagedown";
    PreviousTab, "previous_tab", "노트 · 탭", "이전 탭", "ctrl-shift-tab|ctrl-pageup", "ctrl-shift-tab|ctrl-pageup";
    Tab1, "tab_1", "노트 · 탭", "1번 탭", "ctrl-1", "";
    Tab2, "tab_2", "노트 · 탭", "2번 탭", "ctrl-2", "";
    Tab3, "tab_3", "노트 · 탭", "3번 탭", "ctrl-3", "";
    Tab4, "tab_4", "노트 · 탭", "4번 탭", "ctrl-4", "";
    Tab5, "tab_5", "노트 · 탭", "5번 탭", "ctrl-5", "";
    Tab6, "tab_6", "노트 · 탭", "6번 탭", "ctrl-6", "";
    Tab7, "tab_7", "노트 · 탭", "7번 탭", "ctrl-7", "";
    Tab8, "tab_8", "노트 · 탭", "8번 탭", "ctrl-8", "";
    LastTab, "last_tab", "노트 · 탭", "마지막 탭", "ctrl-9", "";
    Sidebar, "sidebar", "노트 · 탭", "사이드바 표시 / 숨기기", "ctrl-\\", "";
    Rename, "rename", "노트 · 탭", "이름 바꾸기", "f2", "";
    ReadOnly, "read_only", "본문", "읽기 / 편집 모드 전환", "ctrl-shift-v", "";
    Wrap, "wrap", "본문", "자동 줄 바꿈 전환", "alt-z", "cmd-alt-z";
    DateTime, "date_time", "본문", "시간과 날짜 삽입", "f5", "";
    Date, "date", "본문", "날짜 삽입", "f6", "";
    ZoomIn, "zoom_in", "본문", "본문 확대", "ctrl-+|ctrl-=|ctrl-add", "";
    ZoomOut, "zoom_out", "본문", "본문 축소", "ctrl--|ctrl-subtract", "";
    ZoomReset, "zoom_reset", "본문", "본문 확대율 초기화", "ctrl-0", "";
    Bold, "bold", "본문", "Markdown 굵게", "ctrl-b", "";
    Italic, "italic", "본문", "Markdown 기울임", "ctrl-i", "";
    Strike, "strike", "본문", "Markdown 취소선", "ctrl-shift-`", "cmd-shift-x";
    Code, "code", "본문", "Markdown 인라인 코드", "ctrl-e", "";
    NextOccurrence, "next_occurrence", "본문", "다음 일치 항목 추가 선택", "ctrl-d", "";
    Undo, "undo", "편집", "실행 취소", "ctrl-z", "";
    Redo, "redo", "편집", "다시 실행", "ctrl-y|ctrl-shift-z", "cmd-shift-z";
    Cut, "cut", "편집", "잘라내기", "ctrl-x", "";
    Copy, "copy", "편집", "복사", "ctrl-c", "";
    Paste, "paste", "편집", "붙여넣기", "ctrl-v", "";
    SelectAll, "select_all", "편집", "전체 선택", "ctrl-a", "";
    Find, "find", "검색", "현재 노트 찾기", "ctrl-f", "";
    Replace, "replace", "검색", "현재 노트 바꾸기", "ctrl-h", "cmd-alt-f";
    GlobalFind, "global_find", "검색", "전체 노트 찾기", "ctrl-shift-f", "";
    GlobalReplace, "global_replace", "검색", "전체 노트 바꾸기", "ctrl-shift-h", "cmd-alt-shift-f";
    NextMatch, "next_match", "검색", "다음 검색 결과", "f3", "";
    PreviousMatch, "previous_match", "검색", "이전 검색 결과", "shift-f3", "";
}

/// Only overrides are stored. An empty value disables a command; `default` restores it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bindings(pub BTreeMap<Command, String>);
impl Bindings {
    pub fn keys(&self, command: Command, macos: bool) -> Vec<Keystroke> {
        let value = self
            .0
            .get(&command)
            .cloned()
            .unwrap_or_else(|| command.defaults(macos));
        value
            .split('|')
            .filter(|s| !s.is_empty())
            .filter_map(|s| Keystroke::parse(s).ok())
            .collect()
    }
    pub fn label(&self, command: Command) -> String {
        self.keys(command, cfg!(target_os = "macos"))
            .iter()
            .map(display_key)
            .collect::<Vec<_>>()
            .join(" / ")
    }
    pub fn set(&mut self, command: Command, key: Option<&Keystroke>) -> Result<(), String> {
        if let Some(key) = key {
            validate_key(key)?;
            if let Some(other) = COMMANDS.iter().copied().find(|&other| {
                other != command
                    && self
                        .keys(other, cfg!(target_os = "macos"))
                        .iter()
                        .any(|k| same_key(k, key))
            }) {
                return Err(format!("이미 ‘{}’에 사용 중입니다.", other.label()));
            }
        }
        self.0.insert(
            command,
            key.map_or_else(String::new, |key| normalized(key).unparse()),
        );
        Ok(())
    }
    /// Replace one list item, or append while retaining all existing keys.
    pub fn edit(
        &mut self,
        command: Command,
        index: Option<usize>,
        key: &Keystroke,
    ) -> Result<(), String> {
        validate_key(key)?;
        let mut keys = self.keys(command, cfg!(target_os = "macos"));
        if keys
            .iter()
            .enumerate()
            .any(|(i, existing)| Some(i) != index && same_key(existing, key))
        {
            return Err("이미 등록된 단축키입니다.".into());
        }
        if let Some(index) = index {
            let existing = keys.get_mut(index).ok_or("단축키를 찾을 수 없습니다.")?;
            *existing = normalized(key);
        } else {
            keys.push(normalized(key));
        }
        let mut next = self.clone();
        next.store_keys(command, &keys);
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn remove(&mut self, command: Command, index: usize) -> Result<(), String> {
        let mut keys = self.keys(command, cfg!(target_os = "macos"));
        if index >= keys.len() {
            return Err("단축키를 찾을 수 없습니다.".into());
        }
        keys.remove(index);
        self.store_keys(command, &keys);
        Ok(())
    }

    fn store_keys(&mut self, command: Command, keys: &[Keystroke]) {
        self.0.insert(
            command,
            keys.iter()
                .map(Keystroke::unparse)
                .collect::<Vec<_>>()
                .join("|"),
        );
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut seen: Vec<(Command, Keystroke)> = Vec::new();
        for &command in COMMANDS {
            if let Some(value) = self.0.get(&command)
                && !value.is_empty()
            {
                for item in value.split('|') {
                    let key = Keystroke::parse(item)
                        .map_err(|_| "단축키 형식이 잘못되었습니다.".to_owned())?;
                    validate_key(&key)?;
                }
            }
            for key in self.keys(command, cfg!(target_os = "macos")) {
                if let Some((other, _)) = seen.iter().find(|(_, k)| same_key(k, &key)) {
                    return Err(if *other == command {
                        "이미 등록된 단축키입니다.".into()
                    } else {
                        format!("이미 ‘{}’에 사용 중입니다.", other.label())
                    });
                }
                seen.push((command, key));
            }
        }
        Ok(())
    }
}

fn normalized(key: &Keystroke) -> Keystroke {
    let mut key = key.clone();
    let base = match key.key.as_str() {
        "~" => Some("`"),
        "!" => Some("1"),
        "@" => Some("2"),
        "#" => Some("3"),
        "$" => Some("4"),
        "%" => Some("5"),
        "^" => Some("6"),
        "&" => Some("7"),
        "*" => Some("8"),
        "(" => Some("9"),
        ")" => Some("0"),
        "_" => Some("-"),
        "+" => Some("="),
        "{" => Some("["),
        "}" => Some("]"),
        "|" => Some("\\"),
        ":" => Some(";"),
        "\"" => Some("'"),
        "<" => Some(","),
        ">" => Some("."),
        "?" => Some("/"),
        _ => None,
    };
    if let Some(base) = base {
        key.key = base.into();
        key.modifiers.shift = true;
    }
    key.key_char = None;
    key
}
fn same_key(a: &Keystroke, b: &Keystroke) -> bool {
    normalized(a) == normalized(b)
}

pub fn display_key(key: &Keystroke) -> String {
    let mut label = String::new();
    let m = key.modifiers;
    for (enabled, text) in [
        (m.control, "Ctrl+"),
        (
            m.platform,
            if cfg!(target_os = "macos") {
                "Cmd+"
            } else {
                "Win+"
            },
        ),
        (
            m.alt,
            if cfg!(target_os = "macos") {
                "Option+"
            } else {
                "Alt+"
            },
        ),
        (m.shift, "Shift+"),
    ] {
        if enabled {
            label.push_str(text);
        }
    }
    label.push_str(match key.key.as_str() {
        "tab" => "Tab",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "add" => "숫자패드 +",
        "subtract" => "숫자패드 -",
        _ => return label + &key.key.to_uppercase(),
    });
    label
}

fn validate_key(key: &Keystroke) -> Result<(), String> {
    let key = normalized(key);
    let m = key.modifiers;
    let function_key = key
        .key
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n));
    let printable =
        key.key.len() == 1 && key.key.is_ascii() && !key.key.chars().any(char::is_control);
    let keypad = matches!(key.key.as_str(), "add" | "subtract");
    let tab_key = m.control
        && !m.alt
        && !m.platform
        && matches!(key.key.as_str(), "tab" | "pageup" | "pagedown");
    if m.function
        || m.platform && !cfg!(target_os = "macos")
        || m.control && m.alt
        || !(function_key || (printable || keypad) && (m.control || m.platform || m.alt) || tab_key)
        || cfg!(target_os = "macos") && m.control && !m.platform && !tab_key
        || cfg!(target_os = "macos") && key.key == "q" && m.platform
        || cfg!(target_os = "macos")
            && key.key == "w"
            && m.platform
            && m.shift
            && !m.alt
            && !m.control
        || key.key == "f10"
            && m == (gpui::Modifiers {
                shift: true,
                ..Default::default()
            })
        || !cfg!(target_os = "macos") && key.key == "f4" && m.alt
    {
        return Err("Ctrl/Cmd/Alt와 문자·숫자 키를 조합하거나 F1~F24를 사용하세요. 기본 입력·이동 키와 시스템 키는 지정할 수 없습니다.".into());
    }
    Ok(())
}

/// Compiled only when settings change, rather than parsing bindings on every keypress.
pub struct ActiveShortcuts {
    pub bindings: Bindings,
    pub recording: bool,
    windows: Vec<(Command, Keystroke)>,
    macos: Vec<(Command, Keystroke)>,
}
impl Global for ActiveShortcuts {}
impl Default for ActiveShortcuts {
    fn default() -> Self {
        Self::new(Bindings::default())
    }
}
impl ActiveShortcuts {
    pub fn new(bindings: Bindings) -> Self {
        let compile = |macos| {
            COMMANDS
                .iter()
                .flat_map(|&command| {
                    bindings
                        .keys(command, macos)
                        .into_iter()
                        .map(move |key| (command, normalized(&key)))
                })
                .collect()
        };
        Self {
            windows: compile(false),
            macos: compile(true),
            bindings,
            recording: false,
        }
    }
    pub fn command(&self, key: &Keystroke, macos: bool) -> Option<Command> {
        let key = normalized(key);
        (if macos { &self.macos } else { &self.windows })
            .iter()
            .find(|(_, k)| *k == key)
            .map(|(command, _)| *command)
    }
}
pub fn label(command: Command, cx: &App) -> String {
    cx.global::<ActiveShortcuts>().bindings.label(command)
}
pub fn tip(command: Command, text: &str, cx: &App) -> String {
    let keys = label(command, cx);
    if keys.is_empty() {
        text.into()
    } else {
        format!("{text} · {keys}")
    }
}

#[cfg(test)]
mod tests;
