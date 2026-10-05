use crate::drag::{DropTarget, DropZone};
use crate::model::{Notebook, replace_file};
#[cfg(test)]
use crate::scrollbar::ScrollRange;
use crate::scrollbar::{self, Kind};
use crate::selection::Selection;
use crate::settings::{self, Panel, Settings, ThemeMode};
use crate::tabs::TabLayout;
use crate::theme::{self, Theme};
use crate::tooltip::Tooltip;
use crate::{editing, settings_controls, startup, ui};
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::*;
use windows_sys::Win32::System::SystemServices::{MK_CONTROL, MK_SHIFT, SS_NOTIFY, SS_OWNERDRAW};
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const ID_TOGGLE: usize = 101;
const ID_ADD: usize = 102;
const ID_EDIT: usize = 104;
const ID_TREE: usize = 105;
const ID_TABS: usize = 106;
const ID_TAB_ADD: usize = 107;
const ID_TAB_NAME: usize = 125;
const ID_SETTINGS: usize = 120;
const ID_SETTINGS_THEME: usize = 121;
const ID_SETTINGS_FONT: usize = 122;
const ID_SETTINGS_WRAP: usize = 123;
const ID_SETTINGS_CLOSE: usize = 124;
const ID_FIND: usize = 110;
const ID_FIND_PREV: usize = 111;
const ID_FIND_NEXT: usize = 112;
const ID_FIND_CLOSE: usize = 113;
const ID_FIND_CASE: usize = 114;
const ID_REPLACE_INPUT: usize = 115;
const ID_REPLACE_ONE: usize = 116;
const ID_REPLACE_ALL: usize = 117;
const MENU_UNDO: usize = 301;
const MENU_REDO: usize = 302;
const MENU_CUT: usize = 303;
const MENU_COPY: usize = 304;
const MENU_PASTE: usize = 305;
const MENU_SELECT_ALL: usize = 306;
const MENU_FIND: usize = 307;
const MENU_REPLACE: usize = 308;
const MENU_NEW_CHILD: usize = 201;
const MENU_NEW_ROOT: usize = 202;
const MENU_RENAME: usize = 203;
const MENU_DELETE: usize = 204;
const MENU_UP: usize = 205;
const MENU_DOWN: usize = 206;
const MENU_OUTDENT: usize = 207;
const MENU_GROUP: usize = 208;
const MENU_TAB_CLOSE: usize = 401;
const MENU_TAB_CLOSE_OTHERS: usize = 402;
const MENU_TAB_CLOSE_RIGHT: usize = 403;
const AUTOSAVE_TIMER: usize = 1;
const DRAG_SCROLL_TIMER: usize = 2;
const TAB_DRAG_SCROLL_TIMER: usize = 3;
const POSITION_RENAME: u32 = WM_APP + 1;
const REFRESH_UI: u32 = WM_APP + 2;
const FINISH_TAB_RENAME: u32 = WM_APP + 3;

unsafe fn popup_menu(entries: &[(usize, &str, bool)]) -> HMENU {
    unsafe {
        let menu = CreatePopupMenu();
        for &(id, label, enabled) in entries {
            AppendMenuW(
                menu,
                MF_STRING | if enabled { 0 } else { MF_GRAYED },
                id,
                wide(label).as_ptr(),
            );
        }
        menu
    }
}

unsafe fn tab_menu(index: usize, count: usize) -> HMENU {
    unsafe {
        popup_menu(&[
            (MENU_TAB_CLOSE, "탭 닫기\tCtrl+W", true),
            (MENU_TAB_CLOSE_OTHERS, "다른 탭 닫기", count > 1),
            (MENU_TAB_CLOSE_RIGHT, "오른쪽 탭들 닫기", index + 1 < count),
        ])
    }
}

unsafe fn sidebar_menu(nested: bool) -> HMENU {
    unsafe {
        popup_menu(&[
            (MENU_NEW_CHILD, "하위 메뉴 추가", true),
            (MENU_RENAME, "이름 바꾸기\tF2", true),
            (MENU_OUTDENT, "한 단계 위로", nested),
            (MENU_DELETE, "삭제", true),
        ])
    }
}

unsafe fn sidebar_multi_menu() -> HMENU {
    unsafe {
        popup_menu(&[
            (MENU_DELETE, "삭제", true),
            (MENU_GROUP, "선택한 항목과 함께 새 폴더 만들기", true),
        ])
    }
}

#[derive(Clone, Copy)]
struct TabDrag {
    id: u64,
    start: POINT,
    moving: bool,
    gap: usize,
}

#[derive(Clone, Copy)]
struct TreePress {
    id: u64,
    collapse: bool,
    focus_editor: bool,
    clicked: bool,
}

#[derive(Clone, Copy)]
struct TabRename {
    id: u64,
    focus: HWND,
}

thread_local! { static APP: RefCell<Option<App>> = const { RefCell::new(None) }; }

struct MutexGuard(HANDLE);
impl Drop for MutexGuard {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Clone)]
struct OpenTab {
    id: u64,
    text: String,
    dirty: bool,
    // Restored tabs load their text and native editor on first activation.
    editor: HWND,
}

struct App {
    hwnd: HWND,
    tree: HWND,
    tree_host: HWND,
    tabs: HWND,
    tab_name_edit: HWND,
    tab_rename: Option<TabRename>,
    edit: HWND,
    empty_edit: HWND,
    editor_host: HWND,
    editor_clip: HWND,
    tab_scroll: HWND,
    editor_scroll: [HWND; 2],
    toggle: HWND,
    add: HWND,
    tab_add: HWND,
    settings_button: HWND,
    settings_panel: Option<Panel>,
    settings_visible: bool,
    settings: Settings,
    status: HWND,
    theme: Rc<Theme>,
    view_book: Rc<Notebook>,
    view_handles: Rc<HashMap<isize, u64>>,

    visible: Rc<Vec<isize>>,
    view_tabs: RefCell<Rc<Vec<ui::RenderTab>>>,
    view_parents: Rc<HashSet<u64>>,
    tree_scroll: HWND,
    tab_layout: Cell<TabLayout>,
    tab_hover: Cell<Option<(usize, bool)>>,
    tooltip: Tooltip,
    search_host: HWND,
    find_input: HWND,
    find_prev: HWND,
    find_next: HWND,
    find_close: HWND,
    find_case: HWND,
    replace_input: HWND,
    replace_one: HWND,
    replace_all: HWND,
    find_visible: bool,
    replace_visible: bool,
    find_case_sensitive: bool,
    find_message: String,
    last_query: String,
    last_match: Option<editing::CharRange>,
    status_text: RefCell<String>,
    data_dir: PathBuf,
    book: Notebook,
    handles: HashMap<isize, u64>,
    expanded: HashSet<u64>,
    open: Vec<OpenTab>,
    active: Option<usize>,
    loading: bool,
    sidebar_width: i32,
    sidebar_visible: bool,
    resizing: bool,
    dragging: Option<u64>,
    drag_sources: Rc<Vec<u64>>,
    selection: Selection,
    tree_press: Option<TreePress>,
    tree_mouse_input: bool,
    tab_drag: Option<TabDrag>,
    tree_hover: Option<isize>,
    drop_target: Option<DropTarget>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn lo(v: usize) -> usize {
    v & 0xffff
}
fn hi(v: usize) -> usize {
    (v >> 16) & 0xffff
}
fn point_x(lp: LPARAM) -> i32 {
    (lp as u32 as u16 as i16) as i32
}
fn point_y(lp: LPARAM) -> i32 {
    ((lp as u32 >> 16) as u16 as i16) as i32
}

unsafe fn message(hwnd: HWND, title: &str, body: &str, flags: MESSAGEBOX_STYLE) -> i32 {
    unsafe { MessageBoxW(hwnd, wide(body).as_ptr(), wide(title).as_ptr(), flags) }
}

fn report(hwnd: HWND, action: &str, error: impl std::fmt::Display) {
    unsafe {
        message(
            hwnd,
            "Edot",
            &format!("{action}\n\n{error}"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|cell| {
        if let Ok(mut borrow) = cell.try_borrow_mut()
            && let Some(app) = borrow.as_mut()
        {
            f(app);
            app.publish_view();
        }
    });
}

unsafe fn child(
    parent: HWND,
    class: *const u16,
    label: &str,
    style: u32,
    ex: u32,
    id: usize,
) -> HWND {
    unsafe {
        CreateWindowExW(
            ex,
            class,
            wide(label).as_ptr(),
            style,
            0,
            0,
            0,
            0,
            parent,
            id as HMENU,
            GetModuleHandleW(null()),
            null(),
        )
    }
}

unsafe fn window_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buffer = vec![0u16; len + 1];
        let used = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32).max(0) as usize;
        String::from_utf16_lossy(&buffer[..used])
    }
}
unsafe fn memo_editor(hwnd: HWND) -> HWND {
    let _timing = startup::span("editor.create");
    unsafe {
        let editor = child(
            hwnd,
            wide("RICHEDIT50W").as_ptr(),
            "",
            WS_CHILD
                | WS_CLIPSIBLINGS
                | WS_TABSTOP
                | WS_VSCROLL
                | WS_HSCROLL
                | 0x2000 // ES_DISABLENOSCROLL: reserve a fixed, clipped native scrollbar.
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_WANTRETURN as u32,
            0,
            ID_EDIT,
        );
        SendMessageW(editor, editing::EM_SETTEXTMODE, 1, 0);
        SendMessageW(
            editor,
            editing::EM_SETEVENTMASK,
            0,
            editing::ENM_CHANGE | editing::ENM_SELCHANGE,
        );
        SendMessageW(editor, WM_USER + 53, 0, 0x7fff_ffff); // EM_EXLIMITTEXT
        SendMessageW(editor, WM_USER + 63, 0, 0); // EM_HIDESELECTION: keep search matches visible
        SetWindowSubclass(editor, Some(control_subclass), 1, 2);
        editor
    }
}

unsafe extern "system" fn tab_subclass(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _reference: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND | WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
                return SendMessageW(GetParent(hwnd), msg, wp, lp);
            }
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                ui::paint_window(hwnd, |dc| ui::with_view(|v| v.paint_tabs(dc)));
                return 0;
            }
            WM_PRINT => return DefWindowProcW(hwnd, msg, wp, lp),
            WM_PRINTCLIENT => {
                let dc = wp as HDC;
                let saved = SaveDC(dc);
                ui::with_view(|view| view.paint_tabs(dc));
                RestoreDC(dc, saved);
                return 0;
            }
            WM_LBUTTONDOWN => {
                with_app(|app| app.tab_click(point_x(lp), point_y(lp)));
                return 0;
            }
            WM_CONTEXTMENU => {
                with_app(|app| app.tab_context_menu(lp));
                return 0;
            }
            WM_RBUTTONUP => {
                let mut point = POINT {
                    x: point_x(lp),
                    y: point_y(lp),
                };
                ClientToScreen(hwnd, &mut point);
                let coordinates =
                    ((point.y as u32 & 0xffff) << 16 | (point.x as u32 & 0xffff)) as isize;
                with_app(|app| app.tab_context_menu(coordinates));
                return 0;
            }
            WM_MOUSEMOVE => {
                let mut dragging = false;
                with_app(|app| {
                    dragging = app.tab_drag.is_some();
                    if dragging {
                        app.update_tab_drag(POINT {
                            x: point_x(lp),
                            y: point_y(lp),
                        });
                    }
                });
                if dragging {
                    return 0;
                }
                with_app(|app| {
                    let layout = app.tab_layout.get();
                    let hover = layout
                        .hit(point_x(lp))
                        .map(|i| (i, point_x(lp) >= layout.bounds(i).1 - app.theme.px(24)));
                    if hover != app.tab_hover.replace(hover) {
                        InvalidateRect(hwnd, null(), 0);
                    }
                    app.hover_tab_tooltip(point_x(lp), point_y(lp));
                });
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut track);
                return 0;
            }
            WM_MOUSELEAVE => {
                with_app(|app| {
                    app.tooltip.leave(hwnd);
                    if app.tab_hover.replace(None).is_some() {
                        InvalidateRect(hwnd, null(), 0);
                    }
                });
                return 0;
            }
            WM_LBUTTONUP => {
                with_app(|app| {
                    app.finish_tab_drag(POINT {
                        x: point_x(lp),
                        y: point_y(lp),
                    })
                });
                return 0;
            }
            WM_CAPTURECHANGED | WM_CANCELMODE => {
                with_app(|app| app.cancel_tab_drag());
                return 0;
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                ui::with_view(|v| {
                    SendMessageW(v.tab_scroll, msg, wp, lp);
                });
                return 0;
            }
            WM_GETDLGCODE => return DLGC_WANTARROWS as isize,
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(tab_subclass), id);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

unsafe extern "system" fn tab_name_subclass(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_KEYDOWN if matches!(wp as u16, VK_RETURN | VK_ESCAPE | VK_TAB) => {
                with_app(|app| {
                    if let Err(error) = app.finish_tab_rename(wp != VK_ESCAPE as usize, true) {
                        report(app.hwnd, "메모 이름을 저장하지 못했습니다.", error);
                    }
                });
                return 0;
            }
            WM_CHAR if matches!(wp as u16, VK_RETURN | VK_ESCAPE | VK_TAB) => return 0,
            WM_KILLFOCUS => {
                // Focus changes can occur inside an App mutation. Commit after it finishes.
                PostMessageW(GetParent(GetParent(hwnd)), FINISH_TAB_RENAME, 0, 0);
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(tab_name_subclass), id);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

unsafe extern "system" fn search_host_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND | WM_DRAWITEM | WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_NOTIFY => {
                return SendMessageW(GetParent(hwnd), msg, wp, lp);
            }
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                ui::paint_window(hwnd, |dc| ui::with_view(|view| view.paint_search(dc)));
                return 0;
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(search_host_proc), id);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

unsafe extern "system" fn settings_host_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND | WM_DRAWITEM | WM_MEASUREITEM | WM_CTLCOLOREDIT | WM_CTLCOLORBTN
            | WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX => {
                return SendMessageW(GetParent(hwnd), msg, wp, lp);
            }
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                ui::paint_window(hwnd, |dc| ui::with_view(|view| view.paint_settings(dc)));
                return 0;
            }
            WM_PRINT => return DefWindowProcW(hwnd, msg, wp, lp),
            WM_PRINTCLIENT => {
                ui::with_view(|view| view.paint_settings(wp as HDC));
                return 0;
            }
            WM_NCDESTROY => {
                ui::release_buffer(hwnd);
                RemoveWindowSubclass(hwnd, Some(settings_host_proc), 1);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

unsafe extern "system" fn tree_host_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _reference: usize,
) -> LRESULT {
    unsafe {
        if msg == WM_NOTIFY {
            return SendMessageW(GetParent(hwnd), msg, wp, lp);
        }
        if msg == WM_ERASEBKGND {
            return 1;
        }
        if msg == WM_PAINT {
            ui::paint_window(hwnd, |dc| {
                let mut area = RECT::default();
                GetClientRect(hwnd, &mut area);
                ui::with_view(|v| theme::fill(dc, area, v.theme.colors.sidebar));
            });
            return 0;
        }
        if msg == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(tree_host_proc), id);
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
unsafe fn route_wheel(msg: u32, wp: usize) -> bool {
    let mut control = null_mut();
    unsafe {
        ui::with_view(|view| {
            let mut point = POINT::default();
            GetCursorPos(&mut point);
            let mut bounds = RECT::default();
            GetWindowRect(GetParent(GetParent(view.edit)), &mut bounds);
            if view.active.is_some() && PtInRect(&bounds, point) != 0 {
                control = view.editor_scroll
                    [(msg == WM_MOUSEHWHEEL || wp & MK_SHIFT as usize != 0) as usize];
            }
            for (pane, scrollbar, visible) in [
                (view.tabs, view.tab_scroll, true),
                (view.tab_scroll, view.tab_scroll, view.tab_layout.overflow),
                (GetParent(view.tree), view.tree_scroll, view.sidebar_visible),
            ] {
                GetWindowRect(pane, &mut bounds);
                if visible && PtInRect(&bounds, point) != 0 {
                    control = scrollbar;
                }
            }
        });
        if !control.is_null() {
            SendMessageW(control, msg, wp, 0);
        }
    }
    !control.is_null()
}

unsafe extern "system" fn tree_subclass(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _reference: usize,
) -> LRESULT {
    unsafe {
        if msg == WM_ERASEBKGND {
            return 1;
        }
        if msg == WM_PAINT {
            ui::paint_window(hwnd, |dc| {
                let mut area = RECT::default();
                GetClientRect(hwnd, &mut area);
                ui::with_view(|v| theme::fill(dc, area, v.theme.colors.sidebar));
                DefSubclassProc(hwnd, WM_PRINTCLIENT, dc as usize, PRF_CLIENT as isize);
                ui::with_view(|view| view.paint_drag_feedback(dc));
            });
            ui::with_view(|v| {
                InvalidateRect(v.tree_scroll, null(), 0);
            });
            return 0;
        }
        if matches!(msg, WM_MOUSEWHEEL | WM_MOUSEHWHEEL) {
            if route_wheel(msg, wp) {
                return 0;
            }
            ui::with_view(|v| {
                SendMessageW(v.tree_scroll, msg, wp, lp);
            });
            return 0;
        }
        if msg == WM_MOUSEMOVE {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                ..Default::default()
            };
            TrackMouseEvent(&mut tracking);
            with_app(|app| {
                let hovered = app.hit_tree().map(|(_, _, handle)| handle);
                if app.tree_hover != hovered {
                    app.tree_hover = hovered;
                    app.publish_view();
                    RedrawWindow(
                        hwnd,
                        null(),
                        null_mut(),
                        RDW_INVALIDATE | RDW_UPDATENOW | RDW_NOERASE,
                    );
                }
                app.hover_tree_tooltip();
            });
        }
        if msg == WM_MOUSELEAVE {
            with_app(|app| {
                app.tooltip.leave(hwnd);
                if app.tree_hover.take().is_some() {
                    app.publish_view();
                    InvalidateRect(hwnd, null(), 0);
                }
            });
        }
        if msg == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(tree_subclass), id);
        }
        if msg == WM_CONTEXTMENU {
            with_app(|app| app.context_menu(lp));
            return 0;
        }
        if msg == WM_LBUTTONUP {
            with_app(|app| app.tree_mouse_input = true);
        }
        if matches!(msg, WM_LBUTTONDOWN | WM_RBUTTONDOWN) {
            with_app(|app| {
                app.tree_mouse_input = true;
                app.tree_mouse_down(
                    POINT {
                        x: point_x(lp),
                        y: point_y(lp),
                    },
                    msg == WM_RBUTTONDOWN,
                    wp & MK_CONTROL as usize != 0,
                    wp & MK_SHIFT as usize != 0,
                );
            });
        }
        if msg == WM_KEYDOWN && wp == VK_SPACE as usize && GetKeyState(VK_CONTROL as i32) < 0 {
            with_app(|app| {
                if let Some(id) = app.selected_id() {
                    app.selection.click(id, &[], true, false);
                    app.refresh_selection();
                }
            });
            return 0;
        }
        let result = DefSubclassProc(hwnd, msg, wp, lp);
        if matches!(msg, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_LBUTTONUP) {
            with_app(|app| app.tree_mouse_input = false);
        }
        if matches!(msg, WM_LBUTTONDOWN | WM_LBUTTONUP) {
            with_app(|app| {
                if msg == WM_LBUTTONUP || app.tree_press.is_some_and(|press| press.clicked) {
                    app.tree_mouse_up();
                }
            });
        }
        if msg == WM_CANCELMODE || (msg == WM_CAPTURECHANGED && lp as HWND != hwnd) {
            with_app(|app| {
                if !app.tree_mouse_input {
                    app.tree_press = None;
                }
            });
        }

        if msg == WM_KEYDOWN {
            ui::with_view(|v| {
                InvalidateRect(v.tree_scroll, null(), 0);
            });
        }
        result
    }
}

unsafe extern "system" fn editor_host_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND | WM_NOTIFY | WM_CTLCOLOREDIT => {
                return SendMessageW(GetParent(hwnd), msg, wp, lp);
            }
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                // The editor covers this host. Paint only its clipped remainder;
                // a second body-sized bitmap would waste memory.
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                ui::with_view(|v| {
                    theme::fill(dc, paint.rcPaint, v.theme.colors.white);
                });
                EndPaint(hwnd, &paint);
                return 0;
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(editor_host_proc), id);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

unsafe extern "system" fn control_subclass(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    subclass_id: usize,
    reference: usize,
) -> LRESULT {
    unsafe {
        if reference == 1 {
            if msg == WM_ERASEBKGND {
                // Owner drawing fills the complete button in one buffered pass.
                return 1;
            }
            if msg == WM_PRINT {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            if msg == WM_PRINTCLIENT {
                let mut area = RECT::default();
                GetClientRect(hwnd, &mut area);
                let state = SendMessageW(hwnd, BM_GETSTATE, 0, 0) as u32;
                let draw = DRAWITEMSTRUCT {
                    CtlType: ODT_BUTTON,
                    CtlID: GetDlgCtrlID(hwnd) as u32,
                    itemAction: ODA_DRAWENTIRE,
                    itemState: if state & BST_PUSHED != 0 {
                        ODS_SELECTED
                    } else {
                        0
                    } | if GetFocus() == hwnd { ODS_FOCUS } else { 0 }
                        | if IsWindowEnabled(hwnd) == 0 {
                            ODS_DISABLED
                        } else {
                            0
                        },
                    hwndItem: hwnd,
                    hDC: wp as HDC,
                    rcItem: area,
                    ..Default::default()
                };
                ui::with_view(|view| view.paint_control(&draw));
                return 0;
            }
        }
        if reference == 2 && matches!(msg, WM_MOUSEWHEEL | WM_MOUSEHWHEEL) {
            if !route_wheel(msg, wp) {
                ui::with_view(|v| {
                    SendMessageW(
                        v.editor_scroll[(msg == WM_MOUSEHWHEEL) as usize],
                        msg,
                        wp,
                        lp,
                    );
                });
            }
            return 0;
        }

        if reference == 2 && msg == WM_CONTEXTMENU {
            with_app(|app| app.editor_menu());
            return 0;
        }
        if reference == 3 && matches!(msg, WM_SETFOCUS | WM_KILLFOCUS) {
            InvalidateRect(GetParent(hwnd), null(), 0);
        }

        if reference == 1 && msg == WM_MOUSEMOVE {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut tracking);
            InvalidateRect(hwnd, null(), 0);
        }
        if msg == WM_MOUSELEAVE || (reference == 1 && (msg == WM_SETFOCUS || msg == WM_KILLFOCUS)) {
            InvalidateRect(hwnd, null(), 0);
        }
        let result = DefSubclassProc(hwnd, msg, wp, lp);
        if reference == 2
            && msg == WM_KEYDOWN
            && GetKeyState(VK_CONTROL as i32) < 0
            && matches!(wp as u16, VK_HOME | VK_END)
        {
            let target = if wp as u16 == VK_HOME {
                0
            } else {
                scrollbar::body_range(hwnd, false).extent() as i32
            };
            scrollbar::set_body_position(hwnd, false, target);
            if wp as u16 == VK_HOME {
                scrollbar::set_body_position(hwnd, true, 0);
            }
        }

        if reference == 2
            && matches!(
                msg,
                WM_PAINT
                    | WM_SIZE
                    | WM_TIMER
                    | WM_MOUSEWHEEL
                    | WM_MOUSEHWHEEL
                    | WM_VSCROLL
                    | WM_HSCROLL
                    | WM_KEYDOWN
                    | WM_CHAR
                    | WM_SETTEXT
                    | WM_SETFONT
                    | EM_SCROLLCARET
                    | EM_REPLACESEL
                    | WM_UNDO
                    | editing::EM_SETSCROLLPOS
                    | editing::EM_SETTARGETDEVICE
                    | editing::EM_REDO
            )
        {
            ui::with_view(|view| {
                if view.edit == hwnd {
                    scrollbar::layout_body(hwnd);
                    view.refresh_editor_scroll();
                }
            });
        }
        if reference == 3 && msg == WM_PAINT && GetWindowTextLengthW(hwnd) == 0 {
            let dc = GetDC(hwnd);
            ui::with_view(|view| {
                let mut area = RECT::default();
                GetClientRect(hwnd, &mut area);
                theme::text(
                    dc,
                    if hwnd == view.find_input {
                        "찾을 내용"
                    } else {
                        "바꿀 내용"
                    },
                    area,
                    view.theme.ui,
                    view.theme.colors.muted,
                    DT_LEFT,
                );
            });
            ReleaseDC(hwnd, dc);
        }
        if reference == 2 && matches!(msg, WM_KEYUP | WM_LBUTTONUP) {
            with_app(|app| {
                InvalidateRect(app.status, null(), 0);
            });
        }
        if msg == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(control_subclass), subclass_id);
        }
        result
    }
}

impl App {
    unsafe fn new(hwnd: HWND, data_dir: PathBuf, book: Notebook, settings: Settings) -> Self {
        let controls_timing = startup::span("ui.controls");
        let common = WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS;
        let toggle = unsafe {
            child(
                hwnd,
                wide("BUTTON").as_ptr(),
                "사이드바 접기 / 펼치기",
                common | WS_TABSTOP | BS_OWNERDRAW as u32,
                0,
                ID_TOGGLE,
            )
        };
        let add = unsafe {
            child(
                hwnd,
                wide("BUTTON").as_ptr(),
                "새 메모",
                common | WS_TABSTOP | BS_OWNERDRAW as u32,
                0,
                ID_ADD,
            )
        };
        let tree_host = unsafe {
            child(
                hwnd,
                wide("STATIC").as_ptr(),
                "",
                common | WS_CLIPCHILDREN,
                0,
                109,
            )
        };
        unsafe {
            SetWindowSubclass(tree_host, Some(tree_host_proc), 1, 0);
        }
        let tree = unsafe {
            child(
                tree_host,
                WC_TREEVIEWW,
                "",
                common
                    | WS_TABSTOP
                    | TVS_HASBUTTONS
                    | TVS_LINESATROOT
                    | TVS_EDITLABELS
                    | TVS_FULLROWSELECT
                    | TVS_NOHSCROLL
                    | TVS_SHOWSELALWAYS
                    | TVS_NOTOOLTIPS,
                0,
                ID_TREE,
            )
        };
        let tabs = unsafe {
            child(
                hwnd,
                wide("STATIC").as_ptr(),
                "",
                common | WS_TABSTOP | WS_CLIPCHILDREN | SS_NOTIFY,
                0,
                ID_TABS,
            )
        };
        let tab_add = unsafe {
            child(
                hwnd,
                wide("BUTTON").as_ptr(),
                "새 메모 탭",
                common | WS_TABSTOP | BS_OWNERDRAW as u32,
                0,
                ID_TAB_ADD,
            )
        };
        let settings_button = unsafe {
            child(
                hwnd,
                wide("BUTTON").as_ptr(),
                "설정",
                common | WS_TABSTOP | BS_OWNERDRAW as u32,
                0,
                ID_SETTINGS,
            )
        };
        unsafe {
            SetWindowSubclass(tabs, Some(tab_subclass), 1, 0);
        }
        let editor_host = unsafe {
            child(
                hwnd,
                wide("STATIC").as_ptr(),
                "",
                common | WS_CLIPCHILDREN,
                0,
                126,
            )
        };
        let editor_scroll = [false, true].map(|horizontal| unsafe {
            let control = child(
                hwnd,
                wide("STATIC").as_ptr(),
                if horizontal {
                    "본문 가로 스크롤"
                } else {
                    "본문 세로 스크롤"
                },
                common | SS_NOTIFY,
                0,
                if horizontal { 128 } else { 127 },
            );
            scrollbar::attach(
                control,
                if horizontal {
                    Kind::EditorHorizontal
                } else {
                    Kind::EditorVertical
                },
            );
            control
        });
        unsafe {
            SetWindowSubclass(editor_host, Some(editor_host_proc), 1, 0);
        }
        let editor_clip = unsafe {
            child(
                editor_host,
                wide("STATIC").as_ptr(),
                "",
                common | WS_CLIPCHILDREN,
                0,
                129,
            )
        };
        let tab_scroll = unsafe {
            child(
                hwnd,
                wide("STATIC").as_ptr(),
                "Tab scroll",
                common | SS_NOTIFY,
                0,
                130,
            )
        };
        unsafe {
            SetWindowSubclass(editor_clip, Some(editor_host_proc), 1, 0);
            scrollbar::attach(tab_scroll, Kind::Tabs);
        }
        let edit = unsafe { memo_editor(editor_clip) };
        let status = unsafe {
            child(
                hwnd,
                wide("STATIC").as_ptr(),
                "",
                common | SS_OWNERDRAW,
                0,
                0,
            )
        };
        unsafe {
            SetWindowSubclass(tree, Some(tree_subclass), 1, 0);
            EnableWindow(edit, 0);
            for button in [toggle, add, tab_add, settings_button] {
                SetWindowSubclass(button, Some(control_subclass), 1, 1);
            }
            SetWindowSubclass(edit, Some(control_subclass), 1, 2);
        }
        let tree_scroll = unsafe {
            child(
                hwnd,
                wide("STATIC").as_ptr(),
                "",
                common | SS_NOTIFY,
                0,
                108,
            )
        };
        unsafe {
            scrollbar::attach(tree_scroll, Kind::Tree);
        }
        let search_host = unsafe {
            child(
                hwnd,
                wide("STATIC").as_ptr(),
                "찾기 및 바꾸기",
                WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                0,
                118,
            )
        };
        unsafe {
            SetWindowSubclass(search_host, Some(search_host_proc), 1, 0);
        }
        let search_style = WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS;
        let find_input = unsafe {
            child(
                search_host,
                wide("EDIT").as_ptr(),
                "",
                search_style | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                0,
                ID_FIND,
            )
        };
        let replace_input = unsafe {
            child(
                search_host,
                wide("EDIT").as_ptr(),
                "",
                search_style | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                0,
                ID_REPLACE_INPUT,
            )
        };
        let button = |label: &str, id| unsafe {
            child(
                search_host,
                wide("BUTTON").as_ptr(),
                label,
                search_style | WS_TABSTOP | BS_OWNERDRAW as u32,
                0,
                id,
            )
        };
        let find_prev = button("이전", ID_FIND_PREV);
        let find_next = button("다음", ID_FIND_NEXT);
        let find_close = button("닫기", ID_FIND_CLOSE);
        let find_case = button("대소문자 구분", ID_FIND_CASE);
        let replace_one = button("바꾸기", ID_REPLACE_ONE);
        let replace_all = button("모두 바꾸기", ID_REPLACE_ALL);
        unsafe {
            for button in [
                find_prev,
                find_next,
                find_close,
                find_case,
                replace_one,
                replace_all,
            ] {
                SetWindowSubclass(button, Some(control_subclass), 1, 1);
            }
            for field in [find_input, replace_input] {
                SetWindowSubclass(field, Some(control_subclass), 1, 3);
            }
        }
        drop(controls_timing);
        let expanded = book.notes.iter().map(|n| n.id).collect();
        let mut app = Self {
            hwnd,
            tree,
            tree_host,
            tabs,
            tab_name_edit: null_mut(),
            tab_rename: None,
            edit,
            empty_edit: edit,
            editor_host,
            editor_clip,
            tab_scroll,
            editor_scroll,
            toggle,
            add,
            tab_add,
            settings_button,
            settings_panel: None,
            settings_visible: false,
            theme: Rc::new(unsafe {
                Theme::new(GetDpiForWindow(hwnd), settings.dark(), &settings.font)
            }),
            settings,
            status,
            view_book: Rc::new(book.clone()),
            view_handles: Rc::new(HashMap::new()),

            visible: Rc::new(Vec::new()),
            view_tabs: RefCell::new(Rc::new(Vec::new())),
            view_parents: Rc::new(HashSet::new()),
            tree_scroll,
            tab_layout: Cell::new(TabLayout::default()),
            tab_hover: Cell::new(None),
            tooltip: unsafe { Tooltip::new(hwnd) },
            search_host,
            find_input,
            find_prev,
            find_next,
            find_close,
            find_case,
            replace_input,
            replace_one,
            replace_all,
            find_visible: false,
            replace_visible: false,
            find_case_sensitive: false,
            find_message: "Enter로 다음 결과를 찾습니다".into(),
            last_query: String::new(),
            last_match: None,
            status_text: RefCell::new("준비됨".into()),
            data_dir,
            book,
            handles: HashMap::new(),
            expanded,
            open: Vec::new(),
            active: None,
            loading: false,
            sidebar_width: 260,
            sidebar_visible: true,
            resizing: false,
            dragging: None,
            drag_sources: Rc::new(Vec::new()),
            selection: Selection::default(),
            tree_press: None,
            tree_mouse_input: false,
            tab_drag: None,
            tree_hover: None,
            drop_target: None,
        };
        unsafe {
            app.publish_view();
            app.apply_theme(true, true, true);
            app.rebuild_tree(None);
            app.restore_session();
            app.layout();
        }
        if app.open.is_empty() {
            unsafe {
                app.set_status("준비됨");
            }
        }
        app
    }

    unsafe fn restore_session(&mut self) {
        let _timing = startup::span("session.restore");
        let Ok(text) = fs::read_to_string(self.data_dir.join("session.tsv")) else {
            return;
        };
        let mut lines = text.lines();
        if lines.next() != Some("EDOTSESSION1") {
            return;
        }
        let Some(settings) = lines.next() else {
            return;
        };
        let mut fields = settings.split('\t');
        self.sidebar_visible = fields.next() == Some("1");
        self.sidebar_width = fields
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(260)
            .clamp(150, 600);
        let active_id = fields.next().and_then(|v| v.parse::<u64>().ok());
        for line in lines {
            if let Ok(id) = line.parse::<u64>()
                && self.book.get(id).is_some()
                && !self.open.iter().any(|tab| tab.id == id)
            {
                self.open.push(OpenTab {
                    id,
                    text: String::new(),
                    dirty: false,
                    editor: null_mut(),
                });
            }
        }
        let active = self
            .open
            .iter()
            .position(|t| Some(t.id) == active_id)
            .or_else(|| self.open.len().checked_sub(1));
        if let Some(index) = active {
            unsafe {
                self.activate(index);
            }
        }
    }

    fn save_session(&self) -> std::io::Result<()> {
        let active_id = self.active.map(|i| self.open[i].id).unwrap_or(0);
        let mut text = format!(
            "EDOTSESSION1\n{}\t{}\t{}\n",
            self.sidebar_visible as u8, self.sidebar_width, active_id
        );
        for tab in &self.open {
            text.push_str(&format!("{}\n", tab.id));
        }
        replace_file(&self.data_dir.join("session.tsv"), text.as_bytes())
    }

    unsafe fn apply_theme(&self, font: bool, colors: bool, wrap: bool) {
        unsafe {
            for item in [
                self.toggle,
                self.add,
                self.tab_add,
                self.settings_button,
                self.tree,
                self.tabs,
            ] {
                SendMessageW(item, WM_SETFONT, self.theme.ui as WPARAM, 0);
            }
            if !self.tab_name_edit.is_null() {
                SendMessageW(self.tab_name_edit, WM_SETFONT, self.theme.tab as usize, 0);
            }
            for tab in &self.open {
                if !tab.editor.is_null() {
                    self.apply_editor_settings(tab.editor, font, colors, wrap);
                }
            }
            self.apply_editor_settings(self.empty_edit, font, colors, wrap);
            for item in [
                self.find_input,
                self.replace_input,
                self.find_case,
                self.find_prev,
                self.find_next,
                self.find_close,
                self.replace_one,
                self.replace_all,
            ] {
                SendMessageW(item, WM_SETFONT, self.theme.small as usize, 0);
            }
            for field in [self.find_input, self.replace_input] {
                SendMessageW(field, WM_SETFONT, self.theme.ui as usize, 0);
            }
            self.tooltip.style(&self.theme);
            SendMessageW(
                self.tree,
                TVM_SETBKCOLOR,
                0,
                self.theme.colors.sidebar as LPARAM,
            );
            SendMessageW(
                self.tree,
                TVM_SETTEXTCOLOR,
                0,
                self.theme.colors.ink as LPARAM,
            );
            SendMessageW(self.tree, TVM_SETITEMHEIGHT, self.theme.px(29) as usize, 0);
            SendMessageW(self.tree, TVM_SETINDENT, self.theme.px(22) as usize, 0);
            self.style_settings_panel();
            self.style_title_bar();
        }
    }

    unsafe fn apply_editor_settings(&self, editor: HWND, font: bool, colors: bool, wrap: bool) {
        unsafe {
            let visible = IsWindowVisible(editor) != 0;
            let modified = SendMessageW(editor, EM_GETMODIFY, 0, 0);
            let mut scroll = POINT {
                x: scrollbar::body_range(editor, true).position,
                y: scrollbar::body_range(editor, false).position,
            };
            if visible {
                SendMessageW(editor, WM_SETREDRAW, 0, 0);
            }
            if font {
                SendMessageW(editor, WM_SETFONT, self.theme.editor as usize, 0);
            }
            // WM_SETFONT resets RichEdit's default foreground color as well.
            if colors || font {
                SendMessageW(
                    editor,
                    editing::EM_SETBKGNDCOLOR,
                    0,
                    self.theme.colors.white as isize,
                );
                editing::text_color(editor, self.theme.colors.ink);
            }
            if wrap {
                // RichEdit's screen target uses 0 for wrapping to the client
                // width and 1 for disabling wrapping. Text stays untouched.
                // Reserve both native bars in every mode. layout_body clips
                // them outside the viewport independently of text padding.
                SendMessageW(
                    editor,
                    editing::EM_SETTARGETDEVICE,
                    0,
                    if self.settings.wrap { 0 } else { 1 },
                );
                SetWindowPos(
                    editor,
                    null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
                if self.settings.wrap {
                    scroll.x = 0;
                }
            }
            if font || wrap {
                scrollbar::configure_body(editor, self.settings.wrap);
            }
            scrollbar::layout_body(editor);
            scrollbar::set_body_position(editor, true, scroll.x);
            scrollbar::set_body_position(editor, false, scroll.y);
            SendMessageW(editor, EM_SETMODIFY, modified as usize, 0);
            if visible {
                SendMessageW(editor, WM_SETREDRAW, 1, 0);
                InvalidateRect(editor, null(), 0);
            }
        }
    }

    unsafe fn style_title_bar(&self) {
        unsafe {
            let dark = self.theme.dark as i32;
            DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                &dark as *const _ as *const _,
                std::mem::size_of_val(&dark) as u32,
            );
        }
    }

    unsafe fn style_settings_panel(&self) {
        if let Some(panel) = self.settings_panel {
            unsafe {
                for control in [
                    panel.theme,
                    panel.font,
                    panel.wrap,
                    panel.close,
                    panel.theme_label,
                    panel.font_label,
                ] {
                    SendMessageW(control, WM_SETFONT, self.theme.ui as usize, 0);
                }
                for control in [panel.theme, panel.font] {
                    settings_controls::style_dropdown(control, &self.theme);
                }
                SetWindowTextW(
                    panel.wrap,
                    wide(if self.settings.wrap {
                        "자동 줄 바꿈: 켜짐"
                    } else {
                        "자동 줄 바꿈: 꺼짐"
                    })
                    .as_ptr(),
                );
                InvalidateRect(panel.host, null(), 0);
                InvalidateRect(panel.wrap, null(), 0);
                RedrawWindow(
                    panel.host,
                    null(),
                    null_mut(),
                    RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
                );
            }
        }
    }

    unsafe fn show_settings(&mut self, show: bool) {
        unsafe {
            if show && self.settings_panel.is_none() {
                self.tooltip.dismiss();
                let common = WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS;
                let host = child(
                    self.hwnd,
                    wide("STATIC").as_ptr(),
                    "설정",
                    WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                    0,
                    125,
                );
                SetWindowSubclass(host, Some(settings_host_proc), 1, 0);
                let combo = |label: &str, id| {
                    child(
                        host,
                        wide("COMBOBOX").as_ptr(),
                        label,
                        common
                            | WS_TABSTOP
                            | WS_VSCROLL
                            | CBS_DROPDOWNLIST as u32
                            | CBS_OWNERDRAWFIXED as u32
                            | CBS_HASSTRINGS as u32,
                        0,
                        id,
                    )
                };
                let theme = combo("앱 테마", ID_SETTINGS_THEME);
                let font = combo("본문 글꼴", ID_SETTINGS_FONT);
                settings_controls::attach_dropdown(theme);
                settings_controls::attach_dropdown(font);
                let wrap = child(
                    host,
                    wide("BUTTON").as_ptr(),
                    "자동 줄 바꿈",
                    common | WS_TABSTOP | BS_OWNERDRAW as u32,
                    0,
                    ID_SETTINGS_WRAP,
                );
                let close = child(
                    host,
                    wide("BUTTON").as_ptr(),
                    "닫기",
                    common | WS_TABSTOP | BS_OWNERDRAW as u32,
                    0,
                    ID_SETTINGS_CLOSE,
                );
                let theme_label = child(host, wide("STATIC").as_ptr(), "앱 테마", common, 0, 0);
                let font_label = child(host, wide("STATIC").as_ptr(), "본문 글꼴", common, 0, 0);
                for control in [wrap, close] {
                    SetWindowSubclass(control, Some(control_subclass), 1, 1);
                }
                for label in ["밝게", "어둡게", "시스템 설정 사용"] {
                    SendMessageW(theme, CB_ADDSTRING, 0, wide(label).as_ptr() as LPARAM);
                }
                let mut fonts = settings::installed_fonts(self.hwnd);
                if !fonts.contains(&self.settings.font) {
                    fonts.push(self.settings.font.clone());
                    fonts.sort_unstable();
                }
                for name in &fonts {
                    SendMessageW(font, CB_ADDSTRING, 0, wide(name).as_ptr() as LPARAM);
                }
                SendMessageW(
                    font,
                    CB_SETCURSEL,
                    fonts
                        .iter()
                        .position(|name| name == &self.settings.font)
                        .unwrap_or(0),
                    0,
                );
                SendMessageW(
                    theme,
                    CB_SETCURSEL,
                    match self.settings.theme {
                        ThemeMode::Light => 0,
                        ThemeMode::Dark => 1,
                        ThemeMode::System => 2,
                    },
                    0,
                );
                settings_controls::confirm_selection(theme);
                settings_controls::confirm_selection(font);
                self.settings_panel = Some(Panel {
                    host,
                    theme,
                    font,
                    wrap,
                    close,
                    theme_label,
                    font_label,
                });
                // Styling can synchronously request owner drawing.
                self.publish_view();
                self.style_settings_panel();
            }
            self.settings_visible = show;
            if show {
                self.find_visible = false;
                ShowWindow(self.search_host, SW_HIDE);
            }
            self.publish_view();
            self.layout_settings();
            if let Some(panel) = self.settings_panel {
                SetFocus(if show {
                    panel.theme
                } else if self.active.is_some() {
                    self.edit
                } else {
                    self.settings_button
                });
            }
            InvalidateRect(self.settings_button, null(), 0);
        }
    }

    unsafe fn layout_settings(&self) {
        let Some(panel) = self.settings_panel else {
            return;
        };
        unsafe {
            if !self.settings_visible {
                ShowWindow(panel.host, SW_HIDE);
                return;
            }
            let p = |v| self.theme.px(v);
            let mut area = RECT::default();
            GetClientRect(self.hwnd, &mut area);
            let width = p(360).min(area.right - p(24));
            let first_show = GetWindowLongPtrW(panel.host, GWL_STYLE) as u32 & WS_VISIBLE == 0;
            SetWindowPos(
                panel.host,
                HWND_TOP,
                area.right - width - p(12),
                p(42),
                width,
                p(312),
                SWP_NOACTIVATE | SWP_NOREDRAW,
            );
            for (control, x, y, w, h) in [
                (panel.close, width - p(66), p(12), p(48), p(28)),
                (panel.theme_label, p(20), p(52), width - p(40), p(22)),
                (panel.theme, p(20), p(76), width - p(40), p(230)),
                (panel.font_label, p(20), p(130), width - p(40), p(22)),
                (panel.font, p(20), p(154), width - p(40), p(280)),
                (panel.wrap, p(20), p(218), width - p(40), p(40)),
            ] {
                SetWindowPos(
                    control,
                    null_mut(),
                    x,
                    y,
                    w,
                    h,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW,
                );
            }
            // Expose only the fully laid-out panel, then finish its first frame.
            if first_show {
                ShowWindow(panel.host, SW_SHOWNA);
            }
            RedrawWindow(
                panel.host,
                null(),
                null_mut(),
                RDW_INVALIDATE
                    | RDW_ALLCHILDREN
                    | RDW_NOERASE
                    | if first_show { RDW_UPDATENOW } else { 0 },
            );
        }
    }

    unsafe fn change_setting(&mut self, id: usize) {
        let Some(panel) = self.settings_panel else {
            return;
        };
        let mut next = self.settings.clone();
        unsafe {
            match id {
                ID_SETTINGS_THEME => {
                    next.theme = match SendMessageW(panel.theme, CB_GETCURSEL, 0, 0) {
                        0 => ThemeMode::Light,
                        1 => ThemeMode::Dark,
                        _ => ThemeMode::System,
                    }
                }
                ID_SETTINGS_FONT => {
                    let index = SendMessageW(panel.font, CB_GETCURSEL, 0, 0);
                    if index < 0 {
                        return;
                    }
                    let len = SendMessageW(panel.font, CB_GETLBTEXTLEN, index as usize, 0);
                    if !(1..32).contains(&len) {
                        return;
                    }
                    let mut text = vec![0u16; len as usize + 1];
                    SendMessageW(
                        panel.font,
                        CB_GETLBTEXT,
                        index as usize,
                        text.as_mut_ptr() as LPARAM,
                    );
                    next.font = String::from_utf16_lossy(&text[..len as usize]);
                }
                ID_SETTINGS_WRAP => next.wrap = !next.wrap,
                _ => return,
            }
        }
        if next == self.settings {
            unsafe {
                if id == ID_SETTINGS_THEME || id == ID_SETTINGS_FONT {
                    settings_controls::confirm_selection(if id == ID_SETTINGS_THEME {
                        panel.theme
                    } else {
                        panel.font
                    });
                }
            }
            return;
        }
        if let Err(error) = next.save(&self.data_dir) {
            unsafe {
                SendMessageW(
                    panel.theme,
                    CB_SETCURSEL,
                    match self.settings.theme {
                        ThemeMode::Light => 0,
                        ThemeMode::Dark => 1,
                        ThemeMode::System => 2,
                    },
                    0,
                );
                let index = SendMessageW(
                    panel.font,
                    CB_FINDSTRINGEXACT,
                    usize::MAX,
                    wide(&self.settings.font).as_ptr() as LPARAM,
                );
                SendMessageW(panel.font, CB_SETCURSEL, index as usize, 0);
                settings_controls::confirm_selection(panel.theme);
                settings_controls::confirm_selection(panel.font);
            }
            report(self.hwnd, "설정을 저장하지 못했습니다.", error);
            return;
        }
        unsafe {
            if id == ID_SETTINGS_THEME || id == ID_SETTINGS_FONT {
                settings_controls::confirm_selection(if id == ID_SETTINGS_THEME {
                    panel.theme
                } else {
                    panel.font
                });
            }
            self.apply_settings(next);
        }
    }

    unsafe fn apply_settings(&mut self, next: Settings) {
        let font = next.font != self.settings.font;
        let wrap = next.wrap != self.settings.wrap;
        let dark = next.dark();
        let colors = dark != self.theme.dark;
        self.settings = next;
        unsafe {
            if font || colors {
                let theme = Rc::new(Theme::new(self.theme.dpi, dark, &self.settings.font));
                let old = std::mem::replace(&mut self.theme, theme);
                self.publish_view();
                self.apply_theme(font, colors, wrap);
                drop(old);
                RedrawWindow(
                    self.hwnd,
                    null(),
                    null_mut(),
                    RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
                );
            } else if wrap {
                for tab in &self.open {
                    if !tab.editor.is_null() {
                        self.apply_editor_settings(tab.editor, false, false, true);
                    }
                }
                self.apply_editor_settings(self.empty_edit, false, false, true);
            }
            self.publish_view();
            self.style_settings_panel();
            if wrap {
                self.layout();
            }
        }
    }

    fn sidebar_edge(&self, width: i32) -> i32 {
        if self.sidebar_visible {
            self.theme
                .px(self.sidebar_width)
                .min((width - self.theme.px(220)).max(self.theme.px(150)))
        } else {
            0
        }
    }

    fn publish_view(&self) {
        if self.view_tabs.borrow().len() != self.open.len() {
            *self.view_tabs.borrow_mut() = Rc::new(
                self.open
                    .iter()
                    .map(|t| ui::RenderTab { id: t.id })
                    .collect(),
            );
        }
        ui::publish(ui::View {
            hwnd: self.hwnd,
            tree: self.tree,
            tabs: self.tabs,
            tab_scroll: self.tab_scroll,
            edit: self.edit,
            editor_scroll: self.editor_scroll,
            toggle: self.toggle,
            add: self.add,
            tab_add: self.tab_add,
            settings_button: self.settings_button,
            settings_panel: self.settings_panel,
            settings_visible: self.settings_visible,
            word_wrap: self.settings.wrap,
            status: self.status,
            theme: Rc::clone(&self.theme),
            book: Rc::clone(&self.view_book),
            handles: Rc::clone(&self.view_handles),
            open: Rc::clone(&self.view_tabs.borrow()),
            parents: Rc::clone(&self.view_parents),
            active: self.active,
            sidebar_width: self.sidebar_width,
            sidebar_visible: self.sidebar_visible,
            status_text: self.status_text.borrow().clone(),
            tab_layout: self.tab_layout.get(),
            tab_rename: self.tab_rename.map(|rename| rename.id),
            tree_scroll: self.tree_scroll,
            visible: Rc::clone(&self.visible),
            find_visible: self.find_visible,
            replace_visible: self.replace_visible,
            resizing: self.resizing,
            tree_hover: self.tree_hover,
            dragging: self.dragging,
            drag_sources: Rc::clone(&self.drag_sources),
            selected: Rc::clone(&self.selection.ids),
            tab_insertion: self
                .tab_drag
                .filter(|drag| drag.moving)
                .map(|drag| drag.gap),
            drop_target: self.drop_target,
            search_host: self.search_host,
            find_input: self.find_input,
            replace_input: self.replace_input,
            search_buttons: [
                self.find_case,
                self.find_prev,
                self.find_next,
                self.find_close,
                self.replace_one,
                self.replace_all,
            ],
            find_case_sensitive: self.find_case_sensitive,
            find_message: self.find_message.clone(),
        });
    }
    fn sync_tree_view(&mut self) {
        self.view_book = Rc::new(self.book.clone());
        self.view_parents = Rc::new(self.book.notes.iter().map(|n| n.parent).collect());
        self.view_handles = Rc::new(self.handles.clone());
        self.refresh_tree_visibility();
        self.publish_view();
    }
    fn refresh_tree_visibility(&mut self) -> bool {
        let mut visible = Vec::new();
        unsafe {
            let mut handle = SendMessageW(self.tree, TVM_GETNEXTITEM, TVGN_ROOT as usize, 0);
            while handle != 0 {
                visible.push(handle);
                handle = SendMessageW(
                    self.tree,
                    TVM_GETNEXTITEM,
                    TVGN_NEXTVISIBLE as usize,
                    handle,
                );
            }
        }
        if self.visible.as_ref() == &visible {
            false
        } else {
            self.visible = Rc::new(visible);
            true
        }
    }
    unsafe fn layout(&self) {
        let _timing = startup::span("ui.layout");
        unsafe {
            let p = |v| self.theme.px(v);
            let mut area = RECT::default();
            GetClientRect(self.hwnd, &mut area);
            let (w, h) = (area.right, area.bottom);
            let left = self.sidebar_edge(w);
            let content_x = if self.sidebar_visible { left + 1 } else { 0 };
            let tabs_x = content_x + if self.sidebar_visible { 0 } else { p(42) };
            let available = (w - tabs_x - p(78)).max(1);
            let mut tabs = TabLayout::new(
                available,
                self.open.len(),
                p(88),
                p(180),
                self.tab_layout.get().offset,
            );
            tabs.viewport = (tabs.width * self.open.len() as i32).min(available).max(1);
            if let Some(active) = self.active {
                tabs.reveal(active);
            }
            self.tab_layout.set(tabs);
            self.publish_view();
            let editor_y = p(if tabs.overflow { 48 } else { 36 });
            let scroll_width = p(12);
            let scroll_height = if self.settings.wrap { 0 } else { p(12) };
            let editor_width = (w - content_x - scroll_width).max(0);
            let editor_height = (h - editor_y - p(32) - scroll_height).max(0);
            let mut positions = vec![
                (
                    self.toggle,
                    if self.sidebar_visible {
                        left - p(42)
                    } else {
                        p(6)
                    },
                    p(4),
                    p(30),
                    p(28),
                ),
                (
                    self.add,
                    p(12),
                    p(10),
                    p(134).min(left - p(60)).max(0),
                    p(34),
                ),
                (
                    self.tree_host,
                    p(8),
                    p(62),
                    (left - p(22)).max(0),
                    (h - p(70)).max(0),
                ),
                (
                    self.tree_scroll,
                    left - p(12),
                    p(62),
                    p(12),
                    (h - p(70)).max(0),
                ),
                (self.tabs, tabs_x, 0, tabs.viewport, p(36)),
                (self.tab_scroll, tabs_x, p(36), tabs.viewport, p(12)),
                (self.settings_button, w - p(36), p(3), p(30), p(29)),
                (
                    self.tab_add,
                    tabs_x + tabs.viewport + p(3),
                    p(3),
                    p(30),
                    p(29),
                ),
                (
                    self.editor_host,
                    content_x,
                    editor_y,
                    editor_width,
                    editor_height,
                ),
                (
                    self.editor_scroll[0],
                    w - scroll_width,
                    editor_y,
                    scroll_width,
                    editor_height,
                ),
                (
                    self.editor_scroll[1],
                    content_x,
                    h - p(32) - scroll_height,
                    editor_width,
                    scroll_height,
                ),
                (
                    self.status,
                    content_x,
                    (h - p(32)).max(0),
                    (w - content_x).max(0),
                    p(32),
                ),
            ];
            let mut batch = BeginDeferWindowPos(positions.len() as i32);
            for (hwnd, x, y, cx, cy) in positions.drain(..) {
                let mut current = RECT::default();
                GetWindowRect(hwnd, &mut current);
                let mut pt = POINT {
                    x: current.left,
                    y: current.top,
                };
                ScreenToClient(GetParent(hwnd), &mut pt);
                if pt.x == x
                    && pt.y == y
                    && current.right - current.left == cx
                    && current.bottom - current.top == cy
                {
                    continue;
                }
                if !batch.is_null() {
                    batch = DeferWindowPos(
                        batch,
                        hwnd,
                        null_mut(),
                        x,
                        y,
                        cx,
                        cy,
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW | SWP_NOCOPYBITS,
                    );
                }
            }
            if !batch.is_null() {
                EndDeferWindowPos(batch);
            }
            scrollbar::layout_body(self.edit);
            // A deferred batch may only contain siblings. The tree lives in a
            // clipping host so its native scrollbar can stay functional.
            let tree_width =
                (left - p(22)).max(0) + GetSystemMetricsForDpi(SM_CXVSCROLL, self.theme.dpi);
            let tree_height = (h - p(70)).max(0);
            let mut tree_area = RECT::default();
            GetWindowRect(self.tree, &mut tree_area);
            if tree_area.right - tree_area.left != tree_width
                || tree_area.bottom - tree_area.top != tree_height
            {
                SetWindowPos(
                    self.tree,
                    null_mut(),
                    0,
                    0,
                    tree_width,
                    tree_height,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW | SWP_NOCOPYBITS,
                );
            }
            let visible_rows = ((h - p(70)) / p(29).max(1)).max(1) as usize;
            for (hwnd, show) in [
                (self.add, self.sidebar_visible),
                (self.tree_host, self.sidebar_visible),
                (
                    self.tree_scroll,
                    self.sidebar_visible && self.visible.len() > visible_rows,
                ),
                (self.tab_scroll, tabs.overflow),
                (self.editor_clip, self.active.is_some()),
                (self.edit, self.active.is_some()),
                (self.editor_host, self.active.is_some()),
            ] {
                if (IsWindowVisible(hwnd) != 0) != show {
                    ShowWindow(hwnd, if show { SW_SHOWNA } else { SW_HIDE });
                }
            }
            self.layout_search();
            self.layout_tab_rename();
            ui::with_view(|view| view.refresh_editor_scroll());
            self.layout_settings();
            RedrawWindow(
                self.hwnd,
                null(),
                null_mut(),
                RDW_INVALIDATE
                    | RDW_ALLCHILDREN
                    | RDW_NOERASE
                    | if self.resizing { RDW_UPDATENOW } else { 0 },
            );
        }
    }
    unsafe fn layout_search(&self) {
        unsafe {
            self.tooltip.dismiss();
            if !self.find_visible || self.active.is_none() {
                if IsWindowVisible(self.search_host) != 0 {
                    ShowWindow(self.search_host, SW_HIDE);
                    RedrawWindow(
                        self.edit,
                        null(),
                        null_mut(),
                        RDW_INVALIDATE | RDW_UPDATENOW | RDW_NOERASE,
                    );
                }
                return;
            }
            let p = |n| self.theme.px(n);
            let mut client = RECT::default();
            GetClientRect(self.hwnd, &mut client);
            let left = if self.sidebar_visible {
                self.sidebar_edge(client.right) + 1
            } else {
                0
            };
            let width = p(480).min((client.right - left - p(24)).max(0));
            let height = p(if self.replace_visible { 210 } else { 122 });
            SetWindowPos(
                self.search_host,
                HWND_TOP,
                client.right - width - p(12),
                p(46),
                width,
                height,
                SWP_NOACTIVATE | SWP_NOREDRAW | SWP_NOCOPYBITS,
            );
            let positions = [
                (self.find_input, p(22), p(48), (width - p(48)).max(0), p(24)),
                (
                    self.replace_input,
                    p(22),
                    p(94),
                    (width - p(48)).max(0),
                    p(24),
                ),
                (self.find_case, width - p(143), p(8), p(28), p(28)),
                (self.find_prev, width - p(109), p(8), p(28), p(28)),
                (self.find_next, width - p(75), p(8), p(28), p(28)),
                (self.find_close, width - p(41), p(8), p(28), p(28)),
                (
                    self.replace_one,
                    p(16),
                    p(136),
                    (width - p(128)).clamp(p(52), p(88)),
                    p(30),
                ),
                (self.replace_all, width - p(110), p(136), p(94), p(30)),
            ];
            let mut batch = BeginDeferWindowPos(positions.len() as i32);
            for (control, x, y, w, h) in positions {
                if !batch.is_null() {
                    batch = DeferWindowPos(
                        batch,
                        control,
                        null_mut(),
                        x,
                        y,
                        w,
                        h,
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW | SWP_NOCOPYBITS,
                    );
                }
            }
            if !batch.is_null() {
                EndDeferWindowPos(batch);
            }
            for control in [self.replace_input, self.replace_one, self.replace_all] {
                ShowWindow(
                    control,
                    if self.replace_visible {
                        SW_SHOWNA
                    } else {
                        SW_HIDE
                    },
                );
            }
            ShowWindow(
                self.search_host,
                if self.find_visible && self.active.is_some() {
                    SW_SHOWNA
                } else {
                    SW_HIDE
                },
            );
            RedrawWindow(
                self.search_host,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW | RDW_NOERASE,
            );
            // Repaint the part of the document exposed when the panel closes or shrinks.
            InvalidateRect(self.edit, null(), 0);
        }
    }

    unsafe fn search_message(&mut self, message: &str) {
        self.find_message = message.to_owned();
        self.publish_view();
        unsafe {
            InvalidateRect(self.search_host, null(), 0);
        }
    }

    unsafe fn hover_tab_tooltip(&mut self, x: i32, y: i32) {
        unsafe {
            if GetCapture() == self.tab_scroll
                || self.dragging.is_some()
                || self.tab_rename.is_some()
                || y >= self.theme.px(32)
            {
                self.tooltip.hide();
                return;
            }
            let layout = self.tab_layout.get();
            if let Some(index) = layout.hit(x) {
                let (left, right) = layout.bounds(index);
                let id = self.open[index].id;
                if let Some(note) = self.book.get(id) {
                    self.tooltip.hover(
                        self.tabs,
                        id,
                        theme::rect(
                            left.max(0),
                            0,
                            right.min(layout.viewport),
                            self.theme.px(32),
                        ),
                        &note.title,
                        POINT { x, y },
                    );
                    return;
                }
            }
            self.tooltip.hide();
        }
    }

    unsafe fn hover_tree_tooltip(&mut self) {
        unsafe {
            if self.dragging.is_some() || SendMessageW(self.tree, TVM_GETEDITCONTROL, 0, 0) != 0 {
                self.tooltip.hide();
                return;
            }
            if let Some((id, _, handle)) = self.hit_tree() {
                let mut bounds = RECT::default();
                *(&mut bounds as *mut RECT as *mut HTREEITEM) = handle;
                SendMessageW(
                    self.tree,
                    TVM_GETITEMRECT,
                    0,
                    &mut bounds as *mut _ as isize,
                );
                let mut host = RECT::default();
                GetClientRect(self.tree_host, &mut host);
                bounds.left = 0;
                bounds.right = host.right;
                let mut point = POINT::default();
                GetCursorPos(&mut point);
                ScreenToClient(self.tree, &mut point);
                if let Some(note) = self.book.get(id) {
                    self.tooltip
                        .hover(self.tree, id, bounds, &note.title, point);
                    return;
                }
            }
            self.tooltip.hide();
        }
    }

    unsafe fn tab_click(&mut self, x: i32, y: i32) {
        unsafe {
            self.tooltip.hide();
            let layout = self.tab_layout.get();
            if let Some(index) = layout.hit(x) {
                if x >= layout.bounds(index).1 - self.theme.px(24) {
                    self.close_tab(index);
                } else {
                    if self.activate(index) {
                        self.tab_drag = Some(TabDrag {
                            id: self.open[index].id,
                            start: POINT { x, y },
                            moving: false,
                            gap: index,
                        });
                        self.publish_view();
                        SetCapture(self.tabs);
                    }
                }
            }
        }
    }

    unsafe fn update_tab_drag(&mut self, point: POINT) {
        unsafe {
            let Some(mut drag) = self.tab_drag else {
                return;
            };
            if !drag.moving {
                if (point.x - drag.start.x).abs()
                    < GetSystemMetricsForDpi(SM_CXDRAG, self.theme.dpi)
                    && (point.y - drag.start.y).abs()
                        < GetSystemMetricsForDpi(SM_CYDRAG, self.theme.dpi)
                {
                    return;
                }
                drag.moving = true;
                self.tooltip.hide();
                self.tab_hover.set(None);
                SetTimer(self.hwnd, TAB_DRAG_SCROLL_TIMER, 80, None);
            }
            drag.gap = self.tab_layout.get().insertion(point.x);
            self.tab_drag = Some(drag);
            self.publish_view();
            InvalidateRect(self.tabs, null(), 0);
            SetCursor(LoadCursorW(null_mut(), IDC_SIZEALL));
        }
    }

    unsafe fn cancel_tab_drag(&mut self) {
        self.tab_drag = None;
        self.publish_view();
        unsafe {
            KillTimer(self.hwnd, TAB_DRAG_SCROLL_TIMER);
            if GetCapture() == self.tabs {
                ReleaseCapture();
            }
            InvalidateRect(self.tabs, null(), 0);
        }
    }

    unsafe fn reorder_tab(&mut self, id: u64, gap: usize) -> std::io::Result<()> {
        let Some(from) = self.open.iter().position(|tab| tab.id == id) else {
            return Ok(());
        };
        let gap = gap.min(self.open.len());
        let to = gap - usize::from(gap > from);
        if from == to {
            return Ok(());
        }
        let old_active = self.active;
        let active_id = self.active.map(|i| self.open[i].id);
        let tab = self.open.remove(from);
        self.open.insert(to, tab);
        self.active = active_id.and_then(|id| self.open.iter().position(|tab| tab.id == id));
        if let Err(error) = self.save_session() {
            let tab = self.open.remove(to);
            self.open.insert(from, tab);
            self.active = old_active;
            return Err(error);
        }
        unsafe {
            self.layout();
        }
        Ok(())
    }

    unsafe fn finish_tab_drag(&mut self, point: POINT) {
        unsafe {
            let drag = self.tab_drag;
            let mut bounds = RECT::default();
            GetClientRect(self.tabs, &mut bounds);
            self.cancel_tab_drag();
            if let Some(drag) = drag.filter(|drag| drag.moving)
                && point.y >= 0
                && point.y < bounds.bottom
                && let Err(error) =
                    self.reorder_tab(drag.id, self.tab_layout.get().insertion(point.x))
            {
                report(self.hwnd, "탭 순서를 저장하지 못했습니다.", error);
            }
        }
    }

    fn visible_note_ids(&self) -> Vec<u64> {
        self.visible
            .iter()
            .filter_map(|handle| self.handles.get(handle).copied())
            .collect()
    }

    unsafe fn refresh_selection(&self) {
        self.publish_view();
        unsafe {
            InvalidateRect(self.tree, null(), 0);
        }
    }

    unsafe fn tree_mouse_down(&mut self, point: POINT, right: bool, ctrl: bool, shift: bool) {
        unsafe {
            self.tooltip.hide();
            let mut hit = TVHITTESTINFO {
                pt: point,
                ..Default::default()
            };
            SendMessageW(self.tree, TVM_HITTEST, 0, &mut hit as *mut _ as isize);
            if hit.flags & TVHT_ONITEMBUTTON != 0 {
                return;
            }
            let Some(&id) = self.handles.get(&hit.hItem) else {
                if !ctrl && !shift {
                    self.selection = Selection::default();
                    self.refresh_selection();
                }
                return;
            };
            let already = self.selection.ids.contains(&id);
            let preserve = already && (right || (!ctrl && !shift && self.selection.ids.len() > 1));
            if !preserve {
                let visible = if !right && shift {
                    self.visible_note_ids()
                } else {
                    Vec::new()
                };
                self.selection.click(
                    id,
                    &visible,
                    if right { false } else { ctrl },
                    if right { false } else { shift },
                );
            }
            if !right {
                self.tree_press = Some(TreePress {
                    id,
                    collapse: preserve,
                    focus_editor: !ctrl && !shift,
                    clicked: false,
                });
            }
            self.refresh_selection();
            SendMessageW(self.tree, TVM_SELECTITEM, TVGN_CARET as usize, hit.hItem);
            if !right && !ctrl && !shift {
                // TreeView's DragDetect can consume mouse-up before the subclass sees it.
                // Open on selection; only collapse a batch after a completed click.
                self.open_note(id);
            }
        }
    }

    unsafe fn tree_mouse_up(&mut self) {
        unsafe {
            if let Some(press) = self.tree_press.take()
                && self.dragging.is_none()
            {
                if press.collapse {
                    self.selection.single(press.id);
                    self.refresh_selection();
                }
                if press.focus_editor
                    && self
                        .active
                        .is_some_and(|index| self.open[index].id == press.id)
                {
                    SetFocus(self.edit);
                }
            }
        }
    }

    unsafe fn select_all_tree(&mut self) {
        self.selection.all(&self.visible_note_ids());
        unsafe {
            self.refresh_selection();
        }
    }
    unsafe fn scroll_tree(&self, rows: i32) {
        unsafe {
            self.tooltip.dismiss();
            let first = SendMessageW(self.tree, TVM_GETNEXTITEM, TVGN_FIRSTVISIBLE as usize, 0);
            let current = self.visible.iter().position(|&h| h == first).unwrap_or(0) as i32;
            let count = SendMessageW(self.tree, TVM_GETVISIBLECOUNT, 0, 0).max(1) as i32;
            let next =
                (current + rows).clamp(0, (self.visible.len() as i32 - count).max(0)) as usize;
            if let Some(&item) = self.visible.get(next) {
                SendMessageW(self.tree, TVM_SELECTITEM, TVGN_FIRSTVISIBLE as usize, item);
            }
            InvalidateRect(self.tree_scroll, null(), 0);
        }
    }
    unsafe fn set_scroll(&self, kind: Kind, target: i32) {
        unsafe {
            self.tooltip.dismiss();
            match kind {
                Kind::Dropdown { list, .. } => scrollbar::set_list_position(list, target),
                Kind::EditorVertical | Kind::EditorHorizontal => {
                    scrollbar::set_body_position(self.edit, kind.horizontal(), target);
                    ui::with_view(|v| v.refresh_editor_scroll());
                }
                Kind::Tree => {
                    let first =
                        SendMessageW(self.tree, TVM_GETNEXTITEM, TVGN_FIRSTVISIBLE as usize, 0);
                    let current = self.visible.iter().position(|&h| h == first).unwrap_or(0) as i32;
                    self.scroll_tree(target - current);
                }
                Kind::Tabs => {
                    let mut tabs = self.tab_layout.get();
                    tabs.scroll(target - tabs.offset);
                    self.tab_layout.set(tabs);
                    self.publish_view();
                    self.layout_tab_rename();
                    InvalidateRect(self.tabs, null(), 0);
                    InvalidateRect(self.tab_scroll, null(), 0);
                }
            }
        }
    }

    unsafe fn set_status(&self, text: &str) {
        unsafe {
            *self.status_text.borrow_mut() = text.into();
            self.publish_view();
            InvalidateRect(self.status, null(), 0);
        }
    }

    unsafe fn selected_id(&self) -> Option<u64> {
        let handle = unsafe { SendMessageW(self.tree, TVM_GETNEXTITEM, TVGN_CARET as usize, 0) };
        self.handles.get(&handle).copied()
    }

    unsafe fn rebuild_tree(&mut self, select: Option<u64>) {
        let _timing = startup::span("tree.build");
        self.selection.retain(|id| self.book.get(id).is_some());
        if let Some(id) = select
            && !self.selection.ids.contains(&id)
        {
            self.selection.single(id);
        }
        unsafe {
            SendMessageW(self.tree, WM_SETREDRAW, 0, 0);
            SendMessageW(self.tree, TVM_DELETEITEM, 0, TVI_ROOT as LPARAM);
        }
        self.handles.clear();
        unsafe {
            self.insert_children(0, TVI_ROOT);
        }
        if let Some(id) = select
            && let Some((&handle, _)) = self.handles.iter().find(|&(_, &value)| value == id)
        {
            unsafe {
                SendMessageW(self.tree, TVM_SELECTITEM, TVGN_CARET as usize, handle);
            }
        }
        self.sync_tree_view();
        unsafe {
            SendMessageW(self.tree, WM_SETREDRAW, 1, 0);
            InvalidateRect(self.tree, null(), 0);
            PostMessageW(self.hwnd, REFRESH_UI, 0, 0);
        }
    }

    unsafe fn insert_children(&mut self, parent_id: u64, parent_handle: HTREEITEM) {
        for id in self.book.children(parent_id) {
            let title = self.book.get(id).unwrap().title.clone();
            let mut label = wide(&title);
            let mut insert = TVINSERTSTRUCTW {
                hParent: parent_handle,
                hInsertAfter: TVI_LAST,
                ..Default::default()
            };
            insert.Anonymous.item = TVITEMW {
                mask: TVIF_TEXT | TVIF_PARAM,
                pszText: label.as_mut_ptr(),
                lParam: id as LPARAM,
                ..Default::default()
            };
            let handle = unsafe {
                SendMessageW(self.tree, TVM_INSERTITEMW, 0, &insert as *const _ as LPARAM)
            } as HTREEITEM;
            self.handles.insert(handle as isize, id);
            unsafe {
                self.insert_children(id, handle);
            }
            if self.expanded.contains(&id) {
                unsafe {
                    SendMessageW(self.tree, TVM_EXPAND, TVE_EXPAND as usize, handle as LPARAM);
                }
            }
        }
    }

    unsafe fn current_text(&self) -> String {
        unsafe { window_text(self.edit) }
    }

    unsafe fn capture_edit(&mut self) {
        if let Some(index) = self.active {
            let text = unsafe { self.current_text() };
            if text != self.open[index].text {
                self.open[index].text = text;
                self.open[index].dirty = true;
                unsafe {
                    SetTimer(self.hwnd, AUTOSAVE_TIMER, 650, None);
                }
            }
        }
    }

    unsafe fn save_all(&mut self) -> bool {
        unsafe {
            self.capture_edit();
        }
        for tab in &mut self.open {
            if tab.dirty {
                let file = self.book.file(&self.data_dir, tab.id);
                if let Err(error) = replace_file(&file, tab.text.as_bytes()) {
                    report(self.hwnd, "메모를 저장하지 못했습니다.", error);
                    return false;
                }
                tab.dirty = false;
            }
        }
        unsafe {
            KillTimer(self.hwnd, AUTOSAVE_TIMER);
            self.set_status("저장됨");
        }
        true
    }

    unsafe fn activate(&mut self, index: usize) -> bool {
        let _timing = startup::span("tab.activate");
        if index >= self.open.len() {
            return false;
        }
        unsafe {
            if let Err(error) = self.finish_tab_rename(true, false) {
                report(self.hwnd, "메모 이름을 저장하지 못했습니다.", error);
                return false;
            }
            if self.selection.ids.len() <= 1 {
                self.selection.single(self.open[index].id);
                self.refresh_selection();
            }
            if self.active == Some(index) {
                SetFocus(self.edit);
                return true;
            }
            if !self.load_editor(index) {
                return false;
            }
            self.capture_edit();
            ShowWindow(self.edit, SW_HIDE);
            self.active = Some(index);
            self.edit = self.open[index].editor;
            self.last_match = None;
            self.publish_view();
            self.layout();
            ShowWindow(self.edit, SW_SHOWNA);
            if let Some((&handle, _)) = self
                .handles
                .iter()
                .find(|&(_, &id)| id == self.open[index].id)
            {
                SendMessageW(self.tree, TVM_SELECTITEM, TVGN_CARET as usize, handle);
            }
            // Selecting a tab can expand its ancestors synchronously, while
            // application state is borrowed and expansion notifications defer.
            if self.refresh_tree_visibility() {
                self.layout();
            }
            InvalidateRect(self.tree_scroll, null(), 0);
            SetFocus(self.edit);
            self.set_status(if self.open[index].dirty {
                "저장 중…"
            } else {
                "저장됨"
            });
            InvalidateRect(self.tabs, null(), 0);
        }
        true
    }

    unsafe fn open_note(&mut self, id: u64) {
        if let Some(index) = self.open.iter().position(|t| t.id == id) {
            unsafe {
                self.activate(index);
            }
            return;
        }
        let index = self.open.len();
        self.open.push(OpenTab {
            id,
            text: String::new(),
            dirty: false,
            editor: null_mut(),
        });
        unsafe {
            if !self.activate(index) {
                self.open.pop();
                self.layout();
            }
        }
    }

    unsafe fn load_editor(&mut self, index: usize) -> bool {
        if !self.open[index].editor.is_null() {
            return true;
        }
        let file = self.book.file(&self.data_dir, self.open[index].id);
        let read_timing = startup::span("note.read");
        let text = match fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) => {
                report(self.hwnd, "메모를 열지 못했습니다.", error);
                return false;
            }
        };
        drop(read_timing);
        let editor = unsafe { memo_editor(self.editor_clip) };
        if editor.is_null() {
            report(
                self.hwnd,
                "편집 창을 만들지 못했습니다.",
                std::io::Error::last_os_error(),
            );
            return false;
        }
        self.loading = true;
        let text_timing = startup::span("editor.load_text");
        unsafe {
            self.apply_editor_settings(editor, true, true, true);
            SetWindowTextW(editor, wide(&text).as_ptr());
            SendMessageW(editor, EM_EMPTYUNDOBUFFER, 0, 0);
        }
        self.loading = false;
        self.open[index].text = unsafe { window_text(editor) };
        self.open[index].editor = editor;
        drop(text_timing);
        true
    }

    unsafe fn close_active(&mut self) {
        let Some(index) = self.active else {
            return;
        };
        unsafe {
            self.close_tab(index);
        }
    }

    unsafe fn close_tab(&mut self, index: usize) {
        unsafe { self.close_tabs(index, MENU_TAB_CLOSE) };
    }

    unsafe fn close_tabs(&mut self, index: usize, command: usize) {
        if index >= self.open.len() {
            return;
        }
        let closes = |candidate: usize| match command {
            MENU_TAB_CLOSE => candidate == index,
            MENU_TAB_CLOSE_OTHERS => candidate != index,
            MENU_TAB_CLOSE_RIGHT => candidate > index,
            _ => false,
        };
        if !(0..self.open.len()).any(closes) {
            return;
        }
        if let Err(error) = unsafe { self.finish_tab_rename(true, false) } {
            report(self.hwnd, "메모 이름을 저장하지 못했습니다.", error);
            return;
        }
        if !unsafe { self.save_all() } {
            return;
        }
        let previous_active = self.active;
        let active_id = previous_active.map(|active| self.open[active].id);
        // Remove backwards so the menu target and predicate keep their original indices.
        for candidate in (0..self.open.len()).rev() {
            if closes(candidate) {
                let closed = self.open.remove(candidate);
                if self.edit == closed.editor {
                    self.edit = self.empty_edit;
                }
                unsafe {
                    if !closed.editor.is_null() {
                        DestroyWindow(closed.editor);
                    }
                }
            }
        }

        self.active = None;
        if self.open.is_empty() {
            self.loading = true;
            unsafe {
                SetWindowTextW(self.edit, wide("").as_ptr());
                EnableWindow(self.edit, 0);
                ShowWindow(self.edit, SW_HIDE);
                self.set_status("준비됨");
                InvalidateRect(self.hwnd, null(), 0);
                InvalidateRect(self.tabs, null(), 0);
            }
            self.loading = false;
            unsafe {
                self.layout();
            }
        } else {
            let next = active_id
                .and_then(|id| self.open.iter().position(|tab| tab.id == id))
                .unwrap_or_else(|| index.min(self.open.len() - 1));
            unsafe {
                if !self.activate(next) {
                    ShowWindow(self.edit, SW_HIDE);
                    self.layout();
                }
            }
        }
    }

    unsafe fn create_note(&mut self, parent: u64) {
        let previous = self.book.clone();
        let id = self.book.add(parent, "새 메모".to_owned());
        let file = self.book.file(&self.data_dir, id);
        if let Err(error) = fs::write(&file, b"") {
            self.book.remove(id);
            report(self.hwnd, "메모를 만들지 못했습니다.", error);
            return;
        }
        if let Err(error) = self.book.save(&self.data_dir) {
            self.book = previous;
            let _ = fs::remove_file(file);
            report(self.hwnd, "메모 목록을 저장하지 못했습니다.", error);
            return;
        }
        self.expanded.insert(parent);
        unsafe {
            self.rebuild_tree(Some(id));
            self.open_note(id);
            self.rename_selected();
        }
    }

    unsafe fn rename_selected(&self) {
        let editor = unsafe { SendMessageW(self.tree, TVM_GETEDITCONTROL, 0, 0) } as HWND;
        if !editor.is_null() {
            unsafe {
                SetFocus(editor);
            }
            return;
        }
        let handle = unsafe { SendMessageW(self.tree, TVM_GETNEXTITEM, TVGN_CARET as usize, 0) };
        if handle != 0 {
            unsafe {
                SetFocus(self.tree);
                SendMessageW(self.tree, TVM_EDITLABELW, 0, handle);
                PostMessageW(self.hwnd, POSITION_RENAME, 0, 0);
            }
        }
    }

    unsafe fn rename_current(&mut self, focus: HWND) {
        unsafe {
            if !self.sidebar_visible {
                self.rename_tab(focus);
                return;
            }
            if self.selection.ids.len() > 1 {
                return;
            }
            if let Err(error) = self.finish_tab_rename(true, false) {
                report(self.hwnd, "메모 이름을 저장하지 못했습니다.", error);
                return;
            }
            self.tooltip.hide();
            self.rename_selected();
        }
    }

    unsafe fn rename_tab(&mut self, focus: HWND) {
        unsafe {
            if self.tab_rename.is_some() {
                SetFocus(self.tab_name_edit);
                return;
            }
            let Some(index) = self.active else {
                return;
            };
            let id = self.open[index].id;
            let Some(note) = self.book.get(id) else {
                return;
            };
            let title = wide(&note.title);
            if self.tab_name_edit.is_null() {
                self.tab_name_edit = child(
                    self.tabs,
                    windows_sys::w!("EDIT"),
                    "",
                    WS_CHILD | WS_TABSTOP | WS_CLIPSIBLINGS | ES_AUTOHSCROLL as u32,
                    0,
                    ID_TAB_NAME,
                );
                if self.tab_name_edit.is_null() {
                    report(
                        self.hwnd,
                        "이름 편집기를 만들지 못했습니다.",
                        std::io::Error::last_os_error(),
                    );
                    return;
                }
                SetWindowSubclass(self.tab_name_edit, Some(tab_name_subclass), 1, 0);
            }
            self.tooltip.hide();
            self.cancel_tab_drag();
            self.tab_rename = Some(TabRename { id, focus });
            let mut tabs = self.tab_layout.get();
            tabs.reveal(index);
            self.tab_layout.set(tabs);
            self.publish_view();
            SetWindowTextW(self.tab_name_edit, title.as_ptr());
            SendMessageW(self.tab_name_edit, WM_SETFONT, self.theme.tab as usize, 0);
            SendMessageW(
                self.tab_name_edit,
                EM_SETMARGINS,
                EC_LEFTMARGIN as usize | EC_RIGHTMARGIN as usize,
                0,
            );
            self.layout_tab_rename();
            ShowWindow(self.tab_name_edit, SW_SHOWNA);
            SetFocus(self.tab_name_edit);
            SendMessageW(self.tab_name_edit, EM_SETSEL, 0, -1);
            RedrawWindow(
                self.tabs,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN | RDW_NOERASE,
            );
            InvalidateRect(self.tab_scroll, null(), 0);
        }
    }

    unsafe fn layout_tab_rename(&self) {
        unsafe {
            let Some(rename) = self.tab_rename else {
                return;
            };
            let Some(index) = self.open.iter().position(|tab| tab.id == rename.id) else {
                return;
            };
            let layout = self.tab_layout.get();
            let (left, right) = layout.bounds(index);
            let p = |v| self.theme.px(v);
            SetWindowPos(
                self.tab_name_edit,
                null_mut(),
                left + p(33),
                p(9),
                (right - left - p(61)).max(1),
                p(18),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    unsafe fn finish_tab_rename(
        &mut self,
        commit: bool,
        restore_focus: bool,
    ) -> std::io::Result<()> {
        let Some(rename) = self.tab_rename.take() else {
            return Ok(());
        };
        unsafe {
            let title = if commit {
                window_text(self.tab_name_edit)
            } else {
                String::new()
            };
            let had_focus = GetFocus() == self.tab_name_edit;
            self.publish_view();
            ShowWindow(self.tab_name_edit, SW_HIDE);
            let result = if commit {
                self.rename_note(rename.id, &title)
            } else {
                Ok(())
            };
            InvalidateRect(self.tabs, null(), 0);
            if restore_focus && had_focus {
                let focus = if IsWindow(rename.focus) != 0 && IsWindowVisible(rename.focus) != 0 {
                    rename.focus
                } else {
                    self.edit
                };
                SetFocus(focus);
            }
            result
        }
    }

    fn rename_note(&mut self, id: u64, title: &str) -> std::io::Result<()> {
        let title = title.trim();
        let Some(note) = self.book.get_mut(id) else {
            return Ok(());
        };
        if title.is_empty() || note.title == title {
            return Ok(());
        }
        let previous = std::mem::replace(&mut note.title, title.to_owned());
        if let Err(error) = self.book.save(&self.data_dir) {
            self.book.get_mut(id).unwrap().title = previous;
            return Err(error);
        }
        if let Some((&handle, _)) = self.handles.iter().find(|&(_, &value)| value == id) {
            let mut label = wide(title);
            let item = TVITEMW {
                mask: TVIF_TEXT,
                hItem: handle,
                pszText: label.as_mut_ptr(),
                ..Default::default()
            };
            unsafe {
                SendMessageW(self.tree, TVM_SETITEMW, 0, &item as *const _ as isize);
            }
        }
        self.sync_tree_view();
        unsafe {
            InvalidateRect(self.tree, null(), 0);
            InvalidateRect(self.tabs, null(), 0);
        }
        Ok(())
    }

    unsafe fn layout_rename(&self) {
        unsafe {
            let editor = SendMessageW(self.tree, TVM_GETEDITCONTROL, 0, 0) as HWND;
            let handle = SendMessageW(self.tree, TVM_GETNEXTITEM, TVGN_CARET as usize, 0);
            if editor.is_null() || handle == 0 {
                return;
            }
            let mut label = RECT::default();
            *(&mut label as *mut RECT as *mut HTREEITEM) = handle;
            SendMessageW(
                self.tree,
                TVM_GETITEMRECT,
                1,
                &mut label as *mut _ as LPARAM,
            );
            let mut area = RECT::default();
            GetClientRect(self.tree, &mut area);
            let x = label.left + self.theme.px(25);
            SendMessageW(editor, WM_SETFONT, self.theme.ui as usize, 1);
            MoveWindow(
                editor,
                x,
                label.top + 1,
                (area.right - x - self.theme.px(18)).max(self.theme.px(30)),
                (label.bottom - label.top - 2).max(1),
                1,
            );
            InvalidateRect(self.tree, null(), 0);
        }
    }

    unsafe fn delete_selected(&mut self) {
        let selected: Vec<_> = self.selection.ids.iter().copied().collect();
        let ids = self.book.selected_subtrees(&selected);
        if ids.is_empty() {
            return;
        }
        let prompt = format!(
            "선택한 항목과 하위 메모 총 {}개를 삭제할까요?\n삭제한 파일은 휴지통 폴더에 보관됩니다.",
            ids.len()
        );
        if unsafe { message(self.hwnd, "메모 삭제", &prompt, MB_YESNO | MB_ICONQUESTION) } != IDYES
        {
            return;
        }
        if !unsafe { self.save_all() } {
            return;
        }
        if let Err(error) = unsafe { self.delete_notes_confirmed(&selected) } {
            report(self.hwnd, "메모를 삭제하지 못했습니다.", error);
        }
    }

    unsafe fn delete_notes_confirmed(&mut self, selected: &[u64]) -> std::io::Result<()> {
        let ids = self.book.trash_notes(&self.data_dir, selected)?;
        if ids.is_empty() {
            return Ok(());
        }
        let old_index = self.active.unwrap_or(0);
        let active_id = self.active.map(|i| self.open[i].id);
        let deleted: HashSet<_> = ids.into_iter().collect();
        if self
            .tab_rename
            .is_some_and(|rename| deleted.contains(&rename.id))
        {
            unsafe {
                self.finish_tab_rename(false, false)?;
            }
        }
        self.open.retain(|tab| {
            if deleted.contains(&tab.id) {
                if self.edit == tab.editor {
                    self.edit = self.empty_edit;
                }
                unsafe {
                    if !tab.editor.is_null() {
                        DestroyWindow(tab.editor);
                    }
                }
                false
            } else {
                true
            }
        });
        self.selection.retain(|id| !deleted.contains(&id));
        self.expanded.retain(|id| !deleted.contains(id));
        let surviving = active_id.and_then(|id| self.open.iter().position(|t| t.id == id));
        self.active = surviving;
        let next = surviving
            .or_else(|| (!self.open.is_empty()).then(|| old_index.min(self.open.len() - 1)));
        unsafe {
            if let Some(next) = next {
                let id = self.open[next].id;
                self.rebuild_tree(Some(id));
                if surviving.is_some() {
                    self.layout();
                } else {
                    self.activate(next);
                }
            } else {
                self.edit = self.empty_edit;
                self.loading = true;
                SetWindowTextW(self.edit, wide("").as_ptr());
                EnableWindow(self.edit, 0);
                self.loading = false;
                self.rebuild_tree(None);
                self.set_status("준비됨");
                self.layout();
            }
        }
        Ok(())
    }

    fn group_selected_notes(&mut self) -> std::io::Result<Option<u64>> {
        if self.selection.ids.len() < 2 {
            return Ok(None);
        }
        let selected: Vec<_> = self.selection.ids.iter().copied().collect();
        let previous = self.book.clone();
        let Some(folder) = self.book.group_notes(&selected) else {
            return Ok(None);
        };
        let file = self.book.file(&self.data_dir, folder);
        if let Err(error) = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
        {
            self.book = previous;
            return Err(error);
        }
        if let Err(error) = self.book.save(&self.data_dir) {
            self.book = previous;
            let _ = fs::remove_file(file);
            return Err(error);
        }
        self.expanded.insert(folder);
        let mut parent = self.book.get(folder).unwrap().parent;
        while parent != 0 {
            self.expanded.insert(parent);
            parent = self.book.get(parent).unwrap().parent;
        }
        unsafe {
            self.rebuild_tree(Some(folder));
            self.layout();
        }
        Ok(Some(folder))
    }

    unsafe fn shift_selected(&mut self, direction: i32) {
        let Some(id) = (unsafe { self.selected_id() }) else {
            return;
        };
        let parent = self.book.get(id).unwrap().parent;
        let siblings = self.book.children(parent);
        let Some(pos) = siblings.iter().position(|&n| n == id) else {
            return;
        };
        let before = if direction < 0 {
            if pos == 0 {
                return;
            }
            Some(siblings[pos - 1])
        } else {
            if pos + 1 >= siblings.len() {
                return;
            }
            siblings.get(pos + 2).copied()
        };
        let previous = self.book.clone();
        if self.book.move_note(id, parent, before) {
            unsafe {
                self.persist_tree(previous, Some(id));
            }
        }
    }

    unsafe fn outdent_selected(&mut self) {
        let Some(id) = (unsafe { self.selected_id() }) else {
            return;
        };
        let parent = self.book.get(id).unwrap().parent;
        if parent == 0 {
            return;
        }
        let grandparent = self.book.get(parent).unwrap().parent;
        let siblings = self.book.children(grandparent);
        let pos = siblings.iter().position(|&n| n == parent).unwrap();
        let before = siblings.get(pos + 1).copied();
        let previous = self.book.clone();
        if self.book.move_note(id, grandparent, before) {
            unsafe {
                self.persist_tree(previous, Some(id));
            }
        }
    }

    unsafe fn persist_tree(&mut self, previous: Notebook, select: Option<u64>) -> bool {
        if let Err(error) = self.book.save(&self.data_dir) {
            self.book = previous;
            report(self.hwnd, "메모 목록을 저장하지 못했습니다.", error);
            unsafe {
                self.rebuild_tree(select);
            }
            return false;
        }
        unsafe {
            self.rebuild_tree(select);
        }
        true
    }

    unsafe fn hit_tree(&self) -> Option<(u64, i32, HTREEITEM)> {
        unsafe {
            let mut pt = POINT::default();
            GetCursorPos(&mut pt);
            ScreenToClient(self.tree, &mut pt);
            let mut hit = TVHITTESTINFO {
                pt,
                flags: 0,
                hItem: 0,
            };
            SendMessageW(self.tree, TVM_HITTEST, 0, &mut hit as *mut _ as LPARAM);
            self.handles
                .get(&hit.hItem)
                .map(|&id| (id, pt.y, hit.hItem))
        }
    }

    unsafe fn drag_target_at_pointer(&self) -> Option<DropTarget> {
        self.dragging?;
        unsafe {
            let mut point = POINT::default();
            let mut bounds = RECT::default();
            GetCursorPos(&mut point);
            GetWindowRect(self.tree_host, &mut bounds);
            if !self.sidebar_visible || PtInRect(&bounds, point) == 0 {
                return None;
            }
            if let Some((target, y, handle)) = self.hit_tree() {
                let mut row = RECT::default();
                *(&mut row as *mut RECT as *mut HTREEITEM) = handle;
                SendMessageW(self.tree, TVM_GETITEMRECT, 1, &mut row as *mut _ as isize);
                let height = (row.bottom - row.top).max(1);
                let zone = if y < row.top + height / 4 {
                    DropZone::Before
                } else if y >= row.bottom - height / 4 {
                    DropZone::After
                } else {
                    DropZone::Inside
                };
                DropTarget::resolve_many(&self.book, &self.drag_sources, Some(target), zone)
            } else {
                DropTarget::resolve_many(&self.book, &self.drag_sources, None, DropZone::Root)
            }
        }
    }

    unsafe fn begin_tree_drag(&mut self, id: u64) {
        if !self.selection.ids.contains(&id) {
            self.selection.single(id);
        }
        let selected: Vec<_> = self.selection.ids.iter().copied().collect();
        self.drag_sources = Rc::new(self.book.selected_roots(&selected));
        if self.drag_sources.is_empty() {
            return;
        }
        self.dragging = Some(id);
        self.tree_press = None;
        self.tree_hover = None;
        self.drop_target = None;
        self.publish_view();
        unsafe {
            self.tooltip.hide();
            SetCapture(self.hwnd);
            SetTimer(self.hwnd, DRAG_SCROLL_TIMER, 160, None);
            self.update_drag_feedback();
            self.repaint_drag_feedback();
        }
    }

    unsafe fn update_drag_feedback(&mut self) {
        unsafe {
            let target = self.drag_target_at_pointer();
            if self.drop_target != target {
                self.drop_target = target;
                self.publish_view();
                self.repaint_drag_feedback();
            }
            SetCursor(LoadCursorW(
                null_mut(),
                if target.is_some() {
                    IDC_SIZEALL
                } else {
                    IDC_NO
                },
            ));
        }
    }

    unsafe fn repaint_drag_feedback(&self) {
        unsafe {
            for control in [self.tree, self.status] {
                RedrawWindow(
                    control,
                    null(),
                    null_mut(),
                    RDW_INVALIDATE | RDW_UPDATENOW | RDW_NOERASE,
                );
            }
        }
    }

    unsafe fn cancel_interaction(&mut self) {
        self.dragging = None;
        self.drag_sources = Rc::new(Vec::new());
        self.tree_press = None;
        self.drop_target = None;
        self.tree_hover = None;
        self.resizing = false;
        self.publish_view();
        unsafe {
            self.cancel_tab_drag();
            KillTimer(self.hwnd, DRAG_SCROLL_TIMER);
            for control in [
                self.editor_scroll[0],
                self.editor_scroll[1],
                self.tree_scroll,
                self.tab_scroll,
            ] {
                SendMessageW(control, WM_CANCELMODE, 0, 0);
            }
            if GetCapture() == self.hwnd
                || [
                    self.editor_scroll[0],
                    self.editor_scroll[1],
                    self.tree_scroll,
                    self.tab_scroll,
                ]
                .contains(&GetCapture())
            {
                ReleaseCapture();
            }
            SetCursor(LoadCursorW(null_mut(), IDC_ARROW));
            self.repaint_drag_feedback();
        }
    }

    unsafe fn finish_drag(&mut self) {
        let target = unsafe { self.drag_target_at_pointer() };
        unsafe {
            self.finish_drag_to(target);
        }
    }

    unsafe fn finish_drag_to(&mut self, target: Option<DropTarget>) {
        let source = self.dragging;
        let sources = Rc::clone(&self.drag_sources);
        unsafe {
            self.cancel_interaction();
        }
        if let (Some(id), Some(target)) = (source, target) {
            let previous = self.book.clone();
            if self.book.move_notes(&sources, target.parent, target.before) {
                self.expanded.insert(target.parent);
                unsafe {
                    self.persist_tree(previous, Some(id));
                }
            }
        }
    }

    unsafe fn tab_context_menu(&mut self, coordinates: LPARAM) {
        unsafe {
            let keyboard = coordinates == -1;
            let mut point = POINT {
                x: point_x(coordinates),
                y: point_y(coordinates),
            };
            let index = if keyboard {
                let Some(index) = self.active else { return };
                let (left, right) = self.tab_layout.get().bounds(index);
                point = POINT {
                    x: ((left + right) / 2).clamp(0, (self.tab_layout.get().viewport - 1).max(0)),
                    y: self.theme.px(34),
                };
                ClientToScreen(self.tabs, &mut point);
                index
            } else {
                let mut client = point;
                ScreenToClient(self.tabs, &mut client);
                let Some(index) = self.tab_layout.get().hit(client.x) else {
                    return;
                };
                index
            };
            self.tooltip.hide();
            self.tab_hover.set(None);
            InvalidateRect(self.tabs, null(), 0);
            let menu = tab_menu(index, self.open.len());
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                self.hwnd,
                null(),
            );
            DestroyMenu(menu);
            if chosen != 0 {
                self.close_tabs(index, chosen as usize);
            }
        }
    }

    unsafe fn context_menu(&mut self, coordinates: LPARAM) {
        unsafe {
            let mut point = POINT {
                x: point_x(coordinates),
                y: point_y(coordinates),
            };
            if coordinates != -1 {
                let mut client = point;
                ScreenToClient(self.tree, &mut client);
                let mut hit = TVHITTESTINFO {
                    pt: client,
                    ..Default::default()
                };
                SendMessageW(self.tree, TVM_HITTEST, 0, &mut hit as *mut _ as isize);
                if !self.handles.contains_key(&hit.hItem) {
                    return;
                }
                let clicked = self.handles[&hit.hItem];
                if !self.selection.ids.contains(&clicked) {
                    self.selection.single(clicked);
                }
                self.refresh_selection();
                SendMessageW(
                    self.tree,
                    TVM_SELECTITEM,
                    TVGN_CARET as usize,
                    hit.hItem as LPARAM,
                );
            } else {
                let handle = SendMessageW(self.tree, TVM_GETNEXTITEM, TVGN_CARET as usize, 0);
                if self.selection.ids.is_empty() {
                    return;
                }
                if !self.handles.contains_key(&handle) {
                    return;
                }
                let id = self.handles[&handle];
                if !self.selection.ids.contains(&id) {
                    self.selection.single(id);
                    self.refresh_selection();
                }
                let mut area = RECT::default();
                *(&mut area as *mut RECT as *mut isize) = handle;
                if SendMessageW(self.tree, TVM_GETITEMRECT, 1, &mut area as *mut _ as isize) == 0 {
                    return;
                }
                point = POINT {
                    x: area.left + self.theme.px(12),
                    y: area.bottom,
                };
                ClientToScreen(self.tree, &mut point);
            }
            let Some(note) = self.selected_id().and_then(|id| self.book.get(id)) else {
                return;
            };
            let menu = if self.selection.ids.len() > 1 {
                sidebar_multi_menu()
            } else {
                sidebar_menu(note.parent != 0)
            };
            self.tooltip.hide();
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                self.hwnd,
                null(),
            );
            DestroyMenu(menu);
            if chosen != 0 {
                self.command(chosen as usize, 0);
            }
        }
    }

    unsafe fn show_find(&mut self, replace: bool) {
        if self.settings_visible {
            unsafe {
                self.show_settings(false);
            }
        }
        if self.active.is_none() {
            return;
        }
        unsafe {
            let selection = editing::selection(self.edit);
            if selection.end > selection.start && selection.end - selection.start <= 200 {
                let mut buffer = vec![0u16; (selection.end - selection.start + 1) as usize];
                let used = SendMessageW(self.edit, WM_USER + 62, 0, buffer.as_mut_ptr() as isize)
                    .max(0) as usize;
                let selected = String::from_utf16_lossy(&buffer[..used]);
                if !selected.contains(['\r', '\n']) {
                    SetWindowTextW(self.find_input, wide(&selected).as_ptr());
                }
            }
            self.find_visible = true;
            self.replace_visible = replace;
            self.last_match = None;
            self.find_message = "Enter로 다음 결과를 찾습니다".into();
            SendMessageW(self.edit, editing::EM_STOPGROUPTYPING, 0, 0);
            self.publish_view();
            self.layout_search();
            SetFocus(self.find_input);
            SendMessageW(self.find_input, EM_SETSEL, 0, -1);
        }
    }
    unsafe fn hide_find(&mut self) {
        self.find_visible = false;
        self.replace_visible = false;
        unsafe {
            self.publish_view();
            self.layout_search();
            SetFocus(self.edit);
        }
    }
    unsafe fn find_next(&mut self, backwards: bool, live: bool) {
        if self.active.is_none() {
            return;
        }
        unsafe {
            let query = window_text(self.find_input);
            if query.is_empty() {
                self.last_match = None;
                self.search_message("검색어를 입력하세요");
                self.set_status("검색어를 입력하세요");
                return;
            }
            let case = self.find_case_sensitive;
            let selection = editing::selection(self.edit);
            let start = if backwards || live {
                selection.start
            } else {
                selection.end
            };
            let end = if backwards { 0 } else { -1 };
            let result =
                editing::find(self.edit, &query, start, end, backwards, case).or_else(|| {
                    editing::find(
                        self.edit,
                        &query,
                        if backwards {
                            GetWindowTextLengthW(self.edit)
                        } else {
                            0
                        },
                        end,
                        backwards,
                        case,
                    )
                });
            self.last_query = query;
            self.last_match = result;
            if let Some(range) = result {
                editing::select(self.edit, range);
                SendMessageW(self.edit, EM_SCROLLCARET, 0, 0);
                self.set_status("검색 결과 선택됨");
                self.search_message("일치하는 텍스트를 찾았습니다");
            } else {
                self.set_status("찾을 수 없음");
                self.search_message("일치하는 텍스트가 없습니다");
            }
        }
    }
    unsafe fn replace_one(&mut self) {
        if self.active.is_none() {
            return;
        }
        unsafe {
            let range = editing::selection(self.edit);
            let query = window_text(self.find_input);
            let case = self.find_case_sensitive;
            if !query.is_empty()
                && range.end > range.start
                && editing::find(self.edit, &query, range.start, range.end, false, case)
                    == Some(range)
            {
                let replacement = window_text(self.replace_input);
                SendMessageW(self.edit, editing::EM_STOPGROUPTYPING, 0, 0);
                SendMessageW(
                    self.edit,
                    EM_REPLACESEL,
                    1,
                    wide(&replacement).as_ptr() as isize,
                );
                self.capture_edit();
            }
            self.find_next(false, false);
        }
    }
    unsafe fn replace_all(&mut self) {
        if self.active.is_none() {
            return;
        }
        unsafe {
            let query = window_text(self.find_input);
            if query.is_empty() {
                return;
            }
            let case = self.find_case_sensitive;
            let mut ranges = Vec::new();
            let mut start = 0;
            while let Some(range) = editing::find(self.edit, &query, start, -1, false, case) {
                if range.end <= start {
                    break;
                }
                start = range.end;
                ranges.push(range);
            }
            if ranges.is_empty() {
                self.set_status("찾을 수 없음");
                self.search_message("일치하는 텍스트가 없습니다");
                return;
            }
            let text = editing::replace_ranges(
                &self.current_text(),
                &ranges,
                &window_text(self.replace_input),
            );
            SendMessageW(self.edit, editing::EM_STOPGROUPTYPING, 0, 0);
            SendMessageW(self.edit, WM_SETREDRAW, 0, 0);
            editing::select(self.edit, editing::CharRange { start: 0, end: -1 });
            SendMessageW(self.edit, EM_REPLACESEL, 1, wide(&text).as_ptr() as isize);
            editing::select(self.edit, editing::CharRange::default());
            SendMessageW(self.edit, WM_SETREDRAW, 1, 0);
            InvalidateRect(self.edit, null(), 0);
            self.capture_edit();
            self.set_status(&format!("{}개 바꿈", ranges.len()));
            self.search_message(&format!("{}개 바꿨습니다", ranges.len()));
        }
    }
    unsafe fn editor_menu(&self) {
        unsafe {
            let menu = CreatePopupMenu();
            let selected = editing::selection(self.edit);
            let has_selection = selected.start != selected.end;
            for (id, label, enabled) in [
                (
                    MENU_UNDO,
                    "실행 취소    Ctrl+Z",
                    SendMessageW(self.edit, EM_CANUNDO, 0, 0) != 0,
                ),
                (
                    MENU_REDO,
                    "다시 실행    Ctrl+Y",
                    SendMessageW(self.edit, WM_USER + 85, 0, 0) != 0,
                ),
                (MENU_CUT, "잘라내기    Ctrl+X", has_selection),
                (MENU_COPY, "복사    Ctrl+C", has_selection),
                (MENU_PASTE, "붙여넣기    Ctrl+V", true),
                (MENU_SELECT_ALL, "전체 선택    Ctrl+A", true),
                (MENU_FIND, "검색    Ctrl+F", true),
                (MENU_REPLACE, "바꾸기    Ctrl+H", true),
            ] {
                AppendMenuW(
                    menu,
                    MF_STRING | if enabled { 0 } else { MF_GRAYED },
                    id,
                    wide(label).as_ptr(),
                );
            }
            let mut point = POINT::default();
            GetCursorPos(&mut point);
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                self.hwnd,
                null(),
            );
            DestroyMenu(menu);
            if chosen != 0 {
                PostMessageW(self.hwnd, WM_COMMAND, chosen as usize, 0);
            }
        }
    }

    unsafe fn command(&mut self, id: usize, code: usize) {
        match id {
            ID_SETTINGS => unsafe {
                self.show_settings(!self.settings_visible);
            },
            ID_SETTINGS_CLOSE => unsafe {
                self.show_settings(false);
            },
            ID_SETTINGS_THEME | ID_SETTINGS_FONT => unsafe {
                if let Some(panel) = self.settings_panel {
                    let control = if id == ID_SETTINGS_THEME {
                        panel.theme
                    } else {
                        panel.font
                    };
                    if code == CBN_SELENDOK as usize
                        || (code == CBN_SELCHANGE as usize
                            && SendMessageW(control, CB_GETDROPPEDSTATE, 0, 0) == 0)
                    {
                        self.change_setting(id);
                    } else if code == CBN_SELENDCANCEL as usize || code == CBN_DROPDOWN as usize {
                        settings_controls::restore_selection(control);
                    } else if code == CBN_CLOSEUP as usize {
                        // Close-up and selection notification order is not fixed.
                        // Restore only after synchronous commit/cancel handlers.
                        settings_controls::defer_restore(control);
                    }
                }
            },
            ID_SETTINGS_WRAP => unsafe {
                self.change_setting(id);
            },
            ID_FIND if code == EN_CHANGE as usize => unsafe {
                self.find_next(false, true);
            },
            ID_FIND_PREV => unsafe {
                self.find_next(true, false);
            },
            ID_FIND_NEXT => unsafe {
                self.find_next(false, false);
            },
            ID_FIND_CASE => unsafe {
                self.find_case_sensitive = !self.find_case_sensitive;
                InvalidateRect(self.find_case, null(), 0);
                self.find_next(false, true);
            },
            ID_FIND_CLOSE => unsafe {
                self.hide_find();
            },
            ID_REPLACE_ONE => unsafe {
                self.replace_one();
            },
            ID_REPLACE_ALL => unsafe {
                self.replace_all();
            },
            MENU_UNDO | MENU_REDO | MENU_CUT | MENU_COPY | MENU_PASTE | MENU_SELECT_ALL => unsafe {
                SetFocus(self.edit);
                match id {
                    MENU_UNDO => {
                        SendMessageW(self.edit, WM_UNDO, 0, 0);
                    }
                    MENU_REDO => {
                        SendMessageW(self.edit, editing::EM_REDO, 0, 0);
                    }
                    MENU_CUT => {
                        SendMessageW(self.edit, WM_CUT, 0, 0);
                    }
                    MENU_COPY => {
                        SendMessageW(self.edit, WM_COPY, 0, 0);
                    }
                    MENU_PASTE => {
                        SendMessageW(self.edit, WM_PASTE, 0, 0);
                    }
                    _ => {
                        editing::select(self.edit, editing::CharRange { start: 0, end: -1 });
                    }
                }
                self.capture_edit();
                self.publish_view();
                InvalidateRect(self.status, null(), 0);
            },
            MENU_FIND => unsafe {
                self.show_find(false);
            },
            MENU_REPLACE => unsafe {
                self.show_find(true);
            },
            ID_TOGGLE => {
                self.sidebar_visible = !self.sidebar_visible;
                unsafe {
                    self.layout();
                }
            }
            ID_ADD | ID_TAB_ADD => {
                let parent = unsafe { self.selected_id() }
                    .and_then(|id| self.book.get(id))
                    .map_or(0, |note| note.parent);
                unsafe {
                    self.create_note(parent);
                }
            }
            MENU_NEW_CHILD => {
                let parent = unsafe { self.selected_id() }.unwrap_or(0);
                unsafe {
                    self.create_note(parent);
                }
            }
            MENU_NEW_ROOT => unsafe {
                self.create_note(0);
            },
            MENU_RENAME => unsafe {
                self.rename_selected();
            },
            MENU_DELETE => unsafe {
                self.delete_selected();
            },
            MENU_GROUP => match self.group_selected_notes() {
                Ok(Some(_)) => unsafe {
                    self.rename_selected();
                },
                Ok(None) => {}
                Err(error) => report(self.hwnd, "폴더를 만들지 못했습니다.", error),
            },
            MENU_UP => unsafe {
                self.shift_selected(-1);
            },
            MENU_DOWN => unsafe {
                self.shift_selected(1);
            },
            MENU_OUTDENT => unsafe {
                self.outdent_selected();
            },
            ID_EDIT if code == EN_CHANGE as usize && !self.loading => unsafe {
                if let Some(index) = self.active {
                    self.open[index].dirty = true;
                }
                self.last_match = None;
                SetTimer(self.hwnd, AUTOSAVE_TIMER, 650, None);
                self.set_status("저장 중…");
            },
            _ => {}
        }
    }

    unsafe fn notify(&mut self, hdr: &NMHDR) -> LRESULT {
        if hdr.hwndFrom == self.edit && hdr.code == editing::EN_SELCHANGE {
            unsafe {
                InvalidateRect(self.status, null(), 0);
            }
        } else if hdr.hwndFrom == self.tree {
            match hdr.code {
                NM_CLICK => {
                    // Finish after DefSubclassProc restores focus and completes DragDetect.
                    if let Some(press) = &mut self.tree_press {
                        press.clicked = true;
                    }
                }
                TVN_BEGINLABELEDITW => unsafe {
                    if self.selection.ids.len() > 1 {
                        return 1;
                    }
                    PostMessageW(self.hwnd, POSITION_RENAME, 0, 0);
                },
                TVN_SELCHANGEDW => {
                    if self.tree_mouse_input
                        || self.tree_press.is_some()
                        || self.dragging.is_some()
                        || self.loading
                    {
                        return 0;
                    }
                    let event = unsafe { &*(hdr as *const _ as *const NMTREEVIEWW) };
                    if event.itemNew.lParam > 0 {
                        let id = event.itemNew.lParam as u64;
                        let keyboard = event.action == TVC_BYKEYBOARD;
                        let ctrl = keyboard && unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
                        let shift = keyboard && unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
                        if !ctrl || shift {
                            let visible = if shift {
                                self.visible_note_ids()
                            } else {
                                Vec::new()
                            };
                            self.selection.click(id, &visible, ctrl, shift);
                        }
                        unsafe {
                            self.refresh_selection();
                            if !ctrl && !shift {
                                self.open_note(id);
                                if keyboard {
                                    SetFocus(self.tree);
                                }
                            }
                        }
                    }
                }
                TVN_ENDLABELEDITW => {
                    let event = unsafe { &*(hdr as *const _ as *const NMTVDISPINFOW) };
                    if !event.item.pszText.is_null() {
                        let mut len = 0;
                        unsafe {
                            while *event.item.pszText.add(len) != 0 {
                                len += 1;
                            }
                        }
                        let title = unsafe {
                            String::from_utf16_lossy(std::slice::from_raw_parts(
                                event.item.pszText,
                                len,
                            ))
                        };
                        let title = title.trim();
                        if !title.is_empty() {
                            let id = event.item.lParam as u64;
                            if let Err(error) = self.rename_note(id, title) {
                                report(self.hwnd, "메모 이름을 저장하지 못했습니다.", error);
                                return 0;
                            }
                            unsafe {
                                self.layout();
                                PostMessageW(self.hwnd, REFRESH_UI, 0, 0);
                            }
                            return 1;
                        }
                    }
                }
                TVN_BEGINDRAGW => {
                    let event = unsafe { &*(hdr as *const _ as *const NMTREEVIEWW) };
                    if event.itemNew.lParam > 0 {
                        unsafe {
                            self.begin_tree_drag(event.itemNew.lParam as u64);
                        }
                    }
                }
                TVN_ITEMEXPANDEDW => {
                    let event = unsafe { &*(hdr as *const _ as *const NMTREEVIEWW) };
                    let id = event.itemNew.lParam as u64;
                    if event.action == TVE_EXPAND {
                        self.expanded.insert(id);
                    } else {
                        self.expanded.remove(&id);
                    }
                    self.sync_tree_view();
                    unsafe {
                        self.layout();
                    }
                }
                NM_RCLICK => unsafe {
                    self.context_menu(GetMessagePos() as isize);
                    return 1; // Suppress the duplicate default context menu.
                },
                _ => {}
            }
        }
        0
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        FINISH_TAB_RENAME => {
            with_app(|app| unsafe {
                if GetFocus() != app.tab_name_edit
                    && let Err(error) = app.finish_tab_rename(true, false)
                {
                    report(app.hwnd, "메모 이름을 저장하지 못했습니다.", error);
                }
            });
            0
        }
        REFRESH_UI => {
            with_app(|app| unsafe {
                for control in [app.tree, app.tabs, app.status] {
                    InvalidateRect(control, null(), 0);
                }
            });
            0
        }
        POSITION_RENAME => {
            with_app(|app| unsafe {
                app.layout_rename();
            });
            0
        }
        WM_ERASEBKGND => 1,
        WM_SETCURSOR => {
            let mut handled = false;
            ui::with_view(|view| unsafe {
                let mut point = POINT::default();
                GetCursorPos(&mut point);
                ScreenToClient(hwnd, &mut point);
                let cursor = if view.dragging.is_some() {
                    Some(if view.drop_target.is_some() {
                        IDC_SIZEALL
                    } else {
                        IDC_NO
                    })
                } else if view.resizing || view.splitter_hit(point) {
                    Some(IDC_SIZEWE)
                } else {
                    None
                };
                if let Some(cursor) = cursor {
                    SetCursor(LoadCursorW(null_mut(), cursor));
                    handled = true;
                }
            });
            if handled {
                1
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
            }
        }
        WM_PAINT => {
            unsafe {
                ui::paint_window(hwnd, |dc| ui::with_view(|v| v.paint_background(dc)));
            }
            0
        }
        WM_DRAWITEM if lp != 0 => {
            ui::with_view(|view| unsafe {
                let draw = &*(lp as *const DRAWITEMSTRUCT);
                ui::buffered(draw.hwndItem, draw.hDC, draw.rcItem, |dc| {
                    let copy = DRAWITEMSTRUCT { hDC: dc, ..*draw };
                    view.paint_control(&copy);
                });
            });
            1
        }
        WM_MEASUREITEM if lp != 0 => {
            unsafe {
                (*(lp as *mut MEASUREITEMSTRUCT)).itemHeight =
                    (24 * GetDpiForWindow(hwnd).max(96) / 96).max(1);
            }
            1
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORBTN | WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX => {
            let mut brush = 0;
            ui::with_view(|view| unsafe {
                let dc = wp as HDC;
                SetTextColor(dc, view.theme.colors.ink);
                SetBkColor(dc, view.theme.colors.white);
                brush = view.theme.white_brush as LRESULT;
            });
            if brush != 0 {
                brush
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
            }
        }
        WM_DPICHANGED => {
            with_app(|app| unsafe {
                let theme = Rc::new(Theme::new(
                    hi(wp) as u32,
                    app.settings.dark(),
                    &app.settings.font,
                ));
                let old = std::mem::replace(&mut app.theme, theme);
                app.publish_view();
                app.apply_theme(true, true, true);
                drop(old);
                let suggested = &*(lp as *const RECT);
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                app.layout();
            });
            0
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED => {
            with_app(|app| unsafe {
                if app.settings.theme == ThemeMode::System {
                    app.apply_settings(app.settings.clone());
                }
            });
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_GETMINMAXINFO => {
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
            unsafe {
                (*(lp as *mut MINMAXINFO)).ptMinTrackSize = POINT {
                    x: 560 * dpi / 96,
                    y: 360 * dpi / 96,
                };
            }
            0
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            if unsafe { route_wheel(msg, wp) } {
                0
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
            }
        }
        WM_SIZE => {
            with_app(|app| unsafe {
                app.layout();
            });
            0
        }
        WM_COMMAND => {
            with_app(|app| unsafe {
                app.command(lo(wp), hi(wp));
            });
            0
        }
        WM_NOTIFY => {
            if lp == 0 {
                return 0;
            }
            let hdr = unsafe { &*(lp as *const NMHDR) };
            if hdr.code == NM_CUSTOMDRAW {
                let mut result = 0;
                ui::with_view(|v| {
                    if hdr.hwndFrom == v.tree {
                        let draw = unsafe { &*(lp as *const NMTVCUSTOMDRAW) };
                        if draw.nmcd.dwDrawStage == CDDS_PREPAINT {
                            result = CDRF_NOTIFYITEMDRAW as isize;
                        }
                        if draw.nmcd.dwDrawStage == CDDS_ITEMPREPAINT {
                            unsafe {
                                v.paint_tree_item(draw);
                            }
                            result = CDRF_SKIPDEFAULT as isize;
                        }
                    }
                });
                return result;
            }
            let mut result = 0;
            with_app(|app| {
                result = unsafe { app.notify(hdr) };
            });
            result
        }
        WM_TIMER if wp == AUTOSAVE_TIMER => {
            with_app(|app| unsafe {
                app.save_all();
            });
            0
        }
        WM_TIMER if wp == TAB_DRAG_SCROLL_TIMER => {
            with_app(|app| unsafe {
                if app.tab_drag.is_some_and(|drag| drag.moving) {
                    let mut point = POINT::default();
                    GetCursorPos(&mut point);
                    ScreenToClient(app.tabs, &mut point);
                    let mut bounds = RECT::default();
                    GetClientRect(app.tabs, &mut bounds);
                    let mut layout = app.tab_layout.get();
                    if point.y >= 0 && point.y < bounds.bottom {
                        if point.x < app.theme.px(24) {
                            layout.scroll(-app.theme.px(24));
                        } else if point.x >= layout.viewport - app.theme.px(24) {
                            layout.scroll(app.theme.px(24));
                        }
                    }
                    app.tab_layout.set(layout);
                    app.update_tab_drag(point);
                    InvalidateRect(app.tab_scroll, null(), 0);
                }
            });
            0
        }
        WM_TIMER if wp == DRAG_SCROLL_TIMER => {
            with_app(|app| unsafe {
                if app.dragging.is_some() {
                    let mut scrolled = false;
                    let mut point = POINT::default();
                    let mut bounds = RECT::default();
                    GetCursorPos(&mut point);
                    GetWindowRect(app.tree_host, &mut bounds);
                    if PtInRect(&bounds, point) != 0 {
                        if point.y < bounds.top + app.theme.px(24) {
                            app.scroll_tree(-2);
                            scrolled = true;
                        } else if point.y >= bounds.bottom - app.theme.px(24) {
                            app.scroll_tree(2);
                            scrolled = true;
                        }
                    }
                    app.update_drag_feedback();
                    if scrolled {
                        app.repaint_drag_feedback();
                    }
                }
            });
            0
        }
        WM_LBUTTONDOWN => {
            with_app(|app| {
                let mut area = RECT::default();
                unsafe {
                    GetClientRect(app.hwnd, &mut area);
                }
                if app.sidebar_visible
                    && (point_x(lp) - app.sidebar_edge(area.right)).abs() <= app.theme.px(4)
                    && point_y(lp) > app.theme.px(36)
                {
                    app.resizing = true;
                    unsafe {
                        SetCapture(hwnd);
                        SetCursor(LoadCursorW(null_mut(), IDC_SIZEWE));
                    }
                }
            });
            0
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                if !app.resizing && app.dragging.is_none() {
                    let mut area = RECT::default();
                    unsafe {
                        GetClientRect(hwnd, &mut area);
                    }
                    if app.sidebar_visible
                        && point_y(lp) > app.theme.px(36)
                        && (point_x(lp) - app.sidebar_edge(area.right)).abs() <= app.theme.px(4)
                    {
                        unsafe {
                            SetCursor(LoadCursorW(null_mut(), IDC_SIZEWE));
                        }
                    }
                }
                if app.resizing {
                    unsafe {
                        SetCursor(LoadCursorW(null_mut(), IDC_SIZEWE));
                    }
                    let width = (point_x(lp) * 96 / app.theme.dpi as i32).clamp(150, 600);
                    if width != app.sidebar_width {
                        app.sidebar_width = width;
                        unsafe {
                            app.layout();
                        }
                    }
                }
                if app.dragging.is_some() {
                    unsafe {
                        app.update_drag_feedback();
                    }
                }
            });
            0
        }
        WM_LBUTTONUP => {
            with_app(|app| {
                if app.resizing {
                    app.resizing = false;
                    unsafe {
                        ReleaseCapture();
                    }
                }
                if app.dragging.is_some() {
                    unsafe {
                        app.finish_drag();
                    }
                }
            });
            0
        }
        WM_CAPTURECHANGED | WM_CANCELMODE => {
            with_app(|app| unsafe {
                if app.resizing || app.dragging.is_some() {
                    app.cancel_interaction();
                }
            });
            0
        }
        WM_CLOSE => {
            let mut ok = true;
            with_app(|app| {
                if let Err(error) = unsafe { app.finish_tab_rename(true, false) } {
                    report(app.hwnd, "메모 이름을 저장하지 못했습니다.", error);
                    ok = false;
                    return;
                }
                ok = unsafe { app.save_all() };
                if ok && let Err(error) = app.save_session() {
                    report(app.hwnd, "탭 상태를 저장하지 못했습니다.", error);
                }
            });
            if ok {
                unsafe {
                    DestroyWindow(hwnd);
                }
            }
            0
        }
        WM_DESTROY => {
            ui::clear();
            APP.with(|cell| {
                cell.borrow_mut().take();
            });
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

fn shortcut(msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN {
        return false;
    }
    let ctrl = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
    let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
    let key = msg.wParam as u32;
    let mut handled = false;
    with_app(|app| unsafe {
        if msg.hwnd == app.tab_name_edit
            && app.tab_rename.is_some()
            && matches!(key as u16, VK_RETURN | VK_ESCAPE | VK_TAB)
        {
            if let Err(error) = app.finish_tab_rename(key != VK_ESCAPE as u32, true) {
                report(app.hwnd, "메모 이름을 저장하지 못했습니다.", error);
            }
            handled = true;
        } else if ctrl && key == VK_OEM_COMMA as u32 {
            app.show_settings(!app.settings_visible);
            handled = true;
        } else if app.settings_visible && key == VK_ESCAPE as u32 {
            let dropped = app.settings_panel.is_some_and(|panel| {
                SendMessageW(panel.theme, CB_GETDROPPEDSTATE, 0, 0) != 0
                    || SendMessageW(panel.font, CB_GETDROPPEDSTATE, 0, 0) != 0
            });
            if !dropped {
                app.show_settings(false);
                handled = true;
            }
        } else if app.settings_visible && !ctrl && key == VK_TAB as u32 {
            if let Some(panel) = app.settings_panel {
                let controls = [panel.theme, panel.font, panel.wrap, panel.close];
                let current = controls
                    .iter()
                    .position(|&control| control == msg.hwnd)
                    .unwrap_or(0);
                SetFocus(
                    controls[if shift {
                        (current + 3) % 4
                    } else {
                        (current + 1) % 4
                    }],
                );
                handled = true;
            }
        } else if app.settings_visible
            && key == VK_RETURN as u32
            && app
                .settings_panel
                .is_some_and(|panel| msg.hwnd == panel.wrap || msg.hwnd == panel.close)
        {
            app.command(GetDlgCtrlID(msg.hwnd) as usize, BN_CLICKED as usize);
            handled = true;
        } else if !ctrl
            && key == VK_TAB as u32
            && app.find_visible
            && IsChild(app.search_host, msg.hwnd) != 0
        {
            let mut controls = vec![app.find_input];
            if app.replace_visible {
                controls.push(app.replace_input);
            }
            controls.extend([app.find_case, app.find_prev, app.find_next]);
            if app.replace_visible {
                controls.extend([app.replace_one, app.replace_all]);
            }
            controls.push(app.find_close);
            let current = controls
                .iter()
                .position(|&control| control == msg.hwnd)
                .unwrap_or(0);
            let next = if shift {
                (current + controls.len() - 1) % controls.len()
            } else {
                (current + 1) % controls.len()
            };
            SetFocus(controls[next]);
            handled = true;
        } else if key == VK_RETURN as u32
            && app.find_visible
            && IsChild(app.search_host, msg.hwnd) != 0
            && msg.hwnd != app.find_input
            && msg.hwnd != app.replace_input
        {
            app.command(GetDlgCtrlID(msg.hwnd) as usize, BN_CLICKED as usize);
            handled = true;
        } else if key == VK_ESCAPE as u32
            && (app.dragging.is_some()
                || app.tab_drag.is_some()
                || app.resizing
                || [
                    app.editor_scroll[0],
                    app.editor_scroll[1],
                    app.tree_scroll,
                    app.tab_scroll,
                ]
                .contains(&GetCapture()))
        {
            app.cancel_interaction();
            handled = true;
        } else if ctrl && key == b'A' as u32 {
            if msg.hwnd == app.tree {
                app.select_all_tree();
                handled = true;
            } else if app.active.is_some()
                && (msg.hwnd == app.edit || msg.hwnd == app.tree || msg.hwnd == app.tabs)
            {
                SetFocus(app.edit);
                editing::select(app.edit, editing::CharRange { start: 0, end: -1 });
                handled = true;
            } else if msg.hwnd == app.find_input
                || msg.hwnd == app.replace_input
                || msg.hwnd == app.tab_name_edit
                || msg.hwnd == SendMessageW(app.tree, TVM_GETEDITCONTROL, 0, 0) as HWND
            {
                SendMessageW(msg.hwnd, EM_SETSEL, 0, -1);
                handled = true;
            }
        } else if ctrl && key == b'F' as u32 {
            app.show_find(false);
            handled = true;
        } else if ctrl && key == b'H' as u32 {
            app.show_find(true);
            handled = true;
        } else if key == VK_F3 as u32 {
            if window_text(app.find_input).is_empty() {
                app.show_find(false);
            } else {
                app.find_next(shift, false);
            }
            handled = true;
        } else if key == VK_ESCAPE as u32
            && app.find_visible
            && (msg.hwnd == app.edit
                || msg.hwnd == app.find_input
                || msg.hwnd == app.replace_input
                || IsChild(app.search_host, msg.hwnd) != 0)
        {
            app.hide_find();
            handled = true;
        } else if key == VK_RETURN as u32
            && (msg.hwnd == app.find_input || msg.hwnd == app.replace_input)
        {
            if msg.hwnd == app.replace_input {
                app.replace_one();
            } else {
                app.find_next(shift, false);
            }
            handled = true;
        } else if ctrl && msg.hwnd == app.edit && (key == b'Z' as u32 || key == b'Y' as u32) {
            SendMessageW(
                app.edit,
                if key == b'Y' as u32 || shift {
                    editing::EM_REDO
                } else {
                    WM_UNDO
                },
                0,
                0,
            );
            handled = true;
        } else if ctrl && key == b'N' as u32 {
            app.command(ID_ADD, 0);
            handled = true;
        } else if ctrl && key == b'S' as u32 {
            app.save_all();
            handled = true;
        } else if ctrl && key == b'W' as u32 {
            app.close_active();
            handled = true;
        } else if ctrl && key == b'B' as u32 {
            app.sidebar_visible = !app.sidebar_visible;
            app.layout();
            handled = true;
        } else if !app.open.is_empty()
            && ((ctrl && (key == VK_TAB as u32 || key == VK_PRIOR as u32 || key == VK_NEXT as u32))
                || (msg.hwnd == app.tabs && (key == VK_LEFT as u32 || key == VK_RIGHT as u32)))
        {
            let prev =
                key == VK_PRIOR as u32 || key == VK_LEFT as u32 || key == VK_TAB as u32 && shift;
            let index = app.active.unwrap_or(0);
            let count = app.open.len();
            app.activate(if prev {
                (index + count - 1) % count
            } else {
                (index + 1) % count
            });
            handled = true;
        } else if ctrl && (b'1' as u32..=b'9' as u32).contains(&key) && !app.open.is_empty() {
            let index = if key == b'9' as u32 {
                app.open.len() - 1
            } else {
                (key - b'1' as u32) as usize
            };
            app.activate(index);
            handled = true;
        } else if key == VK_F6 as u32 {
            SetFocus(if GetFocus() == app.edit && app.sidebar_visible {
                app.tree
            } else {
                app.edit
            });
            handled = true;
        } else if key == VK_F2 as u32 {
            app.rename_current(msg.hwnd);
            handled = true;
        } else if key == VK_DELETE as u32 && msg.hwnd == app.tree {
            app.delete_selected();
            handled = true;
        }
        if handled {
            InvalidateRect(app.status, null(), 0);
        }
    });
    handled
}

pub fn run() {
    startup::start();
    let data_dir = std::env::var_os("EDOT_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
                .join("Edot")
        });
    let load_timing = startup::span("notebook.load");
    let book = match Notebook::load(&data_dir) {
        Ok(book) => book,
        Err(error) => {
            report(
                null_mut(),
                "메모 목록을 읽지 못했습니다. 파일을 덮어쓰지 않습니다.",
                error,
            );
            return;
        }
    };
    drop(load_timing);
    let settings_timing = startup::span("settings.load");
    let settings = match Settings::load(&data_dir) {
        Ok(settings) => settings,
        Err(error) => {
            report(
                null_mut(),
                "설정을 읽지 못했습니다. 설정 파일을 확인해주세요.",
                error,
            );
            return;
        }
    };
    drop(settings_timing);
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let canonical = fs::canonicalize(&data_dir).unwrap_or_else(|_| data_dir.clone());
        let mut hash = 0xcbf29ce484222325u64;
        for byte in canonical.to_string_lossy().to_lowercase().bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        let name = wide(&format!("Local\\Edot-{hash:016x}"));
        let mutex = CreateMutexW(null(), 0, name.as_ptr());
        if mutex.is_null() {
            report(
                null_mut(),
                "메모 저장소를 잠그지 못했습니다.",
                std::io::Error::last_os_error(),
            );
            return;
        }
        let _lock = MutexGuard(mutex);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            message(
                null_mut(),
                "Edot",
                "같은 메모 저장소가 이미 다른 창에서 열려 있습니다.",
                MB_OK | MB_ICONINFORMATION,
            );
            return;
        }
        let controls = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_TREEVIEW_CLASSES | ICC_TAB_CLASSES | ICC_BAR_CLASSES,
        };
        InitCommonControlsEx(&controls);
        let dll_timing = startup::span("richedit.dll");
        if LoadLibraryW(wide("Msftedit.dll").as_ptr()).is_null() {
            report(
                null_mut(),
                "텍스트 편집 컨트롤을 불러오지 못했습니다.",
                std::io::Error::last_os_error(),
            );
            return;
        }
        drop(dll_timing);
        let instance = GetModuleHandleW(null());
        let class = wide("EdotMainWindow");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            report(
                null_mut(),
                "창 클래스를 등록하지 못했습니다.",
                std::io::Error::last_os_error(),
            );
            return;
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("Edot 메모").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            980,
            680,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            report(
                null_mut(),
                "창을 만들지 못했습니다.",
                std::io::Error::last_os_error(),
            );
            return;
        }
        let dpi = GetDpiForWindow(hwnd).max(96) as i32;
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            1100 * dpi / 96,
            760 * dpi / 96,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        APP.with(|cell| {
            *cell.borrow_mut() = Some(App::new(hwnd, data_dir, book, settings));
        });
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        if startup::finish() {
            DestroyWindow(hwnd);
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if shortcut(&msg) {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

pub(crate) unsafe fn scroll_to(kind: Kind, position: i32) {
    unsafe {
        with_app(|app| app.set_scroll(kind, position));
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn body_scrollbars_stay_outside_the_clip_with_small_padding() {
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_TREEVIEW_CLASSES | ICC_BAR_CLASSES,
            });
            assert!(!LoadLibraryW(wide("Msftedit.dll").as_ptr()).is_null());
            let root = std::env::temp_dir().join(format!(
                "edot-body-padding-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let mut book = Notebook::load(&root).unwrap();
            let id = book.add(0, "여백 테스트".into());
            fs::write(book.file(&root, id), "").unwrap();
            book.save(&root).unwrap();
            let hwnd = child(
                null_mut(),
                wide("STATIC").as_ptr(),
                "",
                WS_OVERLAPPEDWINDOW,
                0,
                0,
            );
            SetWindowPos(
                hwnd,
                null_mut(),
                0,
                0,
                600,
                400,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let mut app = App::new(hwnd, root.clone(), book, Settings::default());
            app.open_note(id);
            let editor = app.edit;
            for wrap in [true, false, true] {
                let mut settings = app.settings.clone();
                settings.wrap = wrap;
                app.apply_settings(settings);
                for text in [
                    String::new(),
                    "짧은 메모".into(),
                    "긴 메모 ABC ".repeat(100),
                    "긴 메모 ABC\n".repeat(100),
                ] {
                    SetWindowTextW(editor, wide(&text).as_ptr());
                    // RichEdit can reserve its disabled native bar again during
                    // layout. It must stay clipped even with a small bottom gap.
                    ShowScrollBar(editor, SB_HORZ, 1);
                    let mut last = POINT::default();
                    SendMessageW(
                        editor,
                        WM_USER + 38,
                        &mut last as *mut _ as usize,
                        text.encode_utf16().count().saturating_sub(1) as isize,
                    );
                    scrollbar::layout_body(editor);
                    let mut client = RECT::default();
                    let mut clip_area = RECT::default();
                    let mut host_area = RECT::default();
                    GetClientRect(editor, &mut client);
                    GetClientRect(app.editor_clip, &mut clip_area);
                    GetClientRect(app.editor_host, &mut host_area);
                    assert_eq!(
                        (client.right, client.bottom),
                        (host_area.right, host_area.bottom)
                    );
                    let mut origin = POINT::default();
                    ClientToScreen(editor, &mut origin);
                    ScreenToClient(app.editor_clip, &mut origin);
                    let range = scrollbar::body_range(editor, true);
                    assert!(
                        origin.y + client.bottom >= clip_area.bottom,
                        "native horizontal scrollbar is visible inside the clip"
                    );
                    assert!(
                        origin.x + client.right >= clip_area.right,
                        "native vertical scrollbar is visible inside the clip"
                    );
                    assert_eq!(window_text(editor).replace("\r\n", "\n"), text);
                    if wrap {
                        assert_eq!(range.extent(), 0);
                    }
                }
            }
            ui::clear();
            DestroyWindow(hwnd);
            drop(app);
            fs::remove_dir_all(root).unwrap();
        }
    }

    unsafe extern "system" fn initial_frame_observer(
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
        id: usize,
        reference: usize,
    ) -> LRESULT {
        unsafe {
            if msg == WM_SHOWWINDOW && wp != 0 {
                let mut ready = false;
                ui::with_view(|view| {
                    if let Some(panel) = view.settings_panel {
                        ready = panel.host == hwnd && view.settings_visible;
                        for control in [
                            panel.close,
                            panel.wrap,
                            panel.theme,
                            panel.font,
                            panel.theme_label,
                            panel.font_label,
                        ] {
                            let mut area = RECT::default();
                            GetClientRect(control, &mut area);
                            ready &= area.right > 0 && area.bottom > 0;
                        }
                    }
                });
                (*(reference as *const RefCell<Vec<bool>>))
                    .borrow_mut()
                    .push(ready);
            }
            if msg == WM_NCDESTROY {
                RemoveWindowSubclass(hwnd, Some(initial_frame_observer), id);
            }
            DefSubclassProc(hwnd, msg, wp, lp)
        }
    }

    unsafe extern "system" fn paint_test_owner(
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
        id: usize,
        reference: usize,
    ) -> LRESULT {
        unsafe {
            if msg == WM_PARENTNOTIFY && lo(wp) == WM_CREATE as usize && hi(wp) == 125 {
                SetWindowSubclass(lp as HWND, Some(initial_frame_observer), 77, reference);
            }
            if matches!(
                msg,
                WM_DRAWITEM
                    | WM_CTLCOLOREDIT
                    | WM_CTLCOLORBTN
                    | WM_CTLCOLORSTATIC
                    | WM_CTLCOLORLISTBOX
            ) {
                return window_proc(hwnd, msg, wp, lp);
            }
            if msg == WM_NCDESTROY {
                RemoveWindowSubclass(hwnd, Some(paint_test_owner), id);
            }
            DefSubclassProc(hwnd, msg, wp, lp)
        }
    }

    // Exercises the actual native erase/print messages used before regular
    // WM_PAINT, while every top-level window remains hidden.
    #[test]
    fn settings_initial_frame_and_animation_keep_the_theme() {
        unsafe {
            let controls = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_TREEVIEW_CLASSES | ICC_BAR_CLASSES,
            };
            InitCommonControlsEx(&controls);
            assert!(!LoadLibraryW(wide("Msftedit.dll").as_ptr()).is_null());
            let focus = GetFocus();
            let capture = GetCapture();
            let root = std::env::temp_dir().join(format!(
                "edot-first-frame-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let dc = CreateCompatibleDC(null_mut());
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: 700,
                    biHeight: -700,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0);
            assert!(!dc.is_null() && !bitmap.is_null());
            let original = SelectObject(dc, bitmap);
            let sentinel = theme::rgb(230, 15, 110);
            let canvas = theme::rect(0, 0, 700, 700);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                let dir = root.join(format!("{mode:?}"));
                let book = Notebook::load(&dir).unwrap();
                let readiness = Box::new(RefCell::new(Vec::<bool>::new()));
                let hwnd = child(
                    null_mut(),
                    windows_sys::w!("STATIC"),
                    "first-frame-test",
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                );
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    0,
                    0,
                    1000,
                    760,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                SetWindowSubclass(
                    hwnd,
                    Some(paint_test_owner),
                    77,
                    &*readiness as *const RefCell<Vec<bool>> as usize,
                );
                let mut app = App::new(
                    hwnd,
                    dir,
                    book,
                    Settings {
                        theme: mode,
                        ..Default::default()
                    },
                );
                app.show_settings(true);
                assert_eq!(
                    &*readiness.borrow(),
                    &[true],
                    "first show must follow publishing and child layout"
                );
                let panel = app.settings_panel.unwrap();
                let background = app.theme.colors.white;
                for control in [panel.close, panel.wrap] {
                    theme::fill(dc, canvas, sentinel);
                    assert_eq!(SendMessageW(control, WM_ERASEBKGND, dc as usize, 0), 1);
                    assert_eq!(
                        GetPixel(dc, 0, 0),
                        sentinel,
                        "native erasure must not expose the system button color"
                    );
                    let brush =
                        SendMessageW(panel.host, WM_CTLCOLORBTN, dc as usize, control as isize)
                            as HBRUSH;
                    assert!(!brush.is_null());
                    FillRect(dc, &canvas, brush);
                    assert_eq!(GetPixel(dc, 0, 0), background);
                    for message in [WM_PRINTCLIENT, WM_PRINT] {
                        theme::fill(dc, canvas, sentinel);
                        SendMessageW(
                            control,
                            message,
                            dc as usize,
                            (PRF_CLIENT | PRF_ERASEBKGND) as isize,
                        );
                        assert_eq!(GetPixel(dc, 0, 0), background);
                    }
                }
                theme::fill(dc, canvas, sentinel);
                SendMessageW(
                    panel.host,
                    WM_PRINT,
                    dc as usize,
                    (PRF_CLIENT | PRF_CHILDREN | PRF_ERASEBKGND) as isize,
                );
                assert_eq!(GetPixel(dc, app.theme.px(3), app.theme.px(60)), background);

                let mut combo = COMBOBOXINFO {
                    cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32,
                    ..Default::default()
                };
                assert_ne!(GetComboBoxInfo(panel.font, &mut combo), 0);
                let list = combo.hwndList;
                SetWindowPos(
                    list,
                    null_mut(),
                    0,
                    0,
                    320,
                    250,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let bar = FindWindowExW(
                    list,
                    null_mut(),
                    windows_sys::w!("STATIC"),
                    windows_sys::w!("목록 스크롤"),
                );
                assert!(!bar.is_null());
                let selection = SendMessageW(panel.font, CB_GETCURSEL, 0, 0);
                for message in [WM_PRINTCLIENT, WM_PRINT] {
                    theme::fill(dc, canvas, sentinel);
                    SendMessageW(
                        bar,
                        message,
                        dc as usize,
                        (PRF_CLIENT | PRF_ERASEBKGND) as isize,
                    );
                    assert_eq!(GetPixel(dc, 0, 0), background);
                    // AnimateWindow may print the parent without PRF_CHILDREN.
                    // Its snapshot must still contain the common scrollbar.
                    theme::fill(dc, canvas, sentinel);
                    SendMessageW(
                        list,
                        message,
                        dc as usize,
                        (PRF_CLIENT | PRF_ERASEBKGND) as isize,
                    );
                    let mut bounds = RECT::default();
                    GetWindowRect(bar, &mut bounds);
                    MapWindowPoints(null_mut(), list, &mut bounds as *mut _ as *mut POINT, 2);
                    let length = bounds.bottom - bounds.top;
                    let range = scrollbar::list_range(list);
                    assert!(range.extent() > 0);
                    let (start, size) = range.thumb(length, app.theme.px(24));
                    assert_eq!(
                        GetPixel(dc, bounds.left, bounds.top + length / 2),
                        background
                    );
                    assert_eq!(
                        GetPixel(
                            dc,
                            (bounds.left + bounds.right) / 2,
                            bounds.top + start + size / 2
                        ),
                        app.theme.colors.muted
                    );
                }
                assert_eq!(SendMessageW(panel.font, CB_GETCURSEL, 0, 0), selection);
                assert_eq!(IsWindowVisible(list), 0);
                app.show_settings(false);
                app.show_settings(true);
                assert_eq!(&*readiness.borrow(), &[true, true]);
                assert_eq!(IsWindowVisible(hwnd), 0);
                ui::clear();
                DestroyWindow(hwnd);
                drop(app);
            }
            SelectObject(dc, original);
            DeleteObject(bitmap);
            DeleteDC(dc);
            fs::remove_dir_all(root).unwrap();
            assert_eq!(GetFocus(), focus);
            assert_eq!(GetCapture(), capture);
        }
    }

    #[test]
    fn context_menus_close_target_tabs_and_preserve_notes() {
        unsafe {
            let controls = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_TREEVIEW_CLASSES | ICC_BAR_CLASSES,
            };
            InitCommonControlsEx(&controls);
            assert!(!LoadLibraryW(wide("Msftedit.dll").as_ptr()).is_null());
            for (index, count, expected) in [(0, 1, true), (0, 5, false), (4, 5, true)] {
                let menu = tab_menu(index, count);
                assert_eq!(GetMenuItemCount(menu), 3);
                assert_eq!(GetMenuItemID(menu, 0), MENU_TAB_CLOSE as u32);
                assert_eq!(GetMenuItemID(menu, 1), MENU_TAB_CLOSE_OTHERS as u32);
                assert_eq!(GetMenuItemID(menu, 2), MENU_TAB_CLOSE_RIGHT as u32);
                assert_eq!(
                    GetMenuState(menu, MENU_TAB_CLOSE_RIGHT as u32, MF_BYCOMMAND) & MF_GRAYED != 0,
                    expected
                );
                assert_eq!(
                    GetMenuState(menu, MENU_TAB_CLOSE_OTHERS as u32, MF_BYCOMMAND) & MF_GRAYED != 0,
                    count == 1
                );
                DestroyMenu(menu);
            }
            for nested in [false, true] {
                let menu = sidebar_menu(nested);
                assert_eq!(GetMenuItemCount(menu), 4);
                for (position, id) in [MENU_NEW_CHILD, MENU_RENAME, MENU_OUTDENT, MENU_DELETE]
                    .into_iter()
                    .enumerate()
                {
                    assert_eq!(GetMenuItemID(menu, position as i32), id as u32);
                }
                assert_eq!(
                    GetMenuState(menu, MENU_OUTDENT as u32, MF_BYCOMMAND) & MF_GRAYED == 0,
                    nested
                );
                DestroyMenu(menu);
            }
            let focus = GetFocus();
            let capture = GetCapture();
            let root = std::env::temp_dir().join(format!(
                "edot-context-native-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            for (case, (target, command, remaining, active)) in [
                (0, MENU_TAB_CLOSE, vec![1, 2, 3, 4], 2),
                (1, MENU_TAB_CLOSE_OTHERS, vec![1], 1),
                (0, MENU_TAB_CLOSE_RIGHT, vec![0], 0),
                (3, MENU_TAB_CLOSE_RIGHT, vec![0, 1, 2, 3], 2),
                (4, MENU_TAB_CLOSE_RIGHT, vec![0, 1, 2, 3, 4], 2),
                (2, MENU_TAB_CLOSE, vec![0, 1, 3, 4], 3),
            ]
            .into_iter()
            .enumerate()
            {
                let dir = root.join(case.to_string());
                let mut book = Notebook::load(&dir).unwrap();
                let ids: Vec<_> = (0..5)
                    .map(|i| book.add(0, format!("메뉴 테스트 {i}")))
                    .collect();
                for (i, &id) in ids.iter().enumerate() {
                    fs::write(book.file(&dir, id), format!("원본 메모 {i}")).unwrap();
                }
                book.save(&dir).unwrap();
                let hwnd = child(
                    null_mut(),
                    windows_sys::w!("STATIC"),
                    "context-test",
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                );
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    0,
                    0,
                    900,
                    600,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let mut app = App::new(
                    hwnd,
                    dir.clone(),
                    book,
                    Settings {
                        theme: ThemeMode::Light,
                        ..Default::default()
                    },
                );
                app.open = ids
                    .iter()
                    .map(|&id| OpenTab {
                        id,
                        text: String::new(),
                        dirty: false,
                        editor: null_mut(),
                    })
                    .collect();
                app.active = None;
                app.activate(4);
                SetWindowTextW(app.edit, wide("수정한 비활성 메모").as_ptr());
                app.activate(2); // Captures the hidden editor before it is closed.
                SetWindowTextW(app.edit, wide("수정한 활성 메모").as_ptr());
                let active_editor = app.edit;
                app.close_tabs(target, command);
                assert_eq!(
                    app.open.iter().map(|tab| tab.id).collect::<Vec<_>>(),
                    remaining.iter().map(|&i| ids[i]).collect::<Vec<_>>()
                );
                assert_eq!(app.open[app.active.unwrap()].id, ids[active]);
                if active == 2 {
                    assert_eq!(app.edit, active_editor);
                }
                // The no-op rightmost command must not force an unnecessary save.
                if target != 4 || command != MENU_TAB_CLOSE_RIGHT {
                    assert_eq!(
                        fs::read_to_string(app.book.file(&dir, ids[2])).unwrap(),
                        "수정한 활성 메모"
                    );
                    assert_eq!(
                        fs::read_to_string(app.book.file(&dir, ids[4])).unwrap(),
                        "수정한 비활성 메모"
                    );
                }
                for i in [0, 1, 3] {
                    assert_eq!(
                        fs::read_to_string(app.book.file(&dir, ids[i])).unwrap(),
                        format!("원본 메모 {i}")
                    );
                }
                assert_eq!(Notebook::load(&dir).unwrap().notes.len(), 5);
                let parent = ids[0];
                let child_id = ids[1];
                assert!(app.book.move_note(child_id, parent, None));
                app.rebuild_tree(Some(child_id));
                app.command(MENU_OUTDENT, 0);
                assert_eq!(app.book.get(child_id).unwrap().parent, 0);
                assert_eq!(
                    Notebook::load(&dir).unwrap().get(child_id).unwrap().parent,
                    0
                );
                // Closing the final tab hides the editor without deleting its note.
                while !app.open.is_empty() {
                    app.close_tab(0);
                }
                assert!(app.active.is_none());
                assert_eq!(app.edit, app.empty_edit);
                assert_eq!(Notebook::load(&dir).unwrap().notes.len(), 5);
                ui::clear();
                DestroyWindow(hwnd);
                drop(app);
            }
            fs::remove_dir_all(root).unwrap();
            assert_eq!(GetFocus(), focus);
            assert_eq!(GetCapture(), capture);
        }
    }

    // Direct native-control integration test. It creates an invisible window
    // and isolated notes, and records synchronous application costs (no paint).
    #[test]
    #[ignore = "Windows native controls and opt-in performance measurement"]
    fn settings_preserve_editors_and_measure_transitions() {
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let controls = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_TREEVIEW_CLASSES | ICC_BAR_CLASSES,
            };
            InitCommonControlsEx(&controls);
            assert!(!LoadLibraryW(wide("Msftedit.dll").as_ptr()).is_null());
            let root = std::env::temp_dir().join(format!(
                "edot-settings-native-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&root).unwrap();
            for repeat in [100, 6000] {
                let dir = root.join(repeat.to_string());
                let mut book = Notebook::load(&dir).unwrap();
                let id = book.add(0, "설정 테스트".into());
                let second = book.add(0, "숨겨진 편집기".into());
                let lazy = book.add(0, "아직 읽지 않은 탭".into());
                for index in 4..=50 {
                    book.add(0, format!("스크롤 테스트 {index}"));
                }
                let content =
                    ("가나다 🙂 ABC 0123456789 ".to_owned() + &"long line text ".repeat(16) + "\n")
                        .repeat(repeat);
                for note in [id, second, lazy] {
                    fs::write(book.file(&dir, note), &content).unwrap();
                }
                book.save(&dir).unwrap();
                let hwnd = child(
                    null_mut(),
                    wide("STATIC").as_ptr(),
                    "settings-test",
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                );
                assert!(!hwnd.is_null());
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    0,
                    0,
                    1100,
                    760,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let mut app = App::new(
                    hwnd,
                    dir.clone(),
                    book,
                    Settings {
                        theme: ThemeMode::Light,
                        ..Default::default()
                    },
                );
                app.open_note(second);
                let hidden_editor = app.edit;
                app.open_note(id);
                let editor = app.edit;
                app.open.push(OpenTab {
                    id: lazy,
                    text: String::new(),
                    dirty: false,
                    editor: null_mut(),
                });
                if repeat == 100 {
                    app.show_settings(true);
                    let panel = app.settings_panel.unwrap();
                    SendMessageW(panel.theme, CB_SETCURSEL, 1, 0);
                    assert_eq!(app.settings.theme, ThemeMode::Light);
                    assert_eq!(settings_controls::confirmed_selection(panel.theme), 0);
                    app.command(ID_SETTINGS_THEME, CBN_SELENDOK as usize);
                    assert_eq!(app.settings.theme, ThemeMode::Dark);
                    assert_eq!(settings_controls::confirmed_selection(panel.theme), 1);
                    SendMessageW(panel.theme, CB_SETCURSEL, 2, 0);
                    app.command(ID_SETTINGS_THEME, CBN_SELENDCANCEL as usize);
                    assert_eq!(SendMessageW(panel.theme, CB_GETCURSEL, 0, 0), 1);
                    assert_eq!(app.settings.theme, ThemeMode::Dark);
                    assert_eq!(Settings::load(&dir).unwrap().theme, ThemeMode::Dark);
                    SendMessageW(panel.theme, CB_SETCURSEL, 0, 0);
                    app.command(ID_SETTINGS_THEME, CBN_SELENDOK as usize);
                    app.show_settings(false);
                }
                let before = window_text(editor);
                let hidden_before = window_text(hidden_editor);
                let disk_before = fs::read(app.book.file(&dir, id)).unwrap();
                let mut wrapped_clip = RECT::default();
                let mut wrapped_host = RECT::default();
                GetClientRect(app.editor_clip, &mut wrapped_clip);
                GetClientRect(app.editor_host, &mut wrapped_host);
                assert_eq!(wrapped_clip.right, wrapped_host.right);
                let margins = SendMessageW(editor, EM_GETMARGINS, 0, 0) as u32;
                assert_eq!(margins & 0xffff, app.theme.px(12) as u32);
                assert_eq!(margins >> 16, app.theme.px(12) as u32);
                let original_second_line = SendMessageW(editor, EM_LINEINDEX, 1, 0);
                editing::select(editor, editing::CharRange { start: 4, end: 7 });
                SendMessageW(editor, EM_REPLACESEL, 1, wide("편집").as_ptr() as LPARAM);
                let edited = window_text(editor);
                let selection = editing::selection(editor);
                assert_ne!(edited, before);
                assert_ne!(SendMessageW(editor, EM_CANUNDO, 0, 0), 0);
                let modified = SendMessageW(editor, EM_GETMODIFY, 0, 0);
                let mut dark = app.settings.clone();
                dark.theme = ThemeMode::Dark;
                app.apply_settings(dark);
                assert_eq!(editing::format(editor).1, app.theme.colors.ink);
                assert_eq!(editing::format(hidden_editor).1, app.theme.colors.ink);
                let mut font = app.settings.clone();
                font.font = "Arial".into();
                app.apply_settings(font);
                assert_eq!(
                    editing::format(editor),
                    ("Arial".into(), app.theme.colors.ink)
                );
                assert_eq!(
                    editing::format(hidden_editor),
                    ("Arial".into(), app.theme.colors.ink)
                );
                let mut nowrap = app.settings.clone();
                nowrap.wrap = false;
                app.apply_settings(nowrap);
                assert!(SendMessageW(editor, EM_LINEINDEX, 1, 0) > original_second_line);
                assert_ne!(GetWindowLongPtrW(editor, GWL_STYLE) as u32 & WS_HSCROLL, 0);
                let mut client = RECT::default();
                let mut host = RECT::default();
                GetClientRect(editor, &mut client);
                GetClientRect(app.editor_host, &mut host);
                assert_eq!((client.right, client.bottom), (host.right, host.bottom));
                SendMessageW(
                    editor,
                    editing::EM_EXLINEFROMCHAR,
                    0,
                    edited.replace("\r\n", "\r").encode_utf16().count() as isize - 1,
                );
                let horizontal = ScrollRange::read(editor, true);
                assert!(horizontal.extent() > 0);
                let horizontal = scrollbar::body_range(editor, true);
                app.set_scroll(Kind::EditorHorizontal, horizontal.position);
                assert_eq!(
                    scrollbar::body_range(editor, true).position,
                    horizontal.position
                );
                app.set_scroll(Kind::EditorHorizontal, horizontal.extent() as i32);
                let vertical = scrollbar::body_range(editor, false);
                app.set_scroll(Kind::EditorVertical, vertical.extent() as i32 / 2);
                let middle = ScrollRange::read(editor, false).position;
                assert!(middle > 0);
                if repeat == 6000 {
                    assert!(middle > 65_535);
                }
                let mut clip = RECT::default();
                GetClientRect(app.editor_clip, &mut clip);
                assert_eq!(
                    (clip.right, clip.bottom),
                    (host.right - app.theme.px(12), host.bottom)
                );
                let mut origin = POINT::default();
                ClientToScreen(app.editor_clip, &mut origin);
                ScreenToClient(app.editor_host, &mut origin);
                assert_eq!(origin.y, 0);
                app.set_scroll(Kind::EditorVertical, 0);
                assert_eq!(ScrollRange::read(editor, false).position, 0);
                origin = POINT::default();
                ClientToScreen(app.editor_clip, &mut origin);
                ScreenToClient(app.editor_host, &mut origin);
                assert_eq!(origin.y, app.theme.px(8));
                app.set_scroll(Kind::EditorVertical, vertical.extent() as i32);
                let end_y = ScrollRange::read(editor, false).position;
                assert!(end_y > middle);
                GetClientRect(app.editor_clip, &mut clip);
                assert_eq!(clip.bottom, host.bottom - app.theme.px(8));
                let mut last_position = POINT::default();
                SendMessageW(
                    editor,
                    WM_USER + 38,
                    &mut last_position as *mut _ as usize,
                    edited.replace("\r\n", "\r").encode_utf16().count() as isize - 1,
                );
                ClientToScreen(editor, &mut last_position);
                ScreenToClient(app.editor_host, &mut last_position);
                assert!((0..host.bottom - app.theme.px(8)).contains(&last_position.y));
                app.set_scroll(Kind::EditorHorizontal, horizontal.extent() as i32 / 2);
                assert_eq!(ScrollRange::read(editor, false).position, end_y);
                GetClientRect(app.editor_clip, &mut clip);
                assert_eq!(clip.right, host.right);
                app.set_scroll(Kind::EditorVertical, vertical.extent() as i32 / 2);
                GetClientRect(app.editor_clip, &mut clip);
                assert_eq!((clip.right, clip.bottom), (host.right, host.bottom));
                app.tab_layout
                    .set(TabLayout::new(160, app.open.len(), 88, 180, 0));
                app.publish_view();
                app.set_scroll(Kind::Tabs, i32::MAX);
                assert_eq!(
                    app.tab_layout.get().offset,
                    app.tab_layout.get().width * app.open.len() as i32
                        - app.tab_layout.get().viewport
                );
                let active_before = app.active;
                app.set_scroll(Kind::Tabs, 0);
                assert_eq!(app.tab_layout.get().offset, 0);
                assert_eq!(app.active, active_before);
                let selected_tree = app.selected_id();
                app.set_scroll(Kind::Tree, i32::MAX);
                let first = SendMessageW(app.tree, TVM_GETNEXTITEM, TVGN_FIRSTVISIBLE as usize, 0);
                let end_index = app.visible.iter().position(|&item| item == first).unwrap();
                assert!(end_index > 0);
                let mut tree_range = ScrollRange::default();
                ui::with_view(|view| tree_range = view.scroll_range(Kind::Tree));
                assert_eq!(end_index as i64, tree_range.extent());
                app.set_scroll(Kind::Tree, 0);
                assert_eq!(
                    SendMessageW(app.tree, TVM_GETNEXTITEM, TVGN_FIRSTVISIBLE as usize, 0),
                    app.visible[0]
                );
                assert_eq!(app.selected_id(), selected_tree);
                app.layout();
                assert_eq!(editing::selection(editor), selection);
                assert_eq!(SendMessageW(editor, EM_GETMODIFY, 0, 0), modified);
                assert_eq!(window_text(editor), edited);
                let tab_scroll_before = scrollbar::body_range(editor, false).position;
                let active_index = app.active.unwrap();
                app.activate(0);
                app.activate(active_index);
                assert_eq!(
                    scrollbar::body_range(editor, false).position,
                    tab_scroll_before
                );
                assert_eq!(window_text(hidden_editor), hidden_before);
                assert!(app.open[2].editor.is_null());
                assert_eq!(fs::read(app.book.file(&dir, id)).unwrap(), disk_before);
                SendMessageW(editor, WM_UNDO, 0, 0);
                assert_eq!(window_text(editor), before);
                assert_eq!(IsWindowVisible(hidden_editor), 0);
                // Settings also apply to lazily loaded editors.
                assert!(app.load_editor(2));
                assert_eq!(
                    editing::format(app.open[2].editor),
                    ("Arial".into(), app.theme.colors.ink)
                );
                // Measure one loaded editor, as in the startup comparison.
                for tab in app.open.drain(1..) {
                    DestroyWindow(tab.editor);
                }
                app.active = Some(0);
                app.edit = hidden_editor;
                app.layout();
                let last_char = content.encode_utf16().count() as isize - 1;
                println!(
                    "scroll_environment\tbytes={}\tdpi={}",
                    content.len(),
                    app.theme.dpi
                );
                for kind in [Kind::EditorVertical, Kind::EditorHorizontal] {
                    let range = scrollbar::body_range(app.edit, kind.horizontal());
                    let mut values = Vec::new();
                    for iteration in 0..51 {
                        let start = Instant::now();
                        app.set_scroll(kind, range.extent() as i32 * (iteration % 10) / 9);
                        if iteration > 0 {
                            values.push(start.elapsed().as_secs_f64() * 1000.0);
                        }
                    }
                    values.sort_by(f64::total_cmp);
                    println!(
                        "scroll_bench\tbytes={}\taxis={}\tmedian_ms={:.3}\tmax_ms={:.3}",
                        content.len(),
                        if kind.horizontal() {
                            "horizontal"
                        } else {
                            "vertical"
                        },
                        values[25],
                        values[49]
                    );
                }
                for feature in ["theme", "font", "wrap"] {
                    let mut values = Vec::new();
                    for iteration in 0..16 {
                        let mut next = app.settings.clone();
                        match feature {
                            "theme" => {
                                next.theme = if next.theme == ThemeMode::Dark {
                                    ThemeMode::Light
                                } else {
                                    ThemeMode::Dark
                                }
                            }
                            "font" => {
                                next.font = if next.font == "Arial" {
                                    "Consolas"
                                } else {
                                    "Arial"
                                }
                                .into()
                            }
                            _ => next.wrap = !next.wrap,
                        }
                        let start = Instant::now();
                        app.apply_settings(next);
                        // Force layout through the last character so deferred
                        // RichEdit work is included, even for a hidden window.
                        SendMessageW(app.edit, editing::EM_EXLINEFROMCHAR, 0, last_char);
                        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                        if iteration > 0 {
                            values.push(elapsed);
                        }
                    }
                    values.sort_by(f64::total_cmp);
                    println!(
                        "settings_bench\tbytes={}\tfeature={}\tmedian_ms={:.3}\tmin_ms={:.3}\tmax_ms={:.3}",
                        content.len(),
                        feature,
                        values[7],
                        values[0],
                        values[14]
                    );
                }
                let start = Instant::now();
                let fonts = settings::installed_fonts(hwnd);
                println!(
                    "font_enumeration\tcount={}\tms={:.3}",
                    fonts.len(),
                    start.elapsed().as_secs_f64() * 1000.0
                );
                assert!(fonts.iter().any(|font| font == "Consolas"));
                let mut saves = Vec::new();
                for _ in 0..15 {
                    let start = Instant::now();
                    app.settings.save(&dir).unwrap();
                    saves.push(start.elapsed().as_secs_f64() * 1000.0);
                }
                saves.sort_by(f64::total_cmp);
                println!(
                    "settings_save\tmedian_ms={:.3}\tmin_ms={:.3}\tmax_ms={:.3}",
                    saves[7], saves[0], saves[14]
                );
                ui::clear();
                DestroyWindow(hwnd);
                drop(app);
            }
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use windows_sys::Win32::System::StationsAndDesktops::*;

    #[test]
    fn native_selection_tab_drag_and_batch_commands_preserve_editors() {
        unsafe {
            // No SwitchDesktop or physical input: all HWNDs live on an unshown desktop.
            let name = wide(&format!("EdotInteractionTest{}", std::process::id()));
            let desktop = CreateDesktopW(
                name.as_ptr(),
                null(),
                null(),
                0,
                DESKTOP_CREATEWINDOW
                    | DESKTOP_CREATEMENU
                    | DESKTOP_READOBJECTS
                    | DESKTOP_WRITEOBJECTS,
                null(),
            );
            assert!(!desktop.is_null());
            let desktop_id = desktop as usize;
            let result = std::thread::spawn(move || {
                assert_ne!(SetThreadDesktop(desktop_id as HDESK), 0);
                verify_interactions();
            })
            .join();
            assert_ne!(CloseDesktop(desktop), 0);
            result.unwrap();
        }
    }

    unsafe fn click_tree(tree: HWND, id: u64, modifiers: usize, release: bool) {
        unsafe {
            let mut point = POINT::default();
            with_app(|app| {
                let handle = *app
                    .handles
                    .iter()
                    .find(|&(_, &value)| value == id)
                    .unwrap()
                    .0;
                let mut row = RECT::default();
                *(&mut row as *mut RECT as *mut isize) = handle;
                assert_ne!(
                    SendMessageW(tree, TVM_GETITEMRECT, 1, &mut row as *mut _ as isize),
                    0
                );
                point = POINT {
                    x: row.left + app.theme.px(4),
                    y: (row.top + row.bottom) / 2,
                };
            });
            let coordinates = (point.x as u16 as u32 | ((point.y as u16 as u32) << 16)) as isize;
            if !release {
                // Stage a held press before injecting the drag notification, without a release.
                with_app(|app| {
                    app.tree_mouse_input = true;
                    app.tree_mouse_down(
                        point,
                        false,
                        modifiers & MK_CONTROL as usize != 0,
                        modifiers & MK_SHIFT as usize != 0,
                    );
                    app.tree_mouse_input = false;
                });
                return;
            }
            // Native TreeView's synchronous DragDetect reads the thread queue.
            PostMessageW(tree, WM_LBUTTONUP, modifiers, coordinates);
            SendMessageW(tree, WM_LBUTTONDOWN, modifiers | 1, coordinates);
            let mut pending = MSG::default();
            while PeekMessageW(&mut pending, tree, WM_LBUTTONUP, WM_LBUTTONUP, PM_REMOVE) != 0 {
                DispatchMessageW(&pending);
            }
        }
    }

    unsafe fn verify_rename() {
        unsafe {
            let mut editor = null_mut();
            let mut name_edit = null_mut();
            let mut hwnd = null_mut();
            let mut tree = null_mut();
            let mut tabs = null_mut();
            let mut id = 0;
            let mut selected = Vec::new();
            let mut original_title = String::new();
            let mut tree_title = String::new();
            let mut settings = Settings::default();
            with_app(|app| {
                editor = app.edit;
                hwnd = app.hwnd;
                tree = app.tree;
                tabs = app.tabs;
                id = app.open[app.active.unwrap()].id;
                selected = app.open[..2].iter().map(|tab| tab.id).collect();
                original_title = app.book.get(id).unwrap().title.clone();
                tree_title = app.book.get(selected[0]).unwrap().title.clone();
                settings = app.settings.clone();
                app.selection.all(&selected);
                app.sidebar_visible = false;
                app.layout();
            });
            editing::select(editor, editing::CharRange { start: 1, end: 3 });
            SendMessageW(
                editor,
                EM_REPLACESEL,
                1,
                wide("이름 변경 검증 ").as_ptr() as isize,
            );
            let original_text = window_text(editor);
            let selection = editing::selection(editor);
            let undo = SendMessageW(editor, EM_CANUNDO, 0, 0);
            assert_ne!(undo, 0);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                with_app(|app| {
                    let mut next = app.settings.clone();
                    next.theme = mode;
                    app.apply_settings(next);
                });
                SetFocus(editor);
                assert!(shortcut(&MSG {
                    hwnd: editor,
                    message: WM_KEYDOWN,
                    wParam: VK_F2 as usize,
                    ..Default::default()
                }));
                with_app(|app| {
                    name_edit = app.tab_name_edit;
                    assert_eq!(app.tab_rename.unwrap().id, id);
                    assert_eq!(GetParent(name_edit), tabs);
                    assert_eq!(GetFocus(), name_edit);
                    assert_ne!(IsWindowVisible(name_edit), 0);
                    assert!(!app.sidebar_visible);
                    assert_eq!(*app.selection.ids, selected.iter().copied().collect());
                    assert_eq!(SendMessageW(tree, TVM_GETEDITCONTROL, 0, 0), 0);
                    assert_eq!(
                        SendMessageW(name_edit, WM_GETFONT, 0, 0),
                        app.theme.tab as isize
                    );
                    let mut area = RECT::default();
                    GetClientRect(name_edit, &mut area);
                    let screen = GetDC(hwnd);
                    let dc = CreateCompatibleDC(screen);
                    let bitmap = CreateCompatibleBitmap(screen, area.right, area.bottom);
                    let original = SelectObject(dc, bitmap);
                    let brush =
                        SendMessageW(tabs, WM_CTLCOLOREDIT, dc as usize, name_edit as isize);
                    assert_eq!(brush, app.theme.white_brush as isize);
                    assert_eq!(GetBkColor(dc), app.theme.colors.white);
                    assert_eq!(GetTextColor(dc), app.theme.colors.ink);
                    SendMessageW(name_edit, EM_SETSEL, 0, 0);
                    theme::fill(dc, area, theme::rgb(255, 0, 255));
                    SendMessageW(
                        name_edit,
                        WM_PRINTCLIENT,
                        dc as usize,
                        (PRF_CLIENT | PRF_ERASEBKGND) as isize,
                    );
                    assert_eq!(
                        GetPixel(dc, area.right - 1, area.bottom - 1),
                        app.theme.colors.white
                    );
                    SelectObject(dc, original);
                    DeleteObject(bitmap);
                    GetClientRect(tabs, &mut area);
                    let bitmap = CreateCompatibleBitmap(screen, area.right, area.bottom);
                    let original = SelectObject(dc, bitmap);
                    let left = app.tab_layout.get().bounds(app.active.unwrap()).0;
                    for message in [WM_PRINTCLIENT, WM_PRINT] {
                        theme::fill(dc, area, theme::rgb(255, 0, 255));
                        SendMessageW(
                            tabs,
                            message,
                            dc as usize,
                            (PRF_CLIENT | PRF_CHILDREN) as isize,
                        );
                        assert_eq!(
                            GetPixel(dc, left + app.theme.px(60), app.theme.px(6)),
                            app.theme.colors.blue
                        );
                        assert_eq!(GetFocus(), name_edit);
                    }
                    SelectObject(dc, original);
                    DeleteObject(bitmap);
                    DeleteDC(dc);
                    ReleaseDC(hwnd, screen);
                });
                SetWindowTextW(name_edit, wide("  변경한 탭 이름 한글😀  ").as_ptr());
                SendMessageW(name_edit, EM_SETSEL, 2, 4);
                assert!(shortcut(&MSG {
                    hwnd: name_edit,
                    message: WM_KEYDOWN,
                    wParam: VK_F2 as usize,
                    ..Default::default()
                }));
                let mut start = 0u32;
                let mut end = 0u32;
                SendMessageW(
                    name_edit,
                    EM_GETSEL,
                    &mut start as *mut _ as usize,
                    &mut end as *mut _ as isize,
                );
                assert_eq!((start, end), (2, 4)); // Repeated F2 keeps edits and selection.
                SendMessageW(name_edit, WM_KEYDOWN, VK_RETURN as usize, 0);
                with_app(|app| {
                    assert!(app.tab_rename.is_none());
                    assert_eq!(IsWindowVisible(name_edit), 0);
                    assert_eq!(GetFocus(), editor);
                    assert!(!app.sidebar_visible);
                    assert_eq!(app.book.get(id).unwrap().title, "변경한 탭 이름 한글😀");
                    assert_eq!(
                        Notebook::load(&app.data_dir)
                            .unwrap()
                            .get(id)
                            .unwrap()
                            .title,
                        "변경한 탭 이름 한글😀"
                    );
                    assert_eq!(app.edit, editor);
                    assert_eq!(window_text(editor), original_text);
                    assert_eq!(editing::selection(editor).start, selection.start);
                    assert_eq!(editing::selection(editor).end, selection.end);
                    assert_eq!(SendMessageW(editor, EM_CANUNDO, 0, 0), undo);
                    assert!(app.open[app.active.unwrap()].dirty);
                });
                // Cancellation and whitespace keep the saved title.
                for (text, key) in [("취소할 이름", VK_ESCAPE), ("   ", VK_RETURN)] {
                    assert!(shortcut(&MSG {
                        hwnd: editor,
                        message: WM_KEYDOWN,
                        wParam: VK_F2 as usize,
                        ..Default::default()
                    }));
                    assert_eq!(GetFocus(), name_edit); // Reuses the native editor.
                    SetWindowTextW(name_edit, wide(text).as_ptr());
                    SendMessageW(name_edit, WM_KEYDOWN, key as usize, 0);
                    with_app(|app| {
                        assert_eq!(app.book.get(id).unwrap().title, "변경한 탭 이름 한글😀")
                    });
                }
            }
            // A queued focus-loss commit must preserve the newly focused control.
            SetFocus(tabs);
            assert!(shortcut(&MSG {
                hwnd: tabs,
                message: WM_KEYDOWN,
                wParam: VK_F2 as usize,
                ..Default::default()
            }));
            with_app(|app| {
                app.sidebar_visible = true;
                app.layout();
            });
            SetWindowPos(
                hwnd,
                null_mut(),
                0,
                0,
                560,
                760,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            with_app(|app| {
                assert!(app.tab_layout.get().overflow);
                let index = app.open.iter().position(|tab| tab.id == id).unwrap();
                let (left, right) = app.tab_layout.get().bounds(index);
                let mut area = RECT::default();
                GetWindowRect(name_edit, &mut area);
                let mut point = POINT {
                    x: area.left,
                    y: area.top,
                };
                ScreenToClient(tabs, &mut point);
                assert_eq!(point.x, left + app.theme.px(33));
                assert_eq!(area.right - area.left, right - left - app.theme.px(61));
                assert_eq!(GetFocus(), name_edit);
                assert!(app.sidebar_visible);
            });
            SetWindowPos(
                hwnd,
                null_mut(),
                0,
                0,
                1100,
                760,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            with_app(|app| {
                app.sidebar_visible = false;
                app.layout();
            });
            SetWindowTextW(name_edit, wide("포커스 이동 저장").as_ptr());
            SetFocus(editor);
            let mut pending = MSG::default();
            while PeekMessageW(
                &mut pending,
                hwnd,
                FINISH_TAB_RENAME,
                FINISH_TAB_RENAME,
                PM_REMOVE,
            ) != 0
            {
                DispatchMessageW(&pending);
            }
            with_app(|app| {
                assert!(app.tab_rename.is_none());
                assert_eq!(app.book.get(id).unwrap().title, "포커스 이동 저장");
                assert_eq!(GetFocus(), editor);
                assert!(!app.sidebar_visible);
                app.rename_current(editor);
                SetWindowTextW(name_edit, wide("탭 전환 전 저장").as_ptr());
                assert!(app.activate(0));
                assert!(app.tab_rename.is_none());
                assert_eq!(app.book.get(id).unwrap().title, "탭 전환 전 저장");
                assert!(!app.sidebar_visible);
                let index = app.open.iter().position(|tab| tab.id == id).unwrap();
                assert!(app.activate(index));
                app.rename_current(editor);
                SetWindowTextW(name_edit, wide("저장 실패 이름").as_ptr());
                let blocker = app.data_dir.join("tree.tmp");
                fs::create_dir(&blocker).unwrap();
                assert!(app.finish_tab_rename(true, true).is_err());
                fs::remove_dir(&blocker).unwrap();
                assert_eq!(app.book.get(id).unwrap().title, "탭 전환 전 저장");
                assert_eq!(
                    Notebook::load(&app.data_dir)
                        .unwrap()
                        .get(id)
                        .unwrap()
                        .title,
                    "탭 전환 전 저장"
                );
                app.rename_note(id, &original_title).unwrap();
                app.sidebar_visible = true;
                app.selection.single(selected[0]);
                let handle = *app
                    .handles
                    .iter()
                    .find(|&(_, &value)| value == selected[0])
                    .unwrap()
                    .0;
                SendMessageW(tree, TVM_SELECTITEM, TVGN_CARET as usize, handle);
                app.layout();
            });
            SetFocus(tree);
            assert!(shortcut(&MSG {
                hwnd: tree,
                message: WM_KEYDOWN,
                wParam: VK_F2 as usize,
                ..Default::default()
            }));
            let tree_edit = SendMessageW(tree, TVM_GETEDITCONTROL, 0, 0) as HWND;
            assert!(!tree_edit.is_null());
            with_app(|app| assert!(app.tab_rename.is_none()));
            SetWindowTextW(tree_edit, wide("사이드바에서 수정").as_ptr());
            assert!(shortcut(&MSG {
                hwnd: tree_edit,
                message: WM_KEYDOWN,
                wParam: VK_F2 as usize,
                ..Default::default()
            }));
            assert_eq!(GetFocus(), tree_edit);
            assert_eq!(window_text(tree_edit), "사이드바에서 수정");
            SendMessageW(tree, TVM_ENDEDITLABELNOW, 0, 0);
            with_app(|app| {
                assert_eq!(
                    app.book.get(selected[0]).unwrap().title,
                    "사이드바에서 수정"
                );
                app.selection.all(&selected);
            });
            SetFocus(tree);
            assert!(shortcut(&MSG {
                hwnd: tree,
                message: WM_KEYDOWN,
                wParam: VK_F2 as usize,
                ..Default::default()
            }));
            assert_eq!(SendMessageW(tree, TVM_GETEDITCONTROL, 0, 0), 0);
            // A visible sidebar keeps its multi-selection restriction regardless of focus.
            SetFocus(editor);
            assert!(shortcut(&MSG {
                hwnd: editor,
                message: WM_KEYDOWN,
                wParam: VK_F2 as usize,
                ..Default::default()
            }));
            with_app(|app| {
                assert!(app.tab_rename.is_none());
                assert_eq!(SendMessageW(tree, TVM_GETEDITCONTROL, 0, 0), 0);
                assert_eq!(*app.selection.ids, selected.iter().copied().collect());
                app.selection.single(selected[0]);
            });
            // Both body and tab focus edit the selected sidebar row when it is visible.
            for focus in [editor, tabs] {
                SetFocus(focus);
                assert!(shortcut(&MSG {
                    hwnd: focus,
                    message: WM_KEYDOWN,
                    wParam: VK_F2 as usize,
                    ..Default::default()
                }));
                let tree_edit = SendMessageW(tree, TVM_GETEDITCONTROL, 0, 0) as HWND;
                assert!(!tree_edit.is_null());
                assert_eq!(GetFocus(), tree_edit);
                assert_eq!(window_text(tree_edit), "사이드바에서 수정");
                with_app(|app| {
                    assert!(app.tab_rename.is_none());
                    assert_eq!(app.open[app.active.unwrap()].id, id);
                    assert!(app.sidebar_visible);
                });
                SendMessageW(tree, TVM_ENDEDITLABELNOW, 1, 0);
            }
            with_app(|app| {
                assert!(app.sidebar_visible);
                app.rename_note(selected[0], &tree_title).unwrap();
                app.apply_settings(settings);
                app.layout();
            });
            SendMessageW(editor, WM_UNDO, 0, 0);
            assert_eq!(window_text(editor), format!("본문 {id}"));
        }
    }

    unsafe fn verify_interactions() {
        unsafe {
            InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_TREEVIEW_CLASSES | ICC_BAR_CLASSES,
            });
            assert!(!LoadLibraryW(wide("Msftedit.dll").as_ptr()).is_null());
            let dir = std::env::temp_dir().join(format!(
                "edot-interactions-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let mut book = Notebook::load(&dir).unwrap();
            let ids: Vec<_> = (0..4).map(|i| book.add(0, format!("메모 {i}"))).collect();
            let child = book.add(ids[0], "하위 메모".into());
            for &id in ids.iter().chain([child].iter()) {
                fs::write(book.file(&dir, id), format!("본문 {id}")).unwrap();
            }
            book.save(&dir).unwrap();
            let instance = GetModuleHandleW(null());
            let class = wide("EdotInteractionTestWindow");
            assert_ne!(
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance,
                    lpszClassName: class.as_ptr(),
                    ..Default::default()
                }),
                0
            );
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                windows_sys::w!("test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                1100,
                760,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            assert!(!hwnd.is_null());
            struct WindowGuard(HWND);
            impl Drop for WindowGuard {
                fn drop(&mut self) {
                    unsafe {
                        DestroyWindow(self.0);
                    }
                }
            }
            let guard = WindowGuard(hwnd);
            APP.with(|cell| {
                *cell.borrow_mut() = Some(App::new(hwnd, dir.clone(), book, Settings::default()))
            });
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let mut tree = null_mut();
            let mut tabs = null_mut();
            with_app(|app| {
                tree = app.tree;
                assert!(app.open.is_empty());
            });
            click_tree(tree, ids[0], 0, true);
            with_app(|app| {
                assert_eq!(app.open.len(), 1);
                assert_eq!(app.open[app.active.unwrap()].id, ids[0]);
                assert_eq!(window_text(app.edit), format!("본문 {}", ids[0]));
                assert_eq!(GetFocus(), app.edit);
                assert!(app.tree_press.is_none());
            });
            with_app(|app| {
                for &id in &ids {
                    app.open_note(id);
                }
                tabs = app.tabs;
            });
            verify_rename();
            click_tree(tree, ids[0], 0, true);
            with_app(|app| {
                assert_eq!(app.open[app.active.unwrap()].id, ids[0]);
                assert_eq!(GetFocus(), app.edit);
                assert!(app.tree_press.is_none());
            });
            click_tree(tree, ids[2], MK_CONTROL as usize, true);
            with_app(|app| {
                assert_eq!(*app.selection.ids, HashSet::from([ids[0], ids[2]]));
                assert_eq!(app.open.len(), 4); // Ctrl selection does not open new tabs.
                assert_eq!(app.open[app.active.unwrap()].id, ids[0]);
            });
            click_tree(tree, ids[0], MK_CONTROL as usize, true);
            with_app(|app| assert_eq!(*app.selection.ids, HashSet::from([ids[2]])));
            let mut keys = [0u8; 256];
            GetKeyboardState(keys.as_mut_ptr());
            let mut ctrl_keys = keys;
            ctrl_keys[VK_CONTROL as usize] = 0x80;
            SetKeyboardState(ctrl_keys.as_ptr()); // Private test thread's keyboard queue only.
            assert!(shortcut(&MSG {
                hwnd: tree,
                message: WM_KEYDOWN,
                wParam: b'A' as usize,
                ..Default::default()
            }));
            with_app(|app| assert_eq!(app.selection.ids.len(), 5));
            SendMessageW(tree, WM_KEYDOWN, VK_SPACE as usize, 0);
            with_app(|app| {
                assert_eq!(app.selection.ids.len(), 4);
                assert!(!app.selection.ids.contains(&ids[0]));
            });
            SetKeyboardState(keys.as_ptr());
            click_tree(tree, ids[0], 0, true);
            click_tree(tree, ids[1], MK_SHIFT as usize, true);
            with_app(|app| {
                assert_eq!(*app.selection.ids, HashSet::from([ids[0], child, ids[1]]));
                assert_eq!(app.open[app.active.unwrap()].id, ids[0]);
            });
            // Right-clicking a selected row keeps the batch without opening a tab.
            with_app(|app| {
                let handle = *app.handles.iter().find(|&(_, &id)| id == ids[1]).unwrap().0;
                let mut row = RECT::default();
                *(&mut row as *mut RECT as *mut isize) = handle;
                SendMessageW(tree, TVM_GETITEMRECT, 1, &mut row as *mut _ as isize);
                app.tree_mouse_down(
                    POINT {
                        x: row.left + 4,
                        y: (row.top + row.bottom) / 2,
                    },
                    true,
                    false,
                    false,
                );
                assert_eq!(app.selection.ids.len(), 3);
            });
            // Custom rows use the same selection set in both palettes.
            for dark in [false, true] {
                with_app(|app| {
                    app.theme = Rc::new(Theme::new(app.theme.dpi, dark, "Consolas"));
                    app.apply_theme(true, true, true);
                    app.publish_view();
                    let screen = GetDC(hwnd);
                    let dc = CreateCompatibleDC(screen);
                    let bitmap = CreateCompatibleBitmap(screen, 700, 700);
                    let original = SelectObject(dc, bitmap);
                    for id in [ids[0], ids[1], ids[2]] {
                        let handle = *app
                            .handles
                            .iter()
                            .find(|&(_, &value)| value == id)
                            .unwrap()
                            .0;
                        let mut row = RECT::default();
                        *(&mut row as *mut RECT as *mut isize) = handle;
                        SendMessageW(tree, TVM_GETITEMRECT, 0, &mut row as *mut _ as isize);
                        ui::with_view(|view| {
                            view.paint_tree_item(&NMTVCUSTOMDRAW {
                                nmcd: NMCUSTOMDRAW {
                                    hdc: dc,
                                    rc: row,
                                    dwItemSpec: handle as usize,
                                    ..Default::default()
                                },
                                ..Default::default()
                            })
                        });
                        let expected = if id == ids[2] {
                            app.theme.colors.sidebar
                        } else {
                            app.theme.colors.selected
                        };
                        assert_eq!(
                            GetPixel(dc, app.theme.px(6), (row.top + row.bottom) / 2),
                            expected
                        );
                    }
                    SelectObject(dc, original);
                    DeleteObject(bitmap);
                    DeleteDC(dc);
                    ReleaseDC(hwnd, screen);
                });
            }
            with_app(|app| {
                app.activate(2);
                SetFocus(tree);
            });
            click_tree(tree, ids[0], 0, false);
            with_app(|app| {
                assert_eq!(app.selection.ids.len(), 3); // Kept until click-up or drag.
                assert_eq!(app.open[app.active.unwrap()].id, ids[0]); // Opens before release.
            });
            let mut event = NMTREEVIEWW::default();
            event.hdr.hwndFrom = tree;
            event.hdr.code = TVN_BEGINDRAGW;
            event.itemNew.lParam = ids[0] as isize;
            SendMessageW(hwnd, WM_NOTIFY, ID_TREE, &event as *const _ as isize);
            with_app(|app| {
                assert_eq!(&*app.drag_sources, &[ids[0], ids[1]]);
                let target = DropTarget::resolve_many(
                    &app.book,
                    &app.drag_sources,
                    Some(ids[2]),
                    DropZone::Inside,
                );
                app.finish_drag_to(target);
                assert_eq!(app.book.children(ids[2]), vec![ids[0], ids[1]]);
                assert_eq!(app.book.get(child).unwrap().parent, ids[0]);
                assert_eq!(app.selection.ids.len(), 3);
                assert!(app.dragging.is_none());
                assert_eq!(GetCapture(), null_mut());
            });
            click_tree(tree, ids[0], 0, true);
            with_app(|app| assert_eq!(*app.selection.ids, HashSet::from([ids[0]])));

            // Reorder a dirty editor through actual tab subclass messages.
            let mut x = 0;
            let mut editor = null_mut();
            let mut selection = editing::CharRange { start: 0, end: 0 };
            with_app(|app| {
                app.activate(3);
                editor = app.edit;
                editing::select(editor, editing::CharRange { start: 0, end: 0 });
                SendMessageW(editor, EM_REPLACESEL, 1, wide("수정 ").as_ptr() as isize);
                selection = editing::selection(editor);
                assert_ne!(SendMessageW(editor, EM_CANUNDO, 0, 0), 0);
                x = app.tab_layout.get().bounds(3).0 + app.theme.px(35);
            });
            let y = 15;
            SendMessageW(tabs, WM_LBUTTONDOWN, 1, ((y << 16) | x) as isize);
            SendMessageW(tabs, WM_MOUSEMOVE, 1, (y << 16) as isize);
            with_app(|app| assert_eq!(app.tab_drag.unwrap().gap, 0));
            SendMessageW(tabs, WM_LBUTTONUP, 0, (y << 16) as isize);
            SendMessageW(tabs, WM_LBUTTONDOWN, 1, ((y << 16) | 35) as isize);
            SendMessageW(tabs, WM_MOUSEMOVE, 1, ((y << 16) | 350) as isize);
            assert!(shortcut(&MSG {
                hwnd: tabs,
                message: WM_KEYDOWN,
                wParam: VK_ESCAPE as usize,
                ..Default::default()
            }));
            with_app(|app| {
                assert!(app.tab_drag.is_none());
                assert_eq!(GetCapture(), null_mut());
            });
            with_app(|app| {
                assert_eq!(
                    app.open.iter().map(|t| t.id).collect::<Vec<_>>(),
                    vec![ids[3], ids[0], ids[1], ids[2]]
                );
                assert_eq!(app.active, Some(0));
                assert_eq!(app.edit, editor);
                assert_eq!(editing::selection(editor), selection);
                assert_ne!(SendMessageW(editor, EM_CANUNDO, 0, 0), 0);
                assert_eq!(
                    fs::read_to_string(dir.join("session.tsv"))
                        .unwrap()
                        .lines()
                        .skip(2)
                        .map(|s| s.parse::<u64>().unwrap())
                        .collect::<Vec<_>>(),
                    vec![ids[3], ids[0], ids[1], ids[2]]
                );
                fs::create_dir(dir.join("session.tmp")).unwrap();
                assert!(app.reorder_tab(ids[3], 4).is_err());
                assert_eq!(app.active, Some(0));
                assert_eq!(app.open[0].id, ids[3]);
                fs::remove_dir(dir.join("session.tmp")).unwrap();
                app.selection.all(&[ids[0], ids[1]]);
                let menu = sidebar_multi_menu();
                assert_eq!(GetMenuItemCount(menu), 2);
                assert_eq!(GetMenuItemID(menu, 0), MENU_DELETE as u32);
                assert_eq!(GetMenuItemID(menu, 1), MENU_GROUP as u32);
                DestroyMenu(menu);
                let before = app.book.notes.clone();
                let next_id = app.book.next_id;
                fs::create_dir(dir.join("tree.tmp")).unwrap();
                assert!(app.group_selected_notes().is_err());
                assert_eq!(app.book.notes, before);
                assert_eq!(app.book.next_id, next_id);
                assert!(!app.book.file(&dir, next_id).exists());
                assert_eq!(app.selection.ids.len(), 2);
                fs::remove_dir(dir.join("tree.tmp")).unwrap();
                let folder = app.group_selected_notes().unwrap().unwrap();
                assert_eq!(app.book.children(folder), vec![ids[0], ids[1]]);
                assert_eq!(app.book.get(folder).unwrap().parent, ids[2]);
                assert_eq!(app.edit, editor);
                assert_eq!(editing::selection(editor), selection);
                assert!(app.save_all());
                app.delete_notes_confirmed(&[folder, ids[0], child])
                    .unwrap();
                assert_eq!(
                    app.open.iter().map(|t| t.id).collect::<Vec<_>>(),
                    vec![ids[3], ids[2]]
                );
                assert_eq!(app.edit, editor);
                assert_eq!(editing::selection(editor), selection);
                assert_ne!(SendMessageW(editor, EM_CANUNDO, 0, 0), 0);
                assert_eq!(fs::read_dir(dir.join("trash")).unwrap().count(), 4);
                assert!(
                    fs::read_to_string(app.book.file(&dir, ids[3]))
                        .unwrap()
                        .starts_with("수정 ")
                );
                assert_eq!(
                    Notebook::load(&dir).unwrap().children(0),
                    vec![ids[2], ids[3]]
                );
            });
            drop(guard);
            UnregisterClassW(class.as_ptr(), instance);
            assert!(
                dir.canonicalize()
                    .unwrap()
                    .starts_with(std::env::temp_dir().canonicalize().unwrap())
            );
            fs::remove_dir_all(dir).unwrap();
        }
    }
}
