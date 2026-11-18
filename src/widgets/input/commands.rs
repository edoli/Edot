//! One definition for input bindings and the corresponding menu labels.
use super::*;

actions!(edot_input, [Undo, Redo, Cut, Copy, Paste, SelectAll]);

pub struct EditCommand {
    pub label: &'static str,
    pub command: crate::shortcuts::Command,
    pub action: fn() -> Box<dyn Action>,
}
fn action<A: Action + Default>() -> Box<dyn Action> {
    Box::<A>::default()
}

pub const EDIT_COMMANDS: [EditCommand; 6] = [
    EditCommand {
        label: "실행 취소",
        command: crate::shortcuts::Command::Undo,
        action: action::<Undo>,
    },
    EditCommand {
        label: "다시 실행",
        command: crate::shortcuts::Command::Redo,
        action: action::<Redo>,
    },
    EditCommand {
        label: "잘라내기",
        command: crate::shortcuts::Command::Cut,
        action: action::<Cut>,
    },
    EditCommand {
        label: "복사",
        command: crate::shortcuts::Command::Copy,
        action: action::<Copy>,
    },
    EditCommand {
        label: "붙여넣기",
        command: crate::shortcuts::Command::Paste,
        action: action::<Paste>,
    },
    EditCommand {
        label: "전체 선택",
        command: crate::shortcuts::Command::SelectAll,
        action: action::<SelectAll>,
    },
];
pub fn init(cx: &mut App) {
    cx.set_global(crate::shortcuts::ActiveShortcuts::default());
}

pub(super) fn dispatch_key(event: &KeyDownEvent, window: &mut Window, cx: &mut App) -> bool {
    let command = cx
        .global::<crate::shortcuts::ActiveShortcuts>()
        .command(&event.keystroke, cfg!(target_os = "macos"));
    if let Some(edit) = EDIT_COMMANDS
        .iter()
        .find(|edit| Some(edit.command) == command)
    {
        window.dispatch_action((edit.action)(), cx);
        cx.stop_propagation();
        true
    } else {
        false
    }
}
