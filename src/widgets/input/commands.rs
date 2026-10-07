//! One definition for input bindings and the corresponding menu labels.
use super::*;

actions!(edot_input, [Undo, Redo, Cut, Copy, Paste, SelectAll]);

pub struct EditCommand {
    pub label: &'static str,
    pub shortcut: &'static str,
    key: &'static str,
    pub action: fn() -> Box<dyn Action>,
}
fn action<A: Action + Default>() -> Box<dyn Action> {
    Box::<A>::default()
}

pub const EDIT_COMMANDS: [EditCommand; 6] = [
    EditCommand {
        label: "실행 취소",
        shortcut: "Ctrl+Z",
        key: "ctrl-z",
        action: action::<Undo>,
    },
    EditCommand {
        label: "다시 실행",
        shortcut: "Ctrl+Y",
        key: "ctrl-y",
        action: action::<Redo>,
    },
    EditCommand {
        label: "잘라내기",
        shortcut: "Ctrl+X",
        key: "ctrl-x",
        action: action::<Cut>,
    },
    EditCommand {
        label: "복사",
        shortcut: "Ctrl+C",
        key: "ctrl-c",
        action: action::<Copy>,
    },
    EditCommand {
        label: "붙여넣기",
        shortcut: "Ctrl+V",
        key: "ctrl-v",
        action: action::<Paste>,
    },
    EditCommand {
        label: "전체 선택",
        shortcut: "Ctrl+A",
        key: "ctrl-a",
        action: action::<SelectAll>,
    },
];
pub fn init(cx: &mut App) {
    let context = Rc::new(KeyBindingContextPredicate::parse("EdotInput").unwrap());
    cx.bind_keys(EDIT_COMMANDS.iter().map(|command| {
        KeyBinding::load(
            command.key,
            (command.action)(),
            Some(context.clone()),
            false,
            None,
            &DummyKeyboardMapper,
        )
        .unwrap()
    }));
    cx.bind_keys([KeyBinding::new("ctrl-shift-z", Redo, Some("EdotInput"))]);
}
