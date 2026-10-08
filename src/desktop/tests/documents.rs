use super::*;

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
