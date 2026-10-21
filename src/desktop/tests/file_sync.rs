use super::*;
use crate::watch::Changes;
use std::collections::HashSet;

fn note(id: NoteId) -> Changes {
    Changes {
        notes: HashSet::from([id]),
        ..Default::default()
    }
}

fn sync(f: &Fixture, changes: Changes, cx: &mut TestAppContext) -> Changes {
    let snapshot = f.update(cx, |this, _, cx| this.sync_snapshot(changes, cx));
    let read = snapshot.read();
    f.update(cx, |this, window, cx| {
        this.apply_file_sync(read, window, cx)
    })
}

#[gpui::test]
fn clean_external_reload_preserves_editor_selection_scroll_and_undo(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let original = format!("🙂 한글12\r\n한글3\rkeep\n{}", "more text\n".repeat(100));
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| state.replace_all(&original, window, cx));
        this.changed(f.ids[0], window, cx);
        assert!(this.save_all(cx));
        editor
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| {
            state.set_selected_range(5..11, cx);
        });
    });
    visual.update(|window, cx| window.render_frame(cx)); // Finish caret reveal before scrolling.
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| {
            state.set_scroll_offset(point(px(0.), px(-200.)), cx)
        });
    });
    visual.update(|window, cx| window.render_frame(cx));
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).scroll_offset(), point(px(0.), px(-200.)))
    });
    let external = format!("🙂 외부12\r\n본문3\rkeep\n{}", "more text\n".repeat(100));
    fs::write(f.dir.join(format!("notes/{}.txt", f.ids[0])), &external).unwrap();
    f.update(cx, |this, window, cx| {
        this.request_file_sync(note(f.ids[0]), window, cx);
    });
    cx.run_until_parked(); // Do not advance time: notifications must not wait for a timer.
    f.update(cx, |this, _, cx| {
        assert_eq!(this.active_editor().unwrap(), editor);
        assert_eq!(editor.read(cx).value(), external);
        assert_eq!(editor.read(cx).selected_range(), 5..11);
        assert_eq!(editor.read(cx).scroll_offset(), point(px(0.), px(-200.)));
        assert!(!this.open[0].dirty);
        assert_eq!(this.open[0].saved, external);
        assert!(this.error.is_none());
    });
    f.press(cx, "ctrl-z");
    f.update(cx, |this, _, cx| {
        assert_eq!(editor.read(cx).value(), original);
        assert!(this.open[0].dirty);
    });
}

#[gpui::test]
fn dirty_app_content_wins_and_stale_reads_cannot_undo_a_local_save(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let path = f.dir.join(format!("notes/{}.txt", f.ids[0]));
    f.update(cx, |this, window, cx| {
        this.active_editor()
            .unwrap()
            .update(cx, |state, cx| state.replace_all("app draft", window, cx));
        this.changed(f.ids[0], window, cx);
    });
    fs::write(&path, "external version").unwrap();
    assert!(sync(&f, note(f.ids[0]), cx).is_empty());
    f.update(cx, |this, _, cx| {
        assert_eq!(this.active_editor().unwrap().read(cx).value(), "app draft");
        assert!(this.open[0].dirty);
        assert!(this.error.is_none());
        assert!(this.save_all(cx));
    });
    assert_eq!(fs::read_to_string(&path).unwrap(), "app draft");
    fs::write(&path, "another external version").unwrap();
    let snapshot = f.update(cx, |this, _, cx| this.sync_snapshot(note(f.ids[0]), cx));
    let stale = snapshot.read();
    f.update(cx, |this, window, cx| {
        this.active_editor().unwrap().update(cx, |state, cx| {
            state.replace_all("new app content", window, cx)
        });
        this.changed(f.ids[0], window, cx);
        assert!(this.save_all(cx));
        let retry = this.apply_file_sync(stale, window, cx);
        assert!(retry.notes.contains(&f.ids[0]));
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "new app content"
        );
    });
    assert!(sync(&f, note(f.ids[0]), cx).is_empty());
    assert_eq!(fs::read_to_string(&path).unwrap(), "new app content");
}

#[gpui::test]
fn external_tree_updates_keep_tabs_selection_and_expansion_and_reject_partial_indices(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, _, _| {
        this.collapsed.insert(f.folder);
        this.active_editor().unwrap()
    });
    let mut book = Notebook::reload(&f.dir).unwrap();
    book.get_mut(f.ids[0]).unwrap().title = "external title".into();
    book.get_mut(f.ids[1]).unwrap().parent = f.folder;
    let added = book.add_folder(0, "added".into()).unwrap();
    book.add(added, "new child".into()).unwrap();
    book.save(&f.dir).unwrap();
    f.update(cx, |this, window, cx| {
        this.request_file_sync(Changes::all(), window, cx);
    });
    cx.run_until_parked();
    f.update(cx, |this, _, _| {
        assert_eq!(this.book.get(f.ids[0]).unwrap().title, "external title");
        assert_eq!(this.book.get(f.ids[1]).unwrap().parent, f.folder);
        assert_eq!(this.active_editor().unwrap(), editor);
        assert_eq!(this.session.active, Some(f.ids[0]));
        assert!(this.selection.ids.contains(&f.ids[0]));
        assert!(this.collapsed.contains(&f.folder));
        assert!(this.collapsed.contains(&added));
        assert_eq!(
            this.open.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            vec![f.ids[0], f.ids[1]]
        );
    });
    fs::write(f.dir.join(".edot/tree.tsv"), "EDOT5\nAAAAAAAAAAE\t").unwrap();
    f.update(cx, |this, window, cx| {
        this.request_file_sync(Changes::all(), window, cx);
    });
    cx.run_until_parked();
    f.update(cx, |this, _, _| {
        assert_eq!(this.book.get(f.ids[0]).unwrap().title, "external title");
        assert!(this.error.is_none());
    });
    fs::remove_file(f.dir.join(".edot/tree.tsv")).unwrap();
    assert!(sync(&f, Changes::all(), cx).tree);
    book.get_mut(f.ids[0]).unwrap().title = "completed external title".into();
    book.save(&f.dir).unwrap();
    f.update(cx, |this, window, cx| {
        this.request_file_sync(Changes::all(), window, cx);
    });
    cx.run_until_parked(); // A new notification also interrupts a failed read's backoff.
    f.update(cx, |this, _, _| {
        assert_eq!(
            this.book.get(f.ids[0]).unwrap().title,
            "completed external title"
        );
    });
    // An older read must not overwrite a tree change made in this app meanwhile.
    book.get_mut(f.ids[0]).unwrap().title = "stale external title".into();
    book.save(&f.dir).unwrap();
    let snapshot = f.update(cx, |this, _, cx| this.sync_snapshot(Changes::all(), cx));
    let stale = snapshot.read();
    f.update(cx, |this, window, cx| {
        this.book.get_mut(f.ids[0]).unwrap().title = "local title".into();
        this.save_tree().unwrap();
        assert!(this.apply_file_sync(stale, window, cx).tree);
        assert_eq!(this.book.get(f.ids[0]).unwrap().title, "local title");
    });
}

#[gpui::test]
fn external_tree_deletion_removes_clean_tabs_but_keeps_dirty_app_notes(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.active_editor().unwrap().update(cx, |state, cx| {
            state.replace_all("app must survive", window, cx)
        });
        this.changed(f.ids[0], window, cx);
    });
    fs::write(f.dir.join(".edot/tree.tsv"), "EDOT5\n").unwrap();
    assert!(sync(&f, Changes::all(), cx).is_empty());
    f.update(cx, |this, _, cx| {
        assert_eq!(this.book.notes.len(), 1);
        assert_eq!(this.book.notes[0].id, f.ids[0]);
        assert_eq!(this.book.notes[0].parent, 0);
        assert_eq!(this.open.len(), 1);
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "app must survive"
        );
        assert!(this.save_all(cx));
    });
    let disk = Notebook::reload(&f.dir).unwrap();
    assert_eq!(disk.notes.len(), 1); // Leftover bodies do not resurrect clean deletions.
    assert_eq!(disk.notes[0].id, f.ids[0]);
}

#[gpui::test]
fn missing_and_invalid_bodies_keep_editor_and_only_explicit_save_restores_deletions(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let path = f.dir.join(format!("notes/{}.txt", f.ids[0]));
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert!(sync(&f, note(f.ids[0]), cx).notes.contains(&f.ids[0]));
    f.update(cx, |this, _, cx| {
        assert_eq!(this.active_editor().unwrap().read(cx).value(), original)
    });
    fs::remove_file(&path).unwrap();
    assert!(sync(&f, note(f.ids[0]), cx).notes.contains(&f.ids[0]));
    f.update(cx, |this, _, cx| {
        assert!(this.file_sync.missing.contains(&f.ids[0]));
        assert!(this.save_all(cx)); // A clean tab has nothing to autosave.
        assert!(!path.exists());
        assert!(this.save_all_explicit(cx));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    });
    fs::remove_file(&path).unwrap();
    f.update(cx, |this, window, cx| {
        this.active_editor().unwrap().update(cx, |state, cx| {
            state.replace_all("edited after deletion", window, cx)
        });
        this.changed(f.ids[0], window, cx);
        assert!(!this.save_all(cx));
        assert!(!path.exists());
        assert!(this.save_all_explicit(cx));
    });
    assert_eq!(fs::read_to_string(&path).unwrap(), "edited after deletion");
}

#[gpui::test]
fn session_changes_are_ignored_and_old_space_reads_are_discarded(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let snapshot = f.update(cx, |this, _, cx| this.sync_snapshot(Changes::all(), cx));
    let stale = snapshot.read();
    Session::default().save(&f.dir).unwrap();
    assert!(sync(&f, Changes::all(), cx).is_empty());
    f.update(cx, |this, window, cx| {
        assert_eq!(this.session.active, Some(f.ids[0]));
        assert_eq!(this.open.len(), 2);
        assert!(this.switch_space(f.dir.join("other"), window, cx));
        assert!(this.apply_file_sync(stale, window, cx).is_empty());
        assert!(this.book.notes.is_empty());
        assert!(this.open.is_empty());
    });
}

#[gpui::test]
fn ime_and_title_edits_survive_external_changes_without_losing_other_tree_updates(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, _, _| this.active_editor().unwrap());
    cx.update_window(f.window, |_, window, cx| {
        let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
        handler.replace_and_mark_text_in_range(None, "입력", Some(2..2), window, cx);
    })
    .unwrap();
    let composing = f.update(cx, |_, _, cx| editor.read(cx).value());
    fs::write(
        f.dir.join(format!("notes/{}.txt", f.ids[0])),
        "external body",
    )
    .unwrap();
    assert!(sync(&f, note(f.ids[0]), cx).notes.contains(&f.ids[0]));
    f.update(cx, |_, _, cx| {
        assert!(editor.read(cx).is_composing());
        assert_eq!(editor.read(cx).value(), composing);
    });
    cx.update_window(f.window, |_, window, cx| {
        let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
        handler.unmark_text(window, cx);
    })
    .unwrap();
    assert!(sync(&f, note(f.ids[0]), cx).is_empty());
    f.update(cx, |this, window, cx| {
        assert_eq!(editor.read(cx).value(), composing);
        assert!(this.save_all(cx));
        this.begin_rename(f.ids[0], window, cx);
        this.title_input
            .update(cx, |state, cx| state.set_value("app title", window, cx));
    });
    let mut book = Notebook::reload(&f.dir).unwrap();
    book.get_mut(f.ids[0]).unwrap().title = "external title".into();
    let added = book.add(0, "external added".into()).unwrap();
    book.save(&f.dir).unwrap();
    assert!(sync(&f, Changes::all(), cx).is_empty());
    f.update(cx, |this, window, cx| {
        assert!(this.book.get(added).is_some());
        assert!(this.rename.is_some());
        assert_eq!(this.title_input.read(cx).value(), "app title");
        assert!(this.finish_rename(true, false, window, cx));
    });
    let book = Notebook::reload(&f.dir).unwrap();
    assert_eq!(book.get(f.ids[0]).unwrap().title, "app title");
    assert!(book.get(added).is_some());
}
