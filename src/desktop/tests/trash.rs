use super::*;
use crate::widgets::WindowExt;

#[gpui::test]
fn permanent_delete_requires_confirmation_and_keeps_live_notes(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        assert!(this.delete_confirmed(&[f.folder], window, cx));
        cx.global_mut::<AppState>().settings.confirm_delete = false;
        this.show_trash(window, cx);
    });
    cx.run_until_parked();
    let tree = fs::read(f.dir.join("tree.tsv")).unwrap();
    let live = fs::read(f.dir.join(format!("notes/{}.txt", f.ids[1]))).unwrap();
    f.update(cx, |this, window, cx| {
        this.trash_browser
            .clone()
            .unwrap()
            .update(cx, |browser, cx| {
                let index = browser
                    .entries
                    .iter()
                    .position(|e| e.note.id == f.folder)
                    .unwrap();
                browser.select(index, window, cx);
            });
    });
    cx.run_until_parked();
    f.press(cx, "tab");
    f.press(cx, "delete");
    f.update(cx, |_, w, cx| assert!(!w.has_active_dialog(cx)));
    f.press(cx, "tab");
    f.press(cx, "delete");
    f.update(cx, |_, w, cx| assert!(w.has_active_dialog(cx)));
    f.press(cx, "enter"); // Cancel is selected by default, even with confirm_delete=false.
    assert_eq!(crate::trash::list(&f.dir).unwrap().len(), 2);
    f.update(cx, |_, w, cx| assert!(!w.has_active_dialog(cx)));
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let delete = visual.debug_bounds("trash-delete").unwrap().center();
    visual.simulate_click(delete, Default::default());
    f.press(cx, "escape");
    assert_eq!(crate::trash::list(&f.dir).unwrap().len(), 2);
    visual.update(|w, cx| w.render_frame(cx));
    visual.simulate_click(delete, Default::default());
    f.press(cx, "tab");
    f.press(cx, "enter");
    cx.run_until_parked();
    assert!(crate::trash::deletion_keys(&f.dir).unwrap().is_empty());
    assert_eq!(fs::read(f.dir.join("tree.tsv")).unwrap(), tree);
    assert_eq!(
        fs::read(f.dir.join(format!("notes/{}.txt", f.ids[1]))).unwrap(),
        live
    );
    f.update(cx, |this, _, cx| {
        assert_eq!(this.book.notes.len(), 2);
        let browser = this.trash_browser.as_ref().unwrap().read(cx);
        assert!(browser.entries.is_empty());
        assert!(browser.preview.read(cx).value().is_empty());
    });
    f.press(cx, "escape");
    f.update(cx, |this, _, _| assert!(this.trash_browser.is_none()));
}

#[gpui::test]
fn empty_bin_confirms_a_snapshot_and_preserves_unrecognized_files(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        assert!(this.delete_confirmed(&[f.ids[1]], window, cx));
        this.show_trash(window, cx);
    });
    cx.run_until_parked();
    let index = fs::read(f.dir.join("tree.tsv")).unwrap();
    fs::write(f.dir.join("trash/unknown.txt"), "preserve").unwrap();
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let empty = visual.debug_bounds("trash-empty").unwrap().center();
    visual.simulate_click(empty, Default::default());
    f.press(cx, "enter");
    assert_eq!(crate::trash::list(&f.dir).unwrap().len(), 1);
    f.press(cx, "ctrl-shift-delete");
    f.update(cx, |_, w, cx| assert!(w.has_active_dialog(cx)));
    fs::write(f.dir.join("trash/1-42.txt"), "new after confirmation").unwrap();
    fs::write(
        f.dir.join("trash/1-42.meta"),
        "EDOTTRASH2\n1\t0\t0\tnote\tnew archive\n",
    )
    .unwrap();
    f.press(cx, "tab");
    f.press(cx, "enter");
    cx.run_until_parked();
    assert_eq!(crate::trash::list(&f.dir).unwrap().len(), 1);
    assert_eq!(
        fs::read_to_string(f.dir.join("trash/1-42.txt")).unwrap(),
        "new after confirmation"
    );
    visual.update(|w, cx| w.render_frame(cx));
    let empty = visual.debug_bounds("trash-empty").unwrap().center();
    visual.simulate_click(empty, Default::default());
    f.press(cx, "tab");
    f.press(cx, "enter");
    cx.run_until_parked();
    assert!(crate::trash::list(&f.dir).unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(f.dir.join("trash/unknown.txt")).unwrap(),
        "preserve"
    );
    assert_eq!(fs::read(f.dir.join("tree.tsv")).unwrap(), index);
    f.press(cx, "ctrl-shift-delete");
    f.update(cx, |_, w, cx| assert!(!w.has_active_dialog(cx)));
    f.press(cx, "escape");
    f.update(cx, |this, _, _| assert!(this.trash_browser.is_none()));
}

#[gpui::test]
fn recycle_bin_preview_restore_and_keyboard_preserve_the_existing_note(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let original = fs::read(f.dir.join("notes").join(format!("{}.txt", f.ids[2]))).unwrap();
    f.update(cx, |this, window, cx| {
        assert!(this.delete_confirmed(&[f.folder], window, cx));
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|window, cx| window.render_frame(cx));
    let open_button = visual.debug_bounds("trash-open").unwrap().center();
    visual.simulate_click(open_button, Default::default());
    cx.run_until_parked();
    f.update(cx, |this, window, cx| {
        let browser = this.trash_browser.clone().unwrap();
        browser.update(cx, |browser, cx| {
            assert_eq!(browser.entries.len(), 2);
            let root = browser
                .entries
                .iter()
                .position(|e| e.note.id == f.ids[2])
                .unwrap();
            browser.select(root, window, cx);
        });
    });
    cx.run_until_parked();
    f.update(cx, |this, _, cx| {
        let browser = this.trash_browser.as_ref().unwrap().read(cx);
        assert!(browser.preview.read(cx).is_read_only());
        assert_eq!(browser.preview.read(cx).value().as_bytes(), original);
    });
    f.press(cx, "ctrl-n");
    f.update(cx, |this, _, _| assert_eq!(this.book.notes.len(), 2));
    f.press(cx, "tab");
    f.press(cx, "x");
    f.press(cx, "ctrl-a");
    f.press(cx, "ctrl-c");
    assert_eq!(
        cx.update(|cx| cx
            .read_from_clipboard()
            .unwrap()
            .text()
            .unwrap()
            .into_bytes()),
        original
    );
    f.update(cx, |this, _, cx| {
        let browser = this.trash_browser.as_ref().unwrap().read(cx);
        assert_eq!(browser.preview.read(cx).value().as_bytes(), original);
    });
    f.update(cx, |this, window, cx| {
        this.trash_browser
            .clone()
            .unwrap()
            .update(cx, |browser, cx| {
                let folder = browser
                    .entries
                    .iter()
                    .position(|e| e.note.id == f.folder)
                    .unwrap();
                browser.select(folder, window, cx);
            });
    });
    cx.run_until_parked();
    f.press(cx, "ctrl-enter");
    cx.run_until_parked();
    f.update(cx, |this, _, cx| {
        assert_eq!(this.book.notes.len(), 4);
        assert_eq!(this.book.get(f.ids[2]).unwrap().parent, f.folder);
        assert!(
            this.trash_browser
                .as_ref()
                .unwrap()
                .read(cx)
                .entries
                .is_empty()
        );
        assert_eq!(
            fs::read(this.book.file(&this.dir, f.ids[2])).unwrap(),
            original
        );
        assert_eq!(
            fs::read_to_string(this.book.file(&this.dir, f.ids[1])).unwrap(),
            "closed needle\n"
        );
    });
    f.press(cx, "escape");
    f.update(cx, |this, _, _| assert!(this.trash_browser.is_none()));
}

#[gpui::test]
fn empty_bin_and_failed_restore_remain_safe_to_close(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| this.show_trash(window, cx));
    cx.run_until_parked();
    f.press(cx, "ctrl-enter");
    f.press(cx, "escape");
    f.update(cx, |this, window, cx| {
        assert_eq!(this.book.notes.len(), 4);
        assert!(this.trash_browser.is_none());
        assert!(this.delete_confirmed(&[f.ids[1]], window, cx));
        this.show_trash(window, cx);
    });
    cx.run_until_parked();
    fs::create_dir(f.dir.join("tree.tmp")).unwrap();
    f.press(cx, "ctrl-enter");
    f.update(cx, |this, _, cx| {
        assert!(this.book.get(f.ids[1]).is_none());
        assert_eq!(
            this.trash_browser.as_ref().unwrap().read(cx).entries.len(),
            1
        );
        assert!(!this.book.file(&this.dir, f.ids[1]).exists());
    });
    f.press(cx, "escape");
    f.update(cx, |this, _, _| assert!(this.trash_browser.is_none()));
}
