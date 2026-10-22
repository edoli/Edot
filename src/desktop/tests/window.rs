use super::*;

#[gpui::test]
fn client_close_preserves_failed_note_then_saves_before_removing_window(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    let file = fixture.dir.join(format!("notes/{}.txt", fixture.ids[0]));
    let blocker = file.with_extension("tmp");
    fixture.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| {
            state.replace_all("닫기 전에 보존할 한글🙂", window, cx)
        });
        this.changed(fixture.ids[0], window, cx);
        fs::create_dir(&blocker).unwrap();
        this.close_window(window, cx);
        assert!(this.error.is_some());
        assert!(this.open[0].dirty);
        assert_eq!(editor.read(cx).value(), "닫기 전에 보존할 한글🙂");
    });
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);
    fs::remove_dir(&blocker).unwrap();
    fixture.update(cx, |this, window, cx| this.close_window(window, cx));
    assert_eq!(fs::read_to_string(file).unwrap(), "닫기 전에 보존할 한글🙂");
    assert!(fixture.dir.join(".edot/session.bin").is_file());
    assert!(cx.update(|cx| cx.windows().is_empty()));
}

#[gpui::test]
fn client_close_keeps_window_when_session_cannot_be_saved(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    let session = fixture.dir.join(".edot/session.bin");
    if session.exists() {
        fs::remove_file(&session).unwrap();
    }
    fs::create_dir(&session).unwrap();
    fixture.update(cx, |this, window, cx| {
        this.close_window(window, cx);
        assert!(this.error.is_some());
    });
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);
    fs::remove_dir(&session).unwrap();
    fixture.update(cx, |this, window, cx| this.close_window(window, cx));
    assert!(session.is_file());
    assert!(cx.update(|cx| cx.windows().is_empty()));
}
