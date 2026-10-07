use super::*;

#[gpui::test]
fn tabs_restore_lazily_and_keep_selection_history_through_settings(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        assert!(this.open[1].editor.is_none());
        this.focus_editor(window, cx);
        this.active_editor().unwrap()
    });
    f.press(cx, "ctrl-end");
    f.input(cx, "추가");
    let range = f.update(cx, |this, window, cx| {
        let range = editor.read(cx).selected_range();
        let mut settings = this.settings.clone();
        settings.wrap = false;
        settings.font_size = 24;
        settings.theme = ThemeMode::Dark;
        this.change_settings(settings, window, cx);
        assert_eq!(editor.read(cx).selected_range(), range);
        this.open_note(f.ids[1], true, window, cx);
        this.open_note(f.ids[0], true, window, cx);
        assert_eq!(
            this.active_editor().unwrap().entity_id(),
            editor.entity_id()
        );
        assert_eq!(editor.read(cx).selected_range(), range);
        range
    });
    assert!(range.start > 0);
    f.press(cx, "ctrl-z");
    f.update(cx, |this, _, cx| {
        assert_eq!(editor.read(cx).value(), "🙂 한글12\r\n한글3\rkeep\n");
        assert!(this.save_all(cx));
    });
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[1]))).unwrap(),
        "closed needle\n"
    );
}

#[gpui::test]
fn close_reorder_and_delete_preserve_surviving_editors_and_files(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        let first = this.active_editor().unwrap();
        this.open_note(f.ids[1], false, window, cx);
        let second = this.active_editor().unwrap();
        this.open_note(f.ids[2], false, window, cx);
        this.reorder_tab(f.ids[2], 0, cx);
        assert_eq!(
            Session::load(&f.dir, &this.book).tabs,
            vec![f.ids[2], f.ids[0], f.ids[1]]
        );
        this.close_tabs(f.ids[1], crate::tabs::CloseMode::Current, window, cx);
        assert!(this.book.file(&f.dir, f.ids[1]).exists());
        assert_eq!(
            this.open
                .iter()
                .find(|tab| tab.id == f.ids[0])
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .entity_id(),
            first.entity_id()
        );
        this.open_note(f.ids[1], false, window, cx);
        assert_ne!(
            this.active_editor().unwrap().entity_id(),
            second.entity_id()
        );
        let surviving = this.active_editor().unwrap();
        assert!(this.delete_confirmed(&[f.ids[0]], window, cx));
        assert!(this.book.get(f.ids[0]).is_none());
        assert!(this.book.get(f.ids[2]).is_none());
        assert_eq!(
            this.active_editor().unwrap().entity_id(),
            surviving.entity_id()
        );
    });
    assert_eq!(fs::read_dir(f.dir.join("trash")).unwrap().count(), 2);
}

#[gpui::test]
fn failed_save_retains_dirty_editor_and_blocks_closing(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.replace_all("미저장 본문", window, cx));
        this.changed(f.ids[0], window, cx);
        fs::create_dir(this.book.file(&f.dir, f.ids[0]).with_extension("tmp")).unwrap();
        assert!(!this.save_all(cx));
        this.close_tabs(f.ids[0], crate::tabs::CloseMode::Current, window, cx);
        assert!(this.closed_tabs.is_empty());
        assert!(this.open[0].dirty);
        assert!(this.error.is_some());
        assert_eq!(editor.read(cx).value(), "미저장 본문");
    });
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[0]))).unwrap(),
        "🙂 한글12\r\n한글3\rkeep\n"
    );
}

#[gpui::test]
fn tab_close_modes_keep_expected_tabs_and_never_delete_notes(cx: &mut TestAppContext) {
    use crate::tabs::CloseMode;
    for (mode, indices) in [
        (CloseMode::Current, vec![0, 2]),
        (CloseMode::Others, vec![1]),
        (CloseMode::ToRight, vec![0, 1]),
    ] {
        let f = Fixture::new(cx);
        f.update(cx, |this, w, cx| {
            this.open_note(f.ids[2], true, w, cx);
            this.close_tabs(f.ids[1], mode, w, cx);
            let expected: Vec<_> = indices.iter().map(|&i| f.ids[i]).collect();
            assert_eq!(
                this.open.iter().map(|tab| tab.id).collect::<Vec<_>>(),
                expected
            );
            assert!(this.session.active.is_some_and(|id| expected.contains(&id)));
            for id in f.ids {
                assert!(this.book.file(&this.dir, id).exists());
            }
        });
        for _ in indices.len()..3 {
            f.press(cx, "ctrl-shift-t");
        }
        f.update(cx, |this, _, _| {
            assert_eq!(
                this.open.iter().map(|tab| tab.id).collect::<Vec<_>>(),
                f.ids
            );
            assert!(this.closed_tabs.is_empty());
        });
    }
}

#[gpui::test]
fn close_and_reopen_shortcuts_save_content_and_restore_tab_position(cx: &mut TestAppContext) {
    use gpui::Focusable;
    let f = Fixture::new(cx);
    f.update(cx, |this, w, cx| {
        this.open_note(f.ids[2], true, w, cx);
        this.open_note(f.ids[1], true, w, cx);
        this.active_editor()
            .unwrap()
            .update(cx, |s, cx| s.replace_all("수정한 메모", w, cx));
    });
    f.press(cx, "ctrl-f4");
    f.update(cx, |this, _, _| {
        assert_eq!(
            this.open.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            vec![f.ids[0], f.ids[2]]
        );
        assert_eq!(
            fs::read_to_string(this.book.file(&f.dir, f.ids[1])).unwrap(),
            "수정한 메모"
        );
    });
    f.press(cx, "ctrl-shift-t");
    f.update(cx, |this, w, cx| {
        assert_eq!(
            this.open.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            f.ids
        );
        assert_eq!(this.session.active, Some(f.ids[1]));
        let editor = this.active_editor().unwrap();
        assert_eq!(editor.read(cx).value(), "수정한 메모");
        assert!(editor.read(cx).focus_handle(cx).is_focused(w));
    });
    f.press(cx, "ctrl-shift-t");
    f.update(cx, |this, _, _| assert_eq!(this.open.len(), 3));
}

#[gpui::test]
fn reopen_shortcut_works_after_the_last_tab_closes(cx: &mut TestAppContext) {
    for sidebar_visible in [true, false] {
        let f = Fixture::new(cx);
        f.update(cx, |this, w, cx| {
            this.session.sidebar_visible = sidebar_visible;
            this.focus_editor(w, cx);
        });
        f.press(cx, "ctrl-w");
        f.press(cx, "ctrl-f4");
        f.update(cx, |this, _, _| {
            assert!(this.open.is_empty());
            assert_eq!(this.session.active, None);
        });
        f.press(cx, "ctrl-f4");
        f.press(cx, "ctrl-shift-t");
        f.update(cx, |this, _, _| {
            assert_eq!(this.session.active, Some(f.ids[1]))
        });
        f.press(cx, "ctrl-shift-t");
        f.update(cx, |this, _, _| {
            assert_eq!(
                this.open.iter().map(|tab| tab.id).collect::<Vec<_>>(),
                f.ids[..2]
            );
            assert_eq!(this.session.active, Some(f.ids[0]));
            assert!(this.closed_tabs.is_empty());
        });
    }
}

#[gpui::test]
fn reopen_skips_manually_opened_and_deleted_notes_and_retries_read_errors(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, w, cx| {
        this.open_note(f.ids[2], true, w, cx);
        this.close_tabs(f.ids[0], crate::tabs::CloseMode::Current, w, cx);
        this.close_tabs(f.ids[1], crate::tabs::CloseMode::Current, w, cx);
        this.open_note(f.ids[1], true, w, cx);
        assert_eq!(this.closed_tabs.len(), 1);
    });
    f.press(cx, "ctrl-shift-t");
    f.update(cx, |this, w, cx| {
        assert_eq!(this.session.active, Some(f.ids[0]));
        this.close_tabs(f.ids[1], crate::tabs::CloseMode::Current, w, cx);
        assert!(this.delete_confirmed(&[f.ids[1]], w, cx));
        assert!(this.closed_tabs.is_empty());
    });
    f.press(cx, "ctrl-shift-t");
    f.update(cx, |this, w, cx| {
        assert_eq!(this.open.len(), 2);
        this.close_tabs(f.ids[2], crate::tabs::CloseMode::Current, w, cx);
        let path = this.book.file(&f.dir, f.ids[2]);
        fs::rename(&path, path.with_extension("unavailable")).unwrap();
    });
    f.press(cx, "ctrl-shift-t");
    f.update(cx, |this, _, _| {
        assert!(this.error.is_some());
        assert_eq!(this.closed_tabs.len(), 1);
        assert_eq!(this.open.len(), 1);
        let path = this.book.file(&f.dir, f.ids[2]);
        fs::rename(path.with_extension("unavailable"), &path).unwrap();
    });
    f.press(cx, "escape");
    f.press(cx, "ctrl-shift-t");
    f.update(cx, |this, _, _| {
        assert_eq!(this.session.active, Some(f.ids[2]));
        assert!(this.closed_tabs.is_empty());
    });
}
