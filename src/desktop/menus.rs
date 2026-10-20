//! App menu composition shared by right-click and keyboard invocation.
use super::*;
use component::menu::PopupMenu;

impl Edot {
    pub(super) fn note_menu_targets(&self, id: NoteId) -> Vec<NoteId> {
        if self.selection.ids.contains(&id) {
            self.selection.ids.iter().copied().collect()
        } else {
            vec![id]
        }
    }

    fn note_menu(&self, mut menu: PopupMenu, view: WeakEntity<Edot>, id: NoteId) -> PopupMenu {
        let targets = self.note_menu_targets(id);
        let multiple = targets.len() > 1;
        let nested = self.book.get(id).is_some_and(|note| note.parent != 0);
        if !multiple {
            if self.book.is_folder(id) {
                let weak = view.clone();
                menu = menu.item(
                    PopupMenuItem::new("새 노트").on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| this.create_note(Some(id), window, cx));
                    }),
                );
                let weak = view.clone();
                menu = menu.item(
                    PopupMenuItem::new("새 폴더").on_click(move |_, window, cx| {
                        let _ =
                            weak.update(cx, |this, cx| this.create_folder(Some(id), window, cx));
                    }),
                );
            }
            let weak = view.clone();
            menu = menu.item(PopupMenuItem::new("이름 바꾸기").shortcut("F2").on_click(
                move |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| this.begin_rename_note(id, window, cx));
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
            if self.book.is_note(id) {
                let weak = view.clone();
                menu =
                    menu.separator()
                        .item(PopupMenuItem::new("다른 프로그램으로 열기").on_click(
                            move |_, _, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    if let Some(path) = this.prepare_external_note(id, cx) {
                                        cx.open_with_system(&path);
                                    }
                                });
                            },
                        ));
            }
        } else {
            let weak = view.clone();
            let targets = targets.clone();
            menu = menu.item(
                PopupMenuItem::new("선택한 항목과 함께 새 폴더 만들기").on_click(
                    move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| this.group_notes(&targets, window, cx));
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
                    let _ = weak.update(cx, |this, cx| {
                        this.confirm_delete_notes(targets.clone(), window, cx)
                    });
                }),
        );
        menu
    }
}

pub(super) fn tab_menu(
    menu: PopupMenu,
    weak: WeakEntity<Edot>,
    id: NoteId,
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
    let read_only = editor.read(cx).is_read_only();
    let mut menu = if read_only {
        reading_menu(menu, editor.read(cx).focus_handle(cx))
    } else {
        editing_menu(menu, editor.read(cx).focus_handle(cx), false)
    }
    .separator();
    for (label, shortcut, replace) in [("찾기", "Ctrl+F", false), ("바꾸기", "Ctrl+H", true)] {
        if read_only && replace {
            continue;
        }
        let weak = weak.clone();
        menu = menu.item(PopupMenuItem::new(label).shortcut(shortcut).on_click(
            move |_, window, cx| {
                let _ = weak.update(cx, |this, cx| this.show_find(false, replace, window, cx));
            },
        ));
    }
    menu
}

fn reading_menu(menu: PopupMenu, focus: FocusHandle) -> PopupMenu {
    menu.action_context(focus)
        .menu("복사", "Ctrl+C", Box::new(component::input::Copy))
        .menu("전체 선택", "Ctrl+A", Box::new(component::input::SelectAll))
}

impl Edot {
    pub(super) fn prepare_external_note(
        &mut self,
        id: NoteId,
        cx: &mut Context<Self>,
    ) -> Option<PathBuf> {
        if !self.book.is_note(id) || self.startup_failed {
            return None;
        }
        if let Some(tab) = self.open.iter_mut().find(|tab| tab.id == id)
            && let Some(editor) = &tab.editor
        {
            tab.dirty = editor.read(cx).value().as_ref() != tab.saved;
        }
        if !self.save_all(cx) {
            return None;
        }
        let path = self.book.file(&self.dir, id);
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => Some(path),
            result => {
                let error = result
                    .err()
                    .unwrap_or_else(|| io::Error::other("본문 파일이 아닙니다"));
                self.fail("다른 프로그램으로 열지 못했습니다", error, cx);
                None
            }
        }
    }

    fn install_menu(
        &mut self,
        menu: Entity<PopupMenu>,
        note: Option<NoteId>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu_subscription = Some(cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.keyboard_menu = None;
            this.context_note = None;
            cx.notify();
        }));
        self.menu_position = position;
        self.menu_anchor = Anchor::TopLeft;
        self.context_note = note;
        menu.read(cx).focus_handle(cx).focus(window, cx);
        self.keyboard_menu = Some(menu);
        cx.notify();
    }

    pub(super) fn show_text_mode_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let current = editor.read(cx).text_mode();
        let focus = editor.read(cx).focus_handle(cx);
        let weak = editor.downgrade();
        let menu = PopupMenu::build(window, cx, |mut menu, _, _| {
            let modes = [TextMode::PlainText, TextMode::Markdown];
            for mode in modes {
                let weak = weak.clone();
                let mut item = PopupMenuItem::new(mode.label()).on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |s, cx| s.set_text_mode(mode, cx));
                });
                if mode == current {
                    item = item.shortcut("✓");
                }
                menu = menu.item(item);
            }
            menu.action_context(focus)
                .selected(modes.iter().position(|mode| *mode == current).unwrap_or(0))
        });
        self.install_menu(menu, None, position, window, cx);
        self.menu_anchor = Anchor::BottomRight;
    }

    pub(super) fn show_tree_menu(
        &mut self,
        id: NoteId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.book.get(id).is_none() {
            return;
        }
        self.pressed = None;
        self.tree_focus.focus(window, cx);
        let weak = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, |menu, _, _| {
            self.note_menu(menu.action_context(self.tree_focus.clone()), weak, id)
        });
        self.install_menu(menu, Some(id), position, window, cx);
    }

    pub(super) fn show_tree_background_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pressed = None;
        self.tree_focus.focus(window, cx);
        let weak = cx.entity().downgrade();
        let folder_weak = weak.clone();
        let menu = PopupMenu::build(window, cx, |menu, _, _| {
            menu.action_context(self.tree_focus.clone())
                .item(
                    PopupMenuItem::new("새 노트").on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| this.create_note(Some(0), window, cx));
                    }),
                )
                .item(
                    PopupMenuItem::new("새 폴더").on_click(move |_, window, cx| {
                        let _ = folder_weak
                            .update(cx, |this, cx| this.create_folder(Some(0), window, cx));
                    }),
                )
        });
        self.install_menu(menu, None, position, window, cx);
    }

    pub(super) fn show_keyboard_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use component::menu::PopupMenu;
        let weak = cx.entity().downgrade();
        let focus = window.focused(cx).unwrap_or_else(|| self.focus.clone());
        let tree = self.tree_focus.is_focused(window);
        let read_only = self
            .active_editor()
            .is_some_and(|e| e.read(cx).is_read_only());
        let id = if tree {
            self.caret
        } else {
            self.session.active
        };
        let Some(id) = id else {
            return;
        };
        let menu = PopupMenu::build(window, cx, |menu, _, _| {
            if tree {
                self.note_menu(menu.action_context(focus), weak, id)
            } else if read_only {
                reading_menu(menu, focus)
            } else {
                editing_menu(menu, focus, true)
            }
        });
        let position = if tree {
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
        self.install_menu(menu, tree.then_some(id), position, window, cx);
    }
}
