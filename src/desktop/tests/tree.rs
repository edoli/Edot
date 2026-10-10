use super::*;
use crate::widgets::WindowExt;

#[gpui::test]
fn tree_background_menu_creates_root_notes_with_a_nested_caret_and_an_empty_tree(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    for empty in [false, true] {
        f.update(cx, |this, window, cx| {
            if empty {
                let ids: Vec<_> = this.book.notes.iter().map(|note| note.id).collect();
                assert!(this.delete_confirmed(&ids, window, cx));
                assert!(this.book.notes.is_empty());
            } else {
                this.tree_click(f.ids[2], Default::default(), window, cx);
            }
        });
        visual.update(|window, cx| window.render_frame(cx));
        let host = visual.debug_bounds("tree-host").unwrap();
        let blank = point(host.left() + px(35.), host.bottom() - px(20.));
        visual.simulate_mouse_down(blank, gpui::MouseButton::Right, Default::default());
        f.update(cx, |this, _, _| {
            assert!(this.keyboard_menu.is_some());
            assert!(this.context_note.is_none());
            if !empty {
                assert_eq!(this.caret, Some(f.ids[2]));
                assert_eq!(this.session.active, Some(f.ids[2]));
                assert_eq!(
                    this.selection.ids.as_ref(),
                    &std::collections::HashSet::from([f.ids[2]])
                );
            }
        });
        f.press(cx, "enter");
        f.update(cx, |this, _, _| {
            assert!(this.keyboard_menu.is_none());
            let id = this.session.active.unwrap();
            let note = this.book.get(id).unwrap();
            assert_eq!(note.parent, 0);
            assert_eq!(note.title, "새 노트");
            assert_eq!(this.book.notes.len(), if empty { 1 } else { 4 });
            assert_eq!(this.rename.as_ref().map(|(id, _)| *id), Some(id));
            assert!(this.book.file(&this.dir, id).is_file());
        });
        f.press(cx, "escape");
    }
}

#[gpui::test]
fn delete_dialog_keeps_controls_inline_and_warns_only_for_nonempty_folders(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    for (selected, warning) in [
        (vec![f.ids[1]], false),
        (vec![f.ids[1], f.ids[2]], false),
        (vec![f.ids[0]], true),
        (vec![f.ids[1], f.ids[0]], true),
        (vec![f.ids[2], f.ids[0], f.ids[0]], true),
    ] {
        f.update(cx, |this, window, cx| {
            this.confirm_delete_notes(selected, window, cx)
        });
        visual.update(|window, cx| {
            window.render_frame(cx);
        });
        let checkbox = visual.debug_bounds("delete-dont-ask-again").unwrap();
        let footer = visual.debug_bounds("confirmation-footer").unwrap();
        assert_eq!(checkbox.center().y, footer.center().y);
        assert_eq!(visual.debug_bounds("delete-warning").is_some(), warning);
        f.press(cx, "escape");
    }
    f.update(cx, |this, _, _| assert_eq!(this.book.notes.len(), 3));
}

#[gpui::test]
fn delete_opt_out_persists_only_on_confirmation_and_skips_future_dialogs(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        let lock = cx.global::<AppState>().config_dir.join(".settings.lock");
        if lock.is_file() {
            fs::remove_file(&lock).unwrap();
        }
        fs::create_dir(&lock).unwrap();
        assert!(!this.remember_delete_choice(true, cx));
        assert!(cx.global::<AppState>().settings.confirm_delete);
        assert!(this.book.get(f.ids[2]).is_some());
        fs::remove_dir(lock).unwrap();
        this.error = None;
        this.confirm_delete_notes(vec![f.ids[2]], window, cx);
    });
    f.press(cx, "shift-tab");
    f.press(cx, "space");
    f.press(cx, "escape");
    f.update(cx, |this, window, cx| {
        assert!(cx.global::<AppState>().settings.confirm_delete);
        assert!(
            Settings::load(&cx.global::<AppState>().config_dir)
                .unwrap()
                .confirm_delete
        );
        assert!(this.book.get(f.ids[2]).is_some());
        this.confirm_delete_notes(vec![f.ids[2]], window, cx);
    });
    // Mouse toggling and keyboard navigation both leave confirmation explicit.
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let checkbox = visual.debug_bounds("delete-dont-ask-again").unwrap();
    visual.simulate_mouse_down(
        checkbox.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_up(
        checkbox.center(),
        gpui::MouseButton::Left,
        Default::default(),
    );
    f.press(cx, "tab");
    f.press(cx, "enter");
    f.update(cx, |this, window, cx| {
        assert!(!window.has_active_dialog(cx));
        assert!(this.book.get(f.ids[2]).is_none());
        assert!(!cx.global::<AppState>().settings.confirm_delete);
        let saved = Settings::load(&cx.global::<AppState>().config_dir).unwrap();
        assert!(!saved.confirm_delete);
        cx.global_mut::<AppState>().settings = saved;
        this.confirm_delete_notes(vec![f.ids[1]], window, cx);
        assert!(!window.has_active_dialog(cx));
        assert!(this.book.get(f.ids[1]).is_none());
        this.confirm_remove_space(f.dir.clone(), window, cx);
        assert!(!window.has_active_dialog(cx));
        assert!(this.space_ui.registry.entries.is_empty());
        assert!(this.book.file(&this.dir, f.ids[0]).is_file());
    });
}

#[gpui::test]
fn tree_context_target_and_active_note_survive_selection_and_menu_dismissal(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let selector: &'static str = format!("tree-note-{}", f.ids[1]).leak();
    let row = visual.debug_bounds(selector).unwrap();
    visual.simulate_mouse_down(row.center(), gpui::MouseButton::Right, Default::default());
    visual.update(|window, cx| window.render_frame(cx));
    f.update(cx, |this, _, _| {
        assert_eq!(this.context_note, Some(f.ids[1]));
        assert!(this.keyboard_menu.is_some());
        assert_eq!(this.session.active, Some(f.ids[0]));
        assert_eq!(this.caret, Some(f.ids[0]));
        assert_eq!(
            this.selection.ids.as_ref(),
            &std::collections::HashSet::from([f.ids[0]])
        );
    });
    f.press(cx, "escape");
    f.update(cx, |this, _, _| {
        assert!(this.context_note.is_none());
        assert!(this.keyboard_menu.is_none());
    });
    let host = visual.debug_bounds("tree-host").unwrap();
    let blank = point(host.left() + px(35.), host.bottom() - px(20.));
    visual.simulate_mouse_down(blank, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(blank, gpui::MouseButton::Left, Default::default());
    f.update(cx, |this, window, cx| {
        assert_eq!(
            this.selection.ids.as_ref(),
            &std::collections::HashSet::from([f.ids[0]])
        );
        this.tree_click(
            f.ids[1],
            gpui::Modifiers {
                control: true,
                ..Default::default()
            },
            window,
            cx,
        );
        assert_eq!(this.selection.ids.len(), 2);
        this.show_tree_menu(f.ids[1], point(px(50.), px(90.)), window, cx);
        assert_eq!(this.context_note, Some(f.ids[1]));
        assert_eq!(this.selection.ids.len(), 2);
        assert_eq!(this.session.active, Some(f.ids[0]));
    });
    visual.update(|window, cx| window.render_frame(cx));
    visual.simulate_mouse_down(blank, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(blank, gpui::MouseButton::Left, Default::default());
    f.update(cx, |this, window, cx| {
        this.focus_editor(window, cx);
        assert!(this.selection.ids.contains(&f.ids[0]));
        assert!(this.context_note.is_none());
        assert!(this.keyboard_menu.is_none());
    });

    // A menu outside the multi-selection must operate on its own target.
    f.update(cx, |this, window, cx| {
        this.tree_click(
            f.ids[1],
            gpui::Modifiers {
                control: true,
                ..Default::default()
            },
            window,
            cx,
        );
        this.show_tree_menu(f.ids[2], point(px(50.), px(90.)), window, cx);
        assert_eq!(this.context_note, Some(f.ids[2]));
        assert_eq!(this.caret, Some(f.ids[1]));
        assert_eq!(
            this.selection.ids.as_ref(),
            &std::collections::HashSet::from([f.ids[0], f.ids[1]])
        );
    });
    f.press(cx, "down");
    f.press(cx, "enter");
    f.update(cx, |this, _, _| {
        assert_eq!(this.rename.as_ref().map(|(id, _)| *id), Some(f.ids[2]));
        assert_eq!(this.selection.ids.len(), 2);
    });
    f.press(cx, "escape");
    f.update(cx, |this, window, cx| {
        this.confirm_delete_notes(vec![f.ids[2]], window, cx);
    });
    f.press(cx, "tab");
    f.press(cx, "enter");
    f.update(cx, |this, window, cx| {
        assert!(this.book.get(f.ids[2]).is_some());
        assert!(!window.has_active_dialog(cx));
    });
    f.update(cx, |this, window, cx| {
        this.show_tree_menu(f.ids[2], point(px(50.), px(90.)), window, cx);
    });
    for _ in 0..4 {
        f.press(cx, "down");
    }
    f.press(cx, "enter");
    f.press(cx, "enter");
    f.update(cx, |this, _, _| {
        assert!(this.book.get(f.ids[2]).is_none());
        assert!(this.book.get(f.ids[0]).is_some());
        assert!(this.book.get(f.ids[1]).is_some());
        assert_eq!(this.selection.ids.len(), 2);
        assert_eq!(this.session.active, Some(f.ids[0]));
    });
    assert!(f.dir.join(format!("notes/{}.txt", f.ids[0])).is_file());
    assert!(f.dir.join(format!("notes/{}.txt", f.ids[1])).is_file());
}

#[gpui::test]
fn opening_external_note_flushes_content_and_blocks_failed_or_missing_files(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| {
            state.replace_all("외부에서 열 최신 내용", window, cx)
        });
        let path = this.prepare_external_note(f.ids[0], cx).unwrap();
        assert_eq!(path, this.book.file(&this.dir, f.ids[0]));
        assert_eq!(fs::read_to_string(&path).unwrap(), "외부에서 열 최신 내용");
        editor.update(cx, |state, cx| {
            state.replace_all("저장 실패 내용", window, cx)
        });
        let temporary = path.with_extension("tmp");
        fs::create_dir(&temporary).unwrap();
        assert!(this.prepare_external_note(f.ids[0], cx).is_none());
        assert_eq!(fs::read_to_string(&path).unwrap(), "외부에서 열 최신 내용");
        assert_eq!(editor.read(cx).value(), "저장 실패 내용");
        fs::remove_dir(temporary).unwrap();
        this.error = None;
        assert!(this.prepare_external_note(f.ids[0], cx).is_some());
        fs::remove_file(&path).unwrap();
        assert!(this.prepare_external_note(f.ids[0], cx).is_none());
        assert!(!path.exists());
    });
}

#[gpui::test]
fn rename_pasted_tabs_and_newlines_become_spaces(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.begin_rename(f.ids[0], window, cx);
        this.title_input.update(cx, |state, cx| {
            state.replace_all("  한글\t제목\r\n새 줄\r끝\n😀  ", window, cx);
        });
        assert!(this.finish_rename(true, true, window, cx));
        assert_eq!(
            this.book.get(f.ids[0]).unwrap().title,
            "한글 제목 새 줄 끝 😀"
        );
        assert!(!this.open[0].dirty);
    });
    let index = fs::read_to_string(f.dir.join("tree.tsv")).unwrap();
    assert!(index.starts_with("EDOT4\n"));
    assert!(index.contains("\t한글 제목 새 줄 끝 😀\n"));
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[0]))).unwrap(),
        "🙂 한글12\r\n한글3\rkeep\n"
    );
}
