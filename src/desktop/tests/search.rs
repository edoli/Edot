use super::*;

#[gpui::test]
fn global_replace_updates_open_and_closed_notes_and_preserves_undo(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        this.open_note(f.ids[1], true, window, cx);
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.replace_all("loaded needle", window, cx));
        this.changed(f.ids[1], window, cx);
        this.global
            .query
            .update(cx, |s, cx| s.set_value("needle", window, cx));
        this.global
            .replacement
            .update(cx, |s, cx| s.set_value("바뀜", window, cx));
        this.replace_global_all(window, cx);
        editor
    });
    f.press(cx, "tab");
    f.press(cx, "enter");
    f.update(cx, |this, window, cx| {
        assert_eq!(editor.read(cx).value(), "loaded 바뀜");
        assert!(
            !this
                .open
                .iter()
                .find(|tab| tab.id == f.ids[1])
                .unwrap()
                .dirty
        );
        assert!(!this.open.iter().any(|tab| tab.id == f.ids[2]));
        this.focus_editor(window, cx);
    });
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[1]))).unwrap(),
        "loaded 바뀜"
    );
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[2]))).unwrap(),
        "child 바뀜\n"
    );
    f.press(cx, "ctrl-z");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "loaded needle")
    });
}
