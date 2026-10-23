use super::*;

#[gpui::test]
fn macos_os_quit_flushes_pending_body_and_session(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    cx.update(|cx| crate::desktop::macos_menu(f.window, &f.view, cx));
    f.update(cx, |this, window, cx| {
        this.active_editor()
            .unwrap()
            .update(cx, |s, cx| s.replace_all("OS 종료 직전 메모🙂", window, cx));
        this.changed(f.ids[0], window, cx);
    });
    cx.update(|cx| cx.shutdown());
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[0]))).unwrap(),
        "OS 종료 직전 메모🙂"
    );
    assert!(f.dir.join(".edot/session.bin").is_file());
}

#[gpui::test]
fn macos_quit_action_uses_the_same_save_veto_as_window_close(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    cx.update(|cx| crate::desktop::macos_menu(f.window, &f.view, cx));
    let file = f.dir.join(format!("notes/{}.txt", f.ids[0]));
    let blocker = file.with_extension("tmp");
    f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| s.replace_all("종료 전 메모🙂", window, cx));
        this.changed(f.ids[0], window, cx);
        fs::create_dir(&blocker).unwrap();
    });
    cx.update(|cx| cx.dispatch_action(&crate::desktop::QuitEdot));
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);
    f.update(cx, |this, _, _| assert!(this.error.is_some()));
    fs::remove_dir(&blocker).unwrap();
    cx.update(|cx| cx.dispatch_action(&crate::desktop::QuitEdot));
    assert!(cx.update(|cx| cx.windows().is_empty()));
    assert_eq!(fs::read_to_string(file).unwrap(), "종료 전 메모🙂");
}

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
