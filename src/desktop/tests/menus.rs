use super::*;

#[gpui::test]
fn delete_confirmation_defaults_to_cancel_and_keyboard_menu_dismisses(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, window, cx| {
        this.selection.single(f.ids[0]);
        this.caret = Some(f.ids[0]);
        this.tree_focus.focus(window, cx);
        this.confirm_delete(window, cx);
    });
    f.press(cx, "enter");
    f.update(cx, |this, _, _| assert!(this.book.get(f.ids[0]).is_some()));
    f.press(cx, "escape");
    f.update(cx, |this, window, cx| this.tree_focus.focus(window, cx));
    f.press(cx, "shift-f10");
    f.update(cx, |this, _, _| assert!(this.keyboard_menu.is_some()));
    f.press(cx, "escape");
    f.update(cx, |this, _, _| assert!(this.keyboard_menu.is_none()));
}

#[gpui::test]
fn context_menu_shortcuts_align_in_editor_and_tree(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    f.update(cx, |this, w, cx| this.focus_editor(w, cx));
    f.press(cx, "shift-f10");
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let undo = visual.debug_bounds("menu-shortcut-Ctrl+Z").unwrap();
    for selector in [
        "menu-shortcut-Ctrl+Y",
        "menu-shortcut-Ctrl+X",
        "menu-shortcut-Ctrl+C",
        "menu-shortcut-Ctrl+V",
        "menu-shortcut-Ctrl+A",
    ] {
        let shortcut = visual.debug_bounds(selector).unwrap();
        assert_eq!(shortcut.right(), undo.right());
    }
    f.press(cx, "escape");
    f.update(cx, |this, w, cx| {
        this.caret = Some(f.ids[0]);
        this.selection.single(f.ids[0]);
        this.tree_focus.focus(w, cx);
    });
    f.press(cx, "shift-f10");
    visual.update(|w, cx| w.render_frame(cx));
    assert_eq!(
        visual.debug_bounds("menu-shortcut-F2").unwrap().right(),
        visual.debug_bounds("menu-shortcut-Delete").unwrap().right()
    );
    f.press(cx, "escape");
}

#[gpui::test]
fn context_menu_copy_targets_editor_and_restores_focus(cx: &mut TestAppContext) {
    use gpui::Focusable;
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, w, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |s, cx| {
            s.set_value("copied line\r\nnext", w, cx);
            s.set_selected_range(0..6, cx);
        });
        this.tree_focus.focus(w, cx);
        editor
    });
    let mut visual = gpui::VisualTestContext::from_window(f.window, cx);
    visual.update(|w, cx| w.render_frame(cx));
    let body = visual.debug_bounds("body-editor").unwrap();
    visual.simulate_mouse_down(
        body.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    visual.simulate_mouse_up(
        body.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    visual.update(|w, cx| w.render_frame(cx));
    let copy = visual.debug_bounds("menu-shortcut-Ctrl+C").unwrap();
    visual.simulate_click(copy.center(), gpui::Modifiers::default());
    f.update(cx, |_, w, cx| {
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "copied");
        assert!(editor.read(cx).focus_handle(cx).is_focused(w));
        assert_eq!(editor.read(cx).value(), "copied line\r\nnext");
    });
}
