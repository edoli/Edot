use super::*;

#[gpui::test]
fn reading_task_clicks_autosave_and_undo_without_changing_reading_mode(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let source = "- [ ] 저장할 한글🙂 항목\r\n";
    f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.replace_all(source, window, cx));
        this.changed(f.ids[0], window, cx);
        assert!(this.save_all(cx));
        assert!(this.set_read_only(true, window, cx));
        this.focus_editor(window, cx);
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let checkbox = visual.debug_bounds("reading-task-0").unwrap().center();
    visual.simulate_click(checkbox, Default::default());
    let checked = source.replace("[ ]", "[x]");
    f.update(cx, |this, _, cx| {
        assert_eq!(this.active_editor().unwrap().read(cx).value(), checked);
        assert!(this.open[0].dirty);
        assert!(cx.global::<AppState>().settings.read_only);
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(700));
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[0]))).unwrap(),
        checked
    );
    f.press(cx, "ctrl-z");
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(700));
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[0]))).unwrap(),
        source
    );
    f.update(cx, |this, _, cx| {
        assert!(cx.global::<AppState>().settings.read_only);
        assert!(!this.open[0].dirty);
        assert_eq!(
            fs::read_to_string(this.book.file(&this.dir, f.ids[1])).unwrap(),
            "closed needle\n"
        );
    });
}

fn add_note(f: &Fixture, cx: &mut TestAppContext, title: &str) -> NoteId {
    f.update(cx, |this, _, _| {
        let id = this.book.add(0, title.into()).unwrap();
        fs::write(this.book.file(&this.dir, id), format!("{title}\r\n본문🙂")).unwrap();
        this.book.save(&this.dir).unwrap();
        id
    })
}

#[gpui::test]
fn tree_previews_replace_in_place_without_closing_existing_tabs(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let next = add_note(&f, cx, "다음 노트");
    let originals =
        [f.ids[2], next].map(|id| fs::read(f.dir.join(format!("notes/{id}.txt"))).unwrap());
    f.update(cx, |this, window, cx| {
        this.tree_click(f.ids[2], Default::default(), window, cx);
        assert_eq!(
            this.open.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            f.ids
        );
        assert!(this.open[2].preview);
        this.tree_click(next, Default::default(), window, cx);
        assert_eq!(
            this.open.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            [f.ids[0], f.ids[1], next]
        );
        assert!(this.open[2].preview);
        this.tree_click(next, Default::default(), window, cx);
        assert_eq!(this.open.len(), 3);
        assert!(this.open[2].preview);
        assert!(this.closed_tabs.is_empty());
        this.tree_click(f.ids[0], Default::default(), window, cx);
        assert_eq!(
            this.open.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            [f.ids[0], f.ids[1]]
        );
        assert!(!this.open.iter().any(|tab| tab.preview));
        assert_eq!(this.session.active, Some(f.ids[0]));
        this.save_session().unwrap();
        assert_eq!(
            Session::load(&this.dir, &this.book).tabs,
            [f.ids[0], f.ids[1]]
        );
        assert!(this.closed_tabs.is_empty());
    });
    for (id, original) in [f.ids[2], next].into_iter().zip(originals) {
        assert_eq!(
            fs::read(f.dir.join(format!("notes/{id}.txt"))).unwrap(),
            original
        );
    }
}

#[gpui::test]
fn focusing_preview_body_with_mouse_enter_or_reading_keeps_the_tab(cx: &mut TestAppContext) {
    use gpui::{Focusable, MouseButton};
    let f = Fixture::new(cx);
    let keyboard_note = add_note(&f, cx, "키보드 포커스");
    let reading_note = add_note(&f, cx, "읽기 포커스");
    let after = add_note(&f, cx, "이후 미리보기");
    f.update(cx, |this, window, cx| {
        window.activate_window();
        this.tree_click(f.ids[2], Default::default(), window, cx);
        assert!(this.open.last().unwrap().preview);
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let body = visual.debug_bounds("body-editor").unwrap().center();
    visual.simulate_mouse_down(body, MouseButton::Left, Default::default());
    visual.simulate_mouse_up(body, MouseButton::Left, Default::default());
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, window, cx| {
        assert!(
            this.active_editor()
                .unwrap()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        );
        assert!(!this.open.last().unwrap().preview);
        this.tree_click(keyboard_note, Default::default(), window, cx);
        assert!(this.open.iter().any(|tab| tab.id == f.ids[2]));
        assert!(this.open.last().unwrap().preview);
    });
    f.press(cx, "enter");
    f.update(cx, |this, window, cx| {
        assert!(!this.open.last().unwrap().preview);
        this.tree_click(reading_note, Default::default(), window, cx);
        assert!(this.open.iter().any(|tab| tab.id == keyboard_note));
        assert!(this.open.last().unwrap().preview);
        assert!(this.set_read_only(true, window, cx));
    });
    visual.update(|w, cx| w.render_frame(cx));
    let reading = visual.debug_bounds("markdown-reading").unwrap().center();
    visual.simulate_mouse_down(reading, MouseButton::Left, Default::default());
    visual.simulate_mouse_up(reading, MouseButton::Left, Default::default());
    visual.update(|w, cx| w.render_frame(cx));
    f.update(cx, |this, window, cx| {
        assert!(
            this.active_editor()
                .unwrap()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        );
        assert!(!this.open.last().unwrap().preview);
        this.tree_click(after, Default::default(), window, cx);
        assert_eq!(this.open.len(), 6);
        assert!(this.open.last().unwrap().preview);
        assert!(this.open.iter().filter(|tab| tab.preview).count() == 1);
    });
}

#[gpui::test]
fn failed_preview_reads_and_unfocused_edits_never_discard_the_old_body(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let next = add_note(&f, cx, "교체 대상");
    let path = f.dir.join(format!("notes/{next}.txt"));
    let next_body = fs::read(&path).unwrap();
    fs::write(&path, [0xff]).unwrap();
    f.update(cx, |this, window, cx| {
        this.tree_click(f.ids[2], Default::default(), window, cx);
        let old = this.active_editor().unwrap();
        assert!(!this.open_tree_note(next, window, cx));
        assert_eq!(this.session.active, Some(f.ids[2]));
        assert!(this.open.last().unwrap().preview);
        assert_eq!(this.active_editor().unwrap(), old);
        this.error = None;
        fs::write(&path, &next_body).unwrap();
        // Emulate a change before its notification has reached the owning view.
        old.update(cx, |s, cx| {
            s.replace_all("초점 없이 수정한 본문", window, cx)
        });
        assert!(this.open_tree_note(next, window, cx));
        assert_eq!(this.open.len(), 4);
        assert!(
            !this
                .open
                .iter()
                .find(|tab| tab.id == f.ids[2])
                .unwrap()
                .preview
        );
        assert_eq!(old.read(cx).value(), "초점 없이 수정한 본문");
        this.changed(f.ids[2], window, cx);
        let blocker = this.book.file(&this.dir, f.ids[2]).with_extension("tmp");
        fs::create_dir(&blocker).unwrap();
        assert!(!this.save_all(cx));
        assert_eq!(old.read(cx).value(), "초점 없이 수정한 본문");
        assert!(this.open.iter().any(|tab| tab.id == f.ids[2] && tab.dirty));
        fs::remove_dir(blocker).unwrap();
        this.error = None;
        assert!(this.save_all(cx));
    });
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[2]))).unwrap(),
        "초점 없이 수정한 본문"
    );
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
