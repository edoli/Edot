use super::*;

#[gpui::test]
fn update_close_saves_dirty_notes_and_session(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    fixture.update(cx, |this, window, cx| {
        let editor = this.open[0].editor.clone().unwrap();
        editor.update(cx, |state, cx| {
            state.replace_all("업데이트 전 보존할 메모", window, cx)
        });
        this.changed(fixture.ids[0], window, cx);
        assert!(this.prepare_update_close(window, cx));
        assert_eq!(
            fs::read_to_string(this.book.file(&this.dir, fixture.ids[0])).unwrap(),
            "업데이트 전 보존할 메모"
        );
        assert!(this.dir.join("session.bin").is_file());
    });
}

#[gpui::test]
fn update_close_refuses_failed_save_and_keeps_edits(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    fixture.update(cx, |this, window, cx| {
        let editor = this.open[0].editor.clone().unwrap();
        editor.update(cx, |state, cx| {
            state.replace_all("저장 실패 시 보존", window, cx)
        });
        this.changed(fixture.ids[0], window, cx);
        let file = this.book.file(&this.dir, fixture.ids[0]);
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        assert!(!this.prepare_update_close(window, cx));
        assert!(this.error.is_some());
        assert_eq!(editor.read(cx).value(), "저장 실패 시 보존");
    });
}

#[gpui::test]
fn update_close_refuses_failed_startup(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    fixture.update(cx, |this, window, cx| {
        this.startup_failed = true;
        assert!(!this.prepare_update_close(window, cx));
    });
}
