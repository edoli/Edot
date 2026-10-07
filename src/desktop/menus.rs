//! App menu composition shared by right-click and keyboard invocation.
use super::*;
use component::menu::PopupMenu;

pub(super) fn tree_menu(menu: PopupMenu, weak: WeakEntity<Edot>, id: u64, cx: &App) -> PopupMenu {
    let Some(view) = weak.upgrade() else {
        return menu;
    };
    view.read(cx).note_menu(menu, weak, id)
}

impl Edot {
    fn note_menu(&self, mut menu: PopupMenu, view: WeakEntity<Edot>, id: u64) -> PopupMenu {
        let multiple = self.selection.ids.len() > 1;
        let nested = self.book.get(id).is_some_and(|note| note.parent != 0);
        if !multiple {
            let weak = view.clone();
            menu = menu.item(PopupMenuItem::new("하위 메모 추가").on_click(
                move |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| this.create_note(Some(id), window, cx));
                },
            ));
            let weak = view.clone();
            menu = menu.item(PopupMenuItem::new("이름 바꾸기").shortcut("F2").on_click(
                move |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| this.begin_rename(id, window, cx));
                },
            ));
            let weak = view.clone();
            menu = menu.item(
                PopupMenuItem::new("한 단계 위로")
                    .disabled(!nested)
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |this, cx| this.outdent(id, cx));
                    }),
            );
        } else {
            let weak = view.clone();
            menu = menu.item(
                PopupMenuItem::new("선택한 항목과 함께 새 폴더 만들기").on_click(
                    move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| this.group_selected(window, cx));
                    },
                ),
            );
        }
        let weak = view.clone();
        menu = menu.separator().item(
            PopupMenuItem::new("삭제")
                .shortcut("Delete")
                .destructive()
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| this.confirm_delete(window, cx));
                }),
        );
        menu
    }
}

pub(super) fn tab_menu(
    menu: PopupMenu,
    weak: WeakEntity<Edot>,
    id: u64,
    index: usize,
    cx: &App,
) -> PopupMenu {
    let Some(view) = weak.upgrade() else {
        return menu;
    };
    let count = view.read(cx).open.len();
    let mut menu = menu;
    for (mode, label, shortcut, disabled) in [
        (CloseMode::Current, "탭 닫기", Some("Ctrl+W"), false),
        (CloseMode::Others, "다른 탭 닫기", None, count <= 1),
        (
            CloseMode::ToRight,
            "오른쪽 탭들 닫기",
            None,
            index + 1 >= count,
        ),
    ] {
        let weak = view.downgrade();
        let mut item = PopupMenuItem::new(label).disabled(disabled);
        if let Some(shortcut) = shortcut {
            item = item.shortcut(shortcut);
        }
        menu = menu.item(item.on_click(move |_, window, cx| {
            let _ = weak.update(cx, |this, cx| this.close_tabs(id, mode, window, cx));
        }));
    }
    menu
}

fn editing_menu(mut menu: PopupMenu, focus: FocusHandle, separate_history: bool) -> PopupMenu {
    menu = menu.action_context(focus);
    for (index, command) in component::input::EDIT_COMMANDS.iter().enumerate() {
        if index == 2 && separate_history {
            menu = menu.separator();
        }
        menu = menu.menu(command.label, command.shortcut, (command.action)());
    }
    menu
}

pub(super) fn editor_menu(
    menu: PopupMenu,
    editor: &Entity<TextareaState>,
    weak: WeakEntity<Edot>,
    cx: &App,
) -> PopupMenu {
    let mut menu = editing_menu(menu, editor.read(cx).focus_handle(cx), false).separator();
    for (label, shortcut, replace) in [("찾기", "Ctrl+F", false), ("바꾸기", "Ctrl+H", true)] {
        let weak = weak.clone();
        menu = menu.item(PopupMenuItem::new(label).shortcut(shortcut).on_click(
            move |_, window, cx| {
                let _ = weak.update(cx, |this, cx| this.show_find(false, replace, window, cx));
            },
        ));
    }
    menu
}

impl Edot {
    pub(super) fn show_keyboard_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use component::menu::PopupMenu;
        let weak = cx.entity().downgrade();
        let focus = window.focused(cx).unwrap_or_else(|| self.focus.clone());
        let tree = self.tree_focus.is_focused(window);
        let id = if tree {
            self.caret
        } else {
            self.session.active
        };
        let Some(id) = id else {
            return;
        };
        if tree && !self.selection.ids.contains(&id) {
            self.selection.single(id);
        }
        let menu = PopupMenu::build(window, cx, |menu, _, _| {
            if tree {
                self.note_menu(menu.action_context(focus), weak, id)
            } else {
                editing_menu(menu, focus, true)
            }
        });
        self.menu_subscription = Some(cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.keyboard_menu = None;
            cx.notify();
        }));
        self.menu_position = if tree {
            let index = self
                .visible_tree()
                .iter()
                .position(|row| row.0 == id)
                .unwrap_or(0);
            let handle = &self.tree_scroll.0.borrow().base_handle;
            point(
                handle.bounds().left() + px(36.),
                (handle.bounds().top()
                    + handle.offset().y
                    + px((index + 1) as f32 * TREE_ROW_HEIGHT))
                .max(handle.bounds().top()),
            )
        } else {
            point(
                px(if self.session.sidebar_visible {
                    self.session.sidebar_width + 30
                } else {
                    30
                } as f32),
                px(70.),
            )
        };
        menu.read(cx).focus_handle(cx).focus(window, cx);
        self.keyboard_menu = Some(menu);
        cx.notify();
    }
}
