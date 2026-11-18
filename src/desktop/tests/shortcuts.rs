use super::*;
use crate::shortcuts::{ActiveShortcuts, Command};
use gpui::{Focusable, Keystroke};

fn with_help<R>(
    f: &Fixture,
    cx: &mut TestAppContext,
    update: impl FnOnce(
        &mut super::super::shortcuts::ShortcutHelp,
        &mut Window,
        &mut Context<super::super::shortcuts::ShortcutHelp>,
    ) -> R,
) -> R {
    let help = f.update(cx, |this, _, _| this.shortcut_help.clone().unwrap());
    cx.update_window(f.window, |_, window, cx| {
        help.update(cx, |help, cx| update(help, window, cx))
    })
    .unwrap()
}

#[gpui::test]
fn recording_blocks_commands_validates_and_persists_immediately(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let count = f.update(cx, |this, window, cx| {
        this.show_shortcuts(window, cx);
        this.book.notes.len()
    });
    with_help(&f, cx, |help, window, cx| {
        help.begin_recording(Command::NewNote, window, cx)
    });
    f.press(cx, "ctrl-s");
    f.update(cx, |this, _, cx| {
        assert_eq!(this.book.notes.len(), count);
        let help = this.shortcut_help.as_ref().unwrap().read(cx);
        assert_eq!(help.recording, Some(Command::NewNote));
        assert!(help.message.as_ref().unwrap().contains("사용 중"));
        assert!(cx.global::<ActiveShortcuts>().recording);
    });
    f.press(cx, "f11");
    f.update(cx, |this, window, cx| {
        assert!(!window.is_fullscreen());
        assert_eq!(
            this.shortcut_help.as_ref().unwrap().read(cx).recording,
            Some(Command::NewNote)
        );
    });
    f.press(cx, "ctrl-k");
    f.update(cx, |this, _, cx| {
        assert!(
            this.shortcut_help
                .as_ref()
                .unwrap()
                .read(cx)
                .recording
                .is_none()
        );
        assert!(!cx.global::<ActiveShortcuts>().recording);
        let stored = Settings::load(&cx.global::<AppState>().config_dir).unwrap();
        assert_eq!(stored.shortcuts.0[&Command::NewNote], "ctrl-n|ctrl-k");
    });
    with_help(&f, cx, |help, window, cx| {
        help.remove(Command::NewNote, 0, window, cx)
    });
    f.press(cx, "escape");
    f.press(cx, "ctrl-n");
    f.update(cx, |this, _, _| assert_eq!(this.book.notes.len(), count));
    f.press(cx, "ctrl-k");
    f.update(cx, |this, window, cx| {
        assert_eq!(this.book.notes.len(), count + 1);
        this.finish_rename(true, false, window, cx);
        this.show_shortcuts(window, cx);
    });
    with_help(&f, cx, |help, window, cx| {
        help.begin_recording(Command::NewNote, window, cx)
    });
    f.press(cx, "escape");
    with_help(&f, cx, |help, _, cx| {
        assert!(help.recording.is_none());
        assert_eq!(
            cx.global::<AppState>().settings.shortcuts.0[&Command::NewNote],
            "ctrl-k"
        );
    });
    with_help(&f, cx, |help, window, cx| {
        help.begin_recording(Command::NewNote, window, cx)
    });
    f.press(cx, "backspace");
    with_help(&f, cx, |help, window, cx| {
        assert!(help.recording.is_none());
        assert_eq!(
            cx.global::<AppState>().settings.shortcuts.0[&Command::NewNote],
            "ctrl-k"
        );
        help.remove(Command::NewNote, 0, window, cx);
        assert_eq!(
            cx.global::<AppState>().settings.shortcuts.0[&Command::NewNote],
            ""
        );
        help.reset(Some(Command::NewNote), window, cx);
        assert!(cx.global::<AppState>().settings.shortcuts.0.is_empty());
    });
}

#[gpui::test]
fn custom_editing_formatting_help_and_fullscreen_use_current_bindings(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        let mut next = cx.global::<AppState>().settings.clone();
        for (command, key) in [
            (Command::Bold, "ctrl-k"),
            (Command::Copy, "ctrl-j"),
            (Command::Help, "f12"),
            (Command::Fullscreen, "ctrl-f11"),
        ] {
            next.shortcuts
                .set(command, Some(&Keystroke::parse(key).unwrap()))
                .unwrap();
        }
        assert!(this.change_settings(next, window, cx));
        this.focus_editor(window, cx);
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| {
            state.set_value("hello", window, cx);
            state.set_selected_range(0..5, cx);
        });
        editor
    });
    f.press(cx, "ctrl-b");
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), "hello"));
    f.press(cx, "ctrl-k");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "**hello**")
    });
    f.press(cx, "ctrl-z");
    f.press(cx, "ctrl-j");
    cx.update(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "hello"));
    f.press(cx, "f1");
    f.update(cx, |this, _, _| assert!(this.shortcut_help.is_none()));
    f.press(cx, "f12");
    f.update(cx, |this, _, _| assert!(this.shortcut_help.is_some()));
    f.press(cx, "f11");
    f.update(cx, |_, window, _| assert!(!window.is_fullscreen()));
    f.press(cx, "ctrl-f11");
    f.update(cx, |_, window, _| assert!(window.is_fullscreen()));
    f.press(cx, "f12");
    f.update(cx, |this, window, cx| {
        assert!(this.shortcut_help.is_none());
        assert!(editor.read(cx).focus_handle(cx).is_focused(window));
    });
}

#[gpui::test]
fn settings_button_opens_the_same_editable_table_as_f1(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| this.show_settings(window, cx));
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let button = visual
        .debug_bounds("settings-shortcuts-button")
        .unwrap()
        .center();
    visual.simulate_click(button, Default::default());
    visual.update(|window, cx| window.render_frame(cx));
    assert!(visual.debug_bounds("settings-panel").is_none());
    assert!(visual.debug_bounds("shortcut-help").is_some());
    assert!(visual.debug_bounds("shortcut-record-new_note").is_some());
    let header = visual.debug_bounds("shortcut-table-header").unwrap();
    assert_eq!(
        visual
            .debug_bounds("shortcut-add-new_note")
            .unwrap()
            .right(),
        visual.debug_bounds("shortcut-add-help").unwrap().right()
    );
    with_help(&f, cx, |help, window, cx| {
        help.search
            .update(cx, |state, cx| state.set_value("새 노트", window, cx))
    });
    visual.update(|window, cx| window.render_frame(cx));
    assert!(visual.debug_bounds("shortcut-record-new_note").is_some());
    assert!(visual.debug_bounds("shortcut-record-save").is_none());
    let record = visual
        .debug_bounds("shortcut-key-new_note-0")
        .unwrap()
        .center();
    visual.simulate_click(record, Default::default());
    with_help(&f, cx, |help, _, _| assert!(help.recording.is_none()));
    let add = visual
        .debug_bounds("shortcut-add-new_note")
        .unwrap()
        .center();
    visual.simulate_click(add, Default::default());
    f.press(cx, "ctrl-k");
    with_help(&f, cx, |help, _, cx| {
        assert_eq!(
            cx.global::<AppState>().settings.shortcuts.0[&Command::NewNote],
            "ctrl-n|ctrl-k"
        );
        assert!(help.recording.is_none());
    });
    visual.update(|window, cx| window.render_frame(cx));
    let clear = visual
        .debug_bounds("shortcut-remove-new_note-0")
        .unwrap()
        .center();
    visual.simulate_click(clear, Default::default());
    with_help(&f, cx, |_, _, cx| {
        assert_eq!(
            cx.global::<AppState>().settings.shortcuts.0[&Command::NewNote],
            "ctrl-k"
        )
    });
    visual.update(|window, cx| window.render_frame(cx));
    let reset = visual
        .debug_bounds("shortcut-reset-new_note")
        .unwrap()
        .center();
    visual.simulate_click(reset, Default::default());
    with_help(&f, cx, |_, _, cx| {
        assert!(cx.global::<AppState>().settings.shortcuts.0.is_empty())
    });
    f.press(cx, "escape");
    f.press(cx, "f1");
    visual.update(|window, cx| window.render_frame(cx));
    assert_eq!(
        visual.debug_bounds("shortcut-table-header").unwrap(),
        header
    );
    assert!(visual.debug_bounds("shortcut-record-new_note").is_some());
    assert!(visual.debug_bounds("shortcut-remove-new_note-0").is_some());
    assert!(visual.debug_bounds("shortcut-reset-new_note").is_some());
}

#[gpui::test]
fn failed_save_keeps_bindings_and_focus_loss_cancels_recording(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    cx.update_window(f.window, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    let blocked = f.dir.join("blocked-shortcut-config");
    fs::write(&blocked, "preserve").unwrap();
    f.update(cx, |this, window, cx| {
        cx.global_mut::<AppState>().config_dir = blocked.clone();
        this.show_shortcuts(window, cx);
    });
    with_help(&f, cx, |help, window, cx| {
        help.begin_recording(Command::NewNote, window, cx)
    });
    f.press(cx, "ctrl-k");
    with_help(&f, cx, |help, _, cx| {
        assert!(
            help.message
                .as_ref()
                .unwrap()
                .contains("저장하지 못했습니다")
        );
        assert!(help.recording.is_none());
        assert!(cx.global::<AppState>().settings.shortcuts.0.is_empty());
        assert_eq!(
            cx.global::<ActiveShortcuts>()
                .command(&Keystroke::parse("ctrl-n").unwrap(), false),
            Some(Command::NewNote)
        );
    });
    with_help(&f, cx, |help, window, cx| {
        help.begin_recording(Command::NewNote, window, cx)
    });
    f.update(cx, |this, window, cx| {
        assert!(this.error.is_none());
        this.focus_editor(window, cx);
    });
    cx.update_window(f.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
    with_help(&f, cx, |help, _, cx| {
        assert!(help.recording.is_none());
        assert!(!cx.global::<ActiveShortcuts>().recording);
    });
    assert_eq!(fs::read_to_string(blocked).unwrap(), "preserve");
}

#[gpui::test]
fn shortcut_list_adds_without_replacing_and_removes_one_key(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| this.show_shortcuts(window, cx));
    with_help(&f, cx, |help, window, cx| {
        help.search
            .update(cx, |input, cx| input.set_value("새 노트", window, cx))
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let add = visual
        .debug_bounds("shortcut-add-new_note")
        .unwrap()
        .center();
    visual.simulate_click(add, Default::default());
    f.press(cx, "ctrl-k");
    with_help(&f, cx, |_, _, cx| {
        assert_eq!(
            cx.global::<AppState>().settings.shortcuts.0[&Command::NewNote],
            "ctrl-n|ctrl-k"
        );
        assert_eq!(
            Settings::load(&cx.global::<AppState>().config_dir).unwrap(),
            cx.global::<AppState>().settings
        );
        for key in ["ctrl-n", "ctrl-k"] {
            assert_eq!(
                cx.global::<ActiveShortcuts>()
                    .command(&Keystroke::parse(key).unwrap(), false),
                Some(Command::NewNote)
            );
        }
    });
    visual.update(|window, cx| window.render_frame(cx));
    assert!(visual.debug_bounds("shortcut-key-new_note-0").is_some());
    assert!(visual.debug_bounds("shortcut-key-new_note-1").is_some());
    let first = visual.debug_bounds("shortcut-key-new_note-0").unwrap();
    let second = visual.debug_bounds("shortcut-key-new_note-1").unwrap();
    let add_bounds = visual.debug_bounds("shortcut-add-new_note").unwrap();
    assert_eq!(first.top(), second.top());
    assert_eq!(first.top(), add_bounds.top());
    let add = visual
        .debug_bounds("shortcut-add-new_note")
        .unwrap()
        .center();
    visual.simulate_click(add, Default::default());
    f.press(cx, "ctrl-k");
    with_help(&f, cx, |help, _, cx| {
        assert_eq!(help.recording, Some(Command::NewNote));
        assert!(help.message.as_ref().unwrap().contains("이미 등록"));
        assert_eq!(
            cx.global::<AppState>().settings.shortcuts.0[&Command::NewNote],
            "ctrl-n|ctrl-k"
        );
    });
    f.press(cx, "escape");
    visual.update(|window, cx| window.render_frame(cx));
    let second = visual
        .debug_bounds("shortcut-key-new_note-1")
        .unwrap()
        .center();
    visual.simulate_click(second, Default::default());
    with_help(&f, cx, |help, _, _| assert!(help.recording.is_none()));
    let remove_second = visual
        .debug_bounds("shortcut-remove-new_note-1")
        .unwrap()
        .center();
    visual.simulate_click(remove_second, Default::default());
    visual.update(|window, cx| window.render_frame(cx));
    let add = visual
        .debug_bounds("shortcut-add-new_note")
        .unwrap()
        .center();
    visual.simulate_click(add, Default::default());
    f.press(cx, "ctrl-j");
    with_help(&f, cx, |_, _, cx| {
        assert_eq!(
            cx.global::<AppState>().settings.shortcuts.0[&Command::NewNote],
            "ctrl-n|ctrl-j"
        )
    });
    visual.update(|window, cx| window.render_frame(cx));
    let remove = visual
        .debug_bounds("shortcut-remove-new_note-0")
        .unwrap()
        .center();
    visual.simulate_click(remove, Default::default());
    with_help(&f, cx, |_, _, cx| {
        assert_eq!(
            cx.global::<AppState>().settings.shortcuts.0[&Command::NewNote],
            "ctrl-j"
        );
        assert_eq!(
            cx.global::<ActiveShortcuts>()
                .command(&Keystroke::parse("ctrl-n").unwrap(), false),
            None
        );
        assert_eq!(
            cx.global::<ActiveShortcuts>()
                .command(&Keystroke::parse("ctrl-j").unwrap(), false),
            Some(Command::NewNote)
        );
    });
}
