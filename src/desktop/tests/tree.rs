use super::*;

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
    assert!(index.starts_with("EDOT2\n"));
    assert!(index.contains("\t한글 제목 새 줄 끝 😀\n"));
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[0]))).unwrap(),
        "🙂 한글12\r\n한글3\rkeep\n"
    );
}
