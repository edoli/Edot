use super::*;

#[gpui::test]
fn recycle_bin_preview_restore_and_keyboard_preserve_the_existing_note(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let original = fs::read(f.dir.join("notes").join(format!("{}.txt", f.ids[0]))).unwrap();
    f.update(cx, |this, window, cx| {
        assert!(this.delete_confirmed(&[f.ids[0]], window, cx));
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
                .position(|e| e.note.id == f.ids[0])
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
    f.update(cx, |this, _, _| assert_eq!(this.book.notes.len(), 1));
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
    f.press(cx, "ctrl-enter");
    cx.run_until_parked();
    f.update(cx, |this, _, cx| {
        assert_eq!(this.book.notes.len(), 3);
        assert_eq!(this.book.get(f.ids[2]).unwrap().parent, f.ids[0]);
        assert!(
            this.trash_browser
                .as_ref()
                .unwrap()
                .read(cx)
                .entries
                .is_empty()
        );
        assert_eq!(
            fs::read(this.book.file(&this.dir, f.ids[0])).unwrap(),
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
        assert_eq!(this.book.notes.len(), 3);
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
