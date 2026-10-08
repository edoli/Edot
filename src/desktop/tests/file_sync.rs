use super::*;
use crate::watch::Changes;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

fn note(id: u64) -> Changes {
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
fn external_plain_title_changes_are_reflected_and_invalid_titles_are_rejected(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let path = f.dir.join("tree.tsv");
    let original = fs::read_to_string(&path).unwrap();
    assert!(original.starts_with("EDOT2\n"));
    let changes = Changes {
        tree: true,
        ..Default::default()
    };
    fs::write(&path, original.replace("첫 메모", "잘못된\t제목")).unwrap();
    assert!(sync(&f, changes.clone(), cx).tree);
    f.update(cx, |this, _, _| {
        assert_eq!(this.book.get(f.ids[0]).unwrap().title, "첫 메모");
    });
    let external = original.replace("첫 메모", "414243 한글 😀");
    fs::write(&path, &external).unwrap();
    assert!(sync(&f, changes, cx).is_empty());
    f.update(cx, |this, _, cx| {
        assert_eq!(this.book.get(f.ids[0]).unwrap().title, "414243 한글 😀");
        assert_eq!(this.session.active, Some(f.ids[0]));
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "🙂 한글12\r\n한글3\rkeep\n"
        );
    });
    assert_eq!(fs::read_to_string(&path).unwrap(), external);
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
    assert!(sync(&f, note(f.ids[0]), cx).is_empty());
    cx.run_until_parked();
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
fn closed_notes_read_latest_files_and_refresh_existing_search_results(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.search_mode = true;
        this.global
            .query
            .update(cx, |state, cx| state.set_value("needle", window, cx));
        this.queue_search(window, cx);
    });
    let settle = |cx: &mut TestAppContext| {
        cx.run_until_parked();
        cx.background_executor
            .advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
    };
    settle(cx);
    fs::write(
        f.dir.join(format!("notes/{}.txt", f.ids[1])),
        "external without match",
    )
    .unwrap();
    assert!(sync(&f, note(f.ids[1]), cx).is_empty());
    settle(cx);
    f.update(cx, |this, window, cx| {
        assert!(
            !this
                .search_results
                .groups
                .iter()
                .any(|row| row.id == f.ids[1])
        );
        assert!(
            this.open
                .iter()
                .find(|tab| tab.id == f.ids[1])
                .unwrap()
                .editor
                .is_none()
        );
        assert!(this.open_note(f.ids[1], false, window, cx));
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "external without match"
        );
    });
}

#[gpui::test]
fn external_tree_updates_keep_tabs_selection_and_expansion_and_reject_partial_indices(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, _, _| {
        this.collapsed.insert(f.ids[0]);
        this.active_editor().unwrap()
    });
    let mut book = Notebook::reload(&f.dir).unwrap();
    book.get_mut(f.ids[0]).unwrap().title = "external title".into();
    book.get_mut(f.ids[1]).unwrap().parent = f.ids[0];
    let added = book.add(0, "added".into());
    book.add(added, "new child".into());
    book.save(&f.dir).unwrap();
    assert!(sync(&f, Changes::all(), cx).is_empty());
    f.update(cx, |this, _, _| {
        assert_eq!(this.book.get(f.ids[0]).unwrap().title, "external title");
        assert_eq!(this.book.get(f.ids[1]).unwrap().parent, f.ids[0]);
        assert_eq!(this.active_editor().unwrap(), editor);
        assert_eq!(this.session.active, Some(f.ids[0]));
        assert!(this.selection.ids.contains(&f.ids[0]));
        assert!(this.collapsed.contains(&f.ids[0]));
        assert!(this.collapsed.contains(&added));
        assert_eq!(
            this.open.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            vec![f.ids[0], f.ids[1]]
        );
    });
    fs::write(f.dir.join("tree.tsv"), "EDOT2\n1\t").unwrap();
    assert!(sync(&f, Changes::all(), cx).tree);
    f.update(cx, |this, _, _| {
        assert_eq!(this.book.get(f.ids[0]).unwrap().title, "external title");
        assert!(this.error.is_none());
    });
    fs::remove_file(f.dir.join("tree.tsv")).unwrap();
    assert!(sync(&f, Changes::all(), cx).tree);
    book.save(&f.dir).unwrap();
    assert!(sync(&f, Changes::all(), cx).is_empty());
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
    fs::write(f.dir.join("tree.tsv"), "EDOT2\n").unwrap();
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
fn deleting_the_clean_active_note_moves_focus_to_the_surviving_editor(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let mut book = Notebook::reload(&f.dir).unwrap();
    book.notes.retain(|note| note.id == f.ids[1]);
    book.save(&f.dir).unwrap();
    assert!(sync(&f, Changes::all(), cx).is_empty());
    f.update(cx, |this, window, cx| {
        assert_eq!(this.session.active, Some(f.ids[1]));
        let editor = this.active_editor().unwrap();
        assert!(gpui::Focusable::focus_handle(editor.read(cx), cx).is_focused(window));
    });
    f.input(cx, "x");
    f.update(cx, |this, _, cx| {
        assert_eq!(
            this.active_editor().unwrap().read(cx).value(),
            "xclosed needle\n"
        );
    });
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
fn session_changes_are_ignored_and_old_vault_reads_are_discarded(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let snapshot = f.update(cx, |this, _, cx| this.sync_snapshot(Changes::all(), cx));
    let stale = snapshot.read();
    Session::default().save(&f.dir).unwrap();
    assert!(sync(&f, Changes::all(), cx).is_empty());
    f.update(cx, |this, window, cx| {
        assert_eq!(this.session.active, Some(f.ids[0]));
        assert_eq!(this.open.len(), 2);
        assert!(this.switch_vault(f.dir.join("other"), window, cx));
        assert!(this.apply_file_sync(stale, window, cx).is_empty());
        assert!(this.book.notes.is_empty());
        assert!(this.open.is_empty());
    });
}

#[gpui::test]
fn bulk_changes_restart_one_full_search_instead_of_many_note_tasks(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.search_mode = true;
        this.global
            .query
            .update(cx, |state, cx| state.set_value("needle", window, cx));
        this.queue_search(window, cx);
    });
    cx.run_until_parked();
    let generation = f.update(cx, |this, _, _| this.search_results.generation);
    assert!(
        sync(
            &f,
            Changes {
                notes: (1..=100).collect(),
                ..Default::default()
            },
            cx
        )
        .is_empty()
    );
    f.update(cx, |this, _, _| {
        assert_eq!(this.search_results.generation, generation + 1);
        assert!(this.search_results.scan_task.is_some());
        assert!(this.search_results.note_tasks.is_empty());
        assert!(this.search_results.pending_notes.is_empty());
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
    let added = book.add(0, "external added".into());
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

#[gpui::test]
fn startup_loads_global_ini_while_running_settings_stay_unchanged(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let expected = Settings {
        theme: ThemeMode::Dark,
        font_size: 25,
        wrap: false,
        ..Default::default()
    };
    let config = f.dir.join("test-global-config");
    expected.save(&config).unwrap();
    let loaded = LoadedVault::open(&crate::vault::Paths {
        vault: f.dir.join("startup-vault"),
        config,
    })
    .unwrap();
    assert_eq!(loaded.settings, expected);
    f.update(cx, |this, _, _| {
        assert_eq!(this.settings, Settings::default())
    });
}

#[gpui::test]
fn real_windows_events_reach_the_app_without_desktop_automation(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    // GPUI's deterministic scheduler rejects OS-thread wakes. Pump real events
    // on its test thread into the normal coalescing/background-read pipeline.
    let (watcher, events) = crate::watch::Watcher::new(&f.dir).unwrap();
    fs::write(
        f.dir.join(format!("notes/{}.txt", f.ids[0])),
        "native external body",
    )
    .unwrap();
    let wait = |cx: &mut TestAppContext, condition: &dyn Fn(&Edot, &Context<Edot>) -> bool| {
        let start = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(5));
            if let Some(changes) = events.take() {
                f.update(cx, |this, window, cx| {
                    this.request_file_sync(changes.unwrap(), window, cx)
                });
            }
            cx.run_until_parked();
            cx.background_executor
                .advance_clock(Duration::from_millis(250));
            cx.run_until_parked();
            if f.update(cx, |this, _, cx| condition(this, cx)) {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "native UI refresh timed out"
            );
        }
    };
    wait(cx, &|this, cx| {
        this.active_editor().unwrap().read(cx).value() == "native external body"
    });
    let mut book = Notebook::reload(&f.dir).unwrap();
    book.get_mut(f.ids[0]).unwrap().title = "native external title".into();
    book.save(&f.dir).unwrap();
    wait(cx, &|this, _| {
        this.book.get(f.ids[0]).unwrap().title == "native external title"
    });
    drop(watcher);
    f.update(cx, |this, window, cx| {
        assert!(this.switch_vault(f.dir.join("new-vault"), window, cx));
    });
    let (_watcher, events) = crate::watch::Watcher::new(&f.dir.join("new-vault")).unwrap();
    fs::write(f.dir.join("tree.tsv"), "bad old vault").unwrap();
    std::thread::sleep(Duration::from_millis(30));
    if let Some(changes) = events.take() {
        f.update(cx, |this, window, cx| {
            this.request_file_sync(changes.unwrap(), window, cx)
        });
    }
    cx.run_until_parked();
    cx.background_executor
        .advance_clock(Duration::from_millis(250));
    cx.run_until_parked();
    f.update(cx, |this, _, _| {
        assert!(this.book.notes.is_empty());
        assert!(this.error.is_none());
    });
}
