use crate::drag::{DropTarget, DropZone};
use crate::model::{Notebook, replace_file};
use crate::settings::{self, Panel, Settings, ThemeMode};
use crate::tabs::TabLayout;
use crate::theme::{self, Theme};
use crate::tooltip::Tooltip;
use crate::{editing, startup, ui};
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
use windows_sys::Win32::System::SystemServices::{SS_NOTIFY, SS_OWNERDRAW};
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
const AUTOSAVE_TIMER: usize = 1;
const DRAG_SCROLL_TIMER: usize = 2;
const POSITION_RENAME: u32 = WM_APP + 1;
const REFRESH_UI: u32 = WM_APP + 2;

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
    edit: HWND,
    empty_edit: HWND,
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
    tab_drag: bool,
    tree_scroll_drag: bool,
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
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                ui::paint_window(hwnd, |dc| ui::with_view(|v| v.paint_tabs(dc)));
                return 0;
            }
            WM_LBUTTONDOWN => {
                with_app(|app| app.tab_click(point_x(lp), point_y(lp)));
                return 0;
            }
            WM_MOUSEMOVE => {
                with_app(|app| {
                    if app.tab_drag {
                        app.drag_tab_scroll(point_x(lp));
                    }
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
                with_app(|app| app.tab_drag = false);
                ReleaseCapture();
                return 0;
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta = hi(wp) as u16 as i16 as i32;
                with_app(|app| app.scroll_tabs(if msg == WM_MOUSEWHEEL { -delta } else { delta }));
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
            WM_COMMAND | WM_DRAWITEM | WM_MEASUREITEM | WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC
            | WM_CTLCOLORLISTBOX => {
                return SendMessageW(GetParent(hwnd), msg, wp, lp);
            }
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                ui::paint_window(hwnd, |dc| ui::with_view(|view| view.paint_settings(dc)));
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
    let mut route = 0;
    unsafe {
        ui::with_view(|view| {
            let mut point = POINT::default();
            GetCursorPos(&mut point);
            let mut bounds = RECT::default();
            GetWindowRect(view.tabs, &mut bounds);
            if PtInRect(&bounds, point) != 0 {
                route = 1;
            }
            GetWindowRect(GetParent(view.tree), &mut bounds);
            if view.sidebar_visible && PtInRect(&bounds, point) != 0 {
                route = 2;
            }
        });
        let delta = hi(wp) as u16 as i16 as i32;
        if route == 1 {
            with_app(|app| app.scroll_tabs(if msg == WM_MOUSEWHEEL { -delta } else { delta }));
        }
        if route == 2 {
            with_app(|app| app.scroll_tree(-delta / 40));
        }
    }
    route != 0
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
        if msg == WM_MOUSEWHEEL {
            let delta = hi(wp) as u16 as i16 as i32;
            with_app(|app| app.scroll_tree(-delta / 40));
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
        if matches!(msg, WM_LBUTTONDOWN | WM_RBUTTONDOWN) {
            with_app(|app| app.tooltip.hide());
        }
        let result = DefSubclassProc(hwnd, msg, wp, lp);
        if msg == WM_KEYDOWN {
            ui::with_view(|v| {
                InvalidateRect(v.tree_scroll, null(), 0);
            });
        }
        result
    }
}

unsafe extern "system" fn scroll_subclass(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _reference: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                ui::paint_window(hwnd, |dc| ui::with_view(|v| v.paint_tree_scroll(dc)));
                return 0;
            }
            WM_LBUTTONDOWN => {
                SetCapture(hwnd);
                with_app(|a| {
                    a.tree_scroll_drag = true;
                    a.drag_tree_scroll(point_y(lp));
                });
                return 0;
            }
            WM_MOUSEMOVE => {
                with_app(|a| {
                    if a.tree_scroll_drag {
                        a.drag_tree_scroll(point_y(lp));
                    }
                });
                return 0;
            }
            WM_LBUTTONUP => {
                ReleaseCapture();
                with_app(|a| a.tree_scroll_drag = false);
                return 0;
            }
            WM_MOUSEWHEEL => {
                with_app(|a| a.scroll_tree(-(hi(wp) as u16 as i16 as i32) / 40));
                return 0;
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(scroll_subclass), id);
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
        if reference == 2 && matches!(msg, WM_MOUSEWHEEL | WM_MOUSEHWHEEL) && route_wheel(msg, wp) {
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
                common | WS_TABSTOP | SS_NOTIFY,
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
        let edit = unsafe { memo_editor(hwnd) };
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
            SetWindowSubclass(tree_scroll, Some(scroll_subclass), 1, 0);
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
            edit,
            empty_edit: edit,
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
            tab_drag: false,
            tree_scroll_drag: false,
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
            self.tooltip.style(self.theme.small, self.theme.px(420));
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
            let mut scroll = POINT::default();
            SendMessageW(
                editor,
                editing::EM_GETSCROLLPOS,
                0,
                &mut scroll as *mut _ as LPARAM,
            );
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
                let style = GetWindowLongPtrW(editor, GWL_STYLE) as u32;
                let style = if self.settings.wrap {
                    style & !WS_HSCROLL
                } else {
                    style | WS_HSCROLL
                };
                SetWindowLongPtrW(editor, GWL_STYLE, style as isize);
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
                    ShowScrollBar(editor, SB_HORZ, 0);
                    scroll.x = 0;
                }
            }
            SendMessageW(
                editor,
                editing::EM_SETSCROLLPOS,
                0,
                &scroll as *const _ as LPARAM,
            );
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
                self.settings_panel = Some(Panel {
                    host,
                    theme,
                    font,
                    wrap,
                    close,
                    theme_label,
                    font_label,
                });
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
            SetWindowPos(
                panel.host,
                HWND_TOP,
                area.right - width - p(12),
                p(42),
                width,
                p(272),
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            for (control, x, y, w, h) in [
                (panel.close, width - p(66), p(12), p(48), p(28)),
                (panel.theme_label, p(20), p(52), width - p(40), p(22)),
                (panel.theme, p(20), p(76), width - p(40), p(230)),
                (panel.font_label, p(20), p(117), width - p(40), p(22)),
                (panel.font, p(20), p(141), width - p(40), p(280)),
                (panel.wrap, p(20), p(191), width - p(40), p(30)),
            ] {
                SetWindowPos(
                    control,
                    null_mut(),
                    x,
                    y,
                    w,
                    h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            RedrawWindow(
                panel.host,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
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
            }
            report(self.hwnd, "설정을 저장하지 못했습니다.", error);
            return;
        }
        unsafe {
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
            edit: self.edit,
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
            tree_scroll: self.tree_scroll,
            visible: Rc::clone(&self.visible),
            find_visible: self.find_visible,
            replace_visible: self.replace_visible,
            resizing: self.resizing,
            tree_hover: self.tree_hover,
            dragging: self.dragging,
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
            let editor_y = p(58);
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
                    left - p(10),
                    p(62),
                    p(6),
                    (h - p(70)).max(0),
                ),
                (self.tabs, tabs_x, 0, tabs.viewport, p(36)),
                (self.settings_button, w - p(36), p(3), p(30), p(29)),
                (
                    self.tab_add,
                    tabs_x + tabs.viewport + p(3),
                    p(3),
                    p(30),
                    p(29),
                ),
                (
                    self.edit,
                    content_x + p(24),
                    editor_y,
                    (w - content_x - p(40)).max(0),
                    (h - editor_y - p(52)).max(0),
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
                (self.edit, self.active.is_some()),
            ] {
                if (IsWindowVisible(hwnd) != 0) != show {
                    ShowWindow(hwnd, if show { SW_SHOWNA } else { SW_HIDE });
                }
            }
            self.layout_search();
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
            if self.tab_drag || self.dragging.is_some() || y >= self.theme.px(32) {
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
            if layout.overflow && y >= self.theme.px(32) {
                self.tab_drag = true;
                SetCapture(self.tabs);
                self.drag_tab_scroll(x);
                return;
            }
            if let Some(index) = layout.hit(x) {
                if x >= layout.bounds(index).1 - self.theme.px(24) {
                    self.close_tab(index);
                } else {
                    self.activate(index);
                }
            }
        }
    }
    unsafe fn scroll_tabs(&self, delta: i32) {
        unsafe {
            self.tooltip.dismiss();
        }
        let mut tabs = self.tab_layout.get();
        tabs.scroll(delta * self.theme.px(100) / 120);
        self.tab_layout.set(tabs);
        self.publish_view();
        unsafe {
            InvalidateRect(self.tabs, null(), 0);
        }
    }
    unsafe fn drag_tab_scroll(&self, x: i32) {
        let mut tabs = self.tab_layout.get();
        let total = tabs.width * tabs.count as i32;
        let thumb = (tabs.viewport * tabs.viewport / total.max(1)).max(self.theme.px(24));
        tabs.offset = ((x - thumb / 2).max(0) * (total - tabs.viewport).max(0)
            / (tabs.viewport - thumb).max(1))
        .clamp(0, (total - tabs.viewport).max(0));
        self.tab_layout.set(tabs);
        self.publish_view();
        unsafe {
            InvalidateRect(self.tabs, null(), 0);
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
    unsafe fn drag_tree_scroll(&self, y: i32) {
        unsafe {
            let mut area = RECT::default();
            GetClientRect(self.tree_scroll, &mut area);
            let count = SendMessageW(self.tree, TVM_GETVISIBLECOUNT, 0, 0).max(1) as i32;
            let total = self.visible.len() as i32;
            let thumb = (area.bottom * count / total.max(1))
                .max(self.theme.px(24))
                .min(area.bottom);
            let index = ((y - thumb / 2).max(0) * (total - count).max(0)
                / (area.bottom - thumb).max(1))
            .clamp(0, (total - count).max(0)) as usize;
            if let Some(&item) = self.visible.get(index) {
                SendMessageW(self.tree, TVM_SELECTITEM, TVGN_FIRSTVISIBLE as usize, item);
            }
            InvalidateRect(self.tree_scroll, null(), 0);
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
        let editor = unsafe { memo_editor(self.hwnd) };
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
        if index >= self.open.len() {
            return;
        }
        if !unsafe { self.save_all() } {
            return;
        }
        let previous_active = self.active;
        let closed = self.open.remove(index);
        if self.edit == closed.editor {
            self.edit = self.empty_edit;
        }
        unsafe {
            if !closed.editor.is_null() {
                DestroyWindow(closed.editor);
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
            let next = match previous_active {
                Some(active) if active == index => index.min(self.open.len() - 1),
                Some(active) if active > index => active - 1,
                Some(active) => active,
                None => 0,
            };
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

    unsafe fn rename_from_focus(&mut self, focus: HWND) {
        unsafe {
            if SendMessageW(self.tree, TVM_GETEDITCONTROL, 0, 0) != 0 {
                self.rename_selected();
                return;
            }
            let target = if focus == self.tree || IsChild(self.tree, focus) != 0 {
                self.selected_id()
            } else {
                self.active.map(|index| self.open[index].id)
            };
            let Some(id) = target else {
                return;
            };
            let Some((&handle, _)) = self.handles.iter().find(|&(_, &value)| value == id) else {
                return;
            };
            self.tooltip.hide();
            if !self.sidebar_visible {
                self.sidebar_visible = true;
                self.layout();
            }
            SendMessageW(self.tree, TVM_SELECTITEM, TVGN_CARET as usize, handle);
            SendMessageW(self.tree, TVM_ENSUREVISIBLE, 0, handle);
            if self.refresh_tree_visibility() {
                self.layout();
            }
            InvalidateRect(self.tree_scroll, null(), 0);
            self.rename_selected();
        }
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
        let Some(id) = (unsafe { self.selected_id() }) else {
            return;
        };
        let ids = self.book.subtree(id);
        let prompt = if ids.len() == 1 {
            "이 메모를 삭제할까요?\n삭제한 파일은 휴지통 폴더에 보관됩니다.".to_owned()
        } else {
            format!(
                "이 메모와 하위 메모 {}개를 삭제할까요?\n삭제한 파일은 휴지통 폴더에 보관됩니다.",
                ids.len() - 1
            )
        };
        if unsafe { message(self.hwnd, "메모 삭제", &prompt, MB_YESNO | MB_ICONQUESTION) } != IDYES
        {
            return;
        }
        if !unsafe { self.save_all() } {
            return;
        }
        let trash = self.data_dir.join("trash");
        if let Err(error) = fs::create_dir_all(&trash) {
            report(self.hwnd, "휴지통을 만들지 못했습니다.", error);
            return;
        }
        let mut moved = Vec::new();
        for &note_id in &ids {
            let source = self.book.file(&self.data_dir, note_id);
            let target = trash.join(format!(
                "{}-{}.txt",
                note_id,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ));
            if let Err(error) = fs::rename(&source, &target) {
                for (from, to) in moved.iter().rev() {
                    let _ = fs::rename(to, from);
                }
                report(self.hwnd, "메모를 휴지통으로 옮기지 못했습니다.", error);
                return;
            }
            moved.push((source, target));
        }
        let previous = self.book.clone();
        self.book.remove(id);
        if let Err(error) = self.book.save(&self.data_dir) {
            self.book = previous;
            for (from, to) in moved.iter().rev() {
                let _ = fs::rename(to, from);
            }
            report(self.hwnd, "메모 목록을 저장하지 못했습니다.", error);
            return;
        }
        self.edit = self.empty_edit;
        self.open.retain(|tab| {
            if ids.contains(&tab.id) {
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
        self.active = None;
        self.loading = true;
        unsafe {
            SetWindowTextW(self.edit, wide("").as_ptr());
            EnableWindow(self.edit, 0);
            self.rebuild_tree(None);
        }
        self.loading = false;
        if !self.open.is_empty() {
            unsafe {
                self.activate(0);
            }
        }
        unsafe {
            self.layout();
        }
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
        let source = self.dragging?;
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
                DropTarget::resolve(&self.book, source, Some(target), zone)
            } else {
                DropTarget::resolve(&self.book, source, None, DropZone::Root)
            }
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
        self.drop_target = None;
        self.tree_hover = None;
        self.resizing = false;
        self.publish_view();
        unsafe {
            KillTimer(self.hwnd, DRAG_SCROLL_TIMER);
            if GetCapture() == self.hwnd {
                ReleaseCapture();
            }
            SetCursor(LoadCursorW(null_mut(), IDC_ARROW));
            self.repaint_drag_feedback();
        }
    }

    unsafe fn finish_drag(&mut self) {
        let target = unsafe { self.drag_target_at_pointer() };
        let source = self.dragging;
        unsafe {
            self.cancel_interaction();
        }
        if let (Some(id), Some(target)) = (source, target) {
            let previous = self.book.clone();
            if self.book.move_note(id, target.parent, target.before) {
                self.expanded.insert(target.parent);
                unsafe {
                    self.persist_tree(previous, Some(id));
                }
            }
        }
    }

    unsafe fn context_menu(&mut self) {
        unsafe {
            if let Some((_, _, handle)) = self.hit_tree() {
                SendMessageW(
                    self.tree,
                    TVM_SELECTITEM,
                    TVGN_CARET as usize,
                    handle as LPARAM,
                );
            }
            let menu = CreatePopupMenu();
            let entries = [
                (MENU_NEW_CHILD, "하위 메모 추가"),
                (MENU_NEW_ROOT, "최상위 메모 추가"),
                (MENU_RENAME, "이름 바꾸기    F2"),
                (MENU_UP, "위로 이동"),
                (MENU_DOWN, "아래로 이동"),
                (MENU_OUTDENT, "한 단계 위로"),
                (MENU_DELETE, "삭제"),
            ];
            for (id, label) in entries {
                AppendMenuW(menu, MF_STRING, id, wide(label).as_ptr());
            }
            let mut pt = POINT::default();
            GetCursorPos(&mut pt);
            TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, self.hwnd, null());
            DestroyMenu(menu);
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
            ID_SETTINGS_THEME | ID_SETTINGS_FONT if code == CBN_SELCHANGE as usize => unsafe {
                self.change_setting(id);
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
                TVN_BEGINLABELEDITW => unsafe {
                    PostMessageW(self.hwnd, POSITION_RENAME, 0, 0);
                },
                NM_CLICK => {
                    let mut pt = POINT::default();
                    unsafe {
                        GetCursorPos(&mut pt);
                        ScreenToClient(self.tree, &mut pt);
                    }
                    let mut hit = TVHITTESTINFO {
                        pt,
                        flags: 0,
                        hItem: 0,
                    };
                    unsafe {
                        SendMessageW(self.tree, TVM_HITTEST, 0, &mut hit as *mut _ as LPARAM);
                    }
                    if hit.flags & (TVHT_ONITEMLABEL | TVHT_ONITEMICON | TVHT_ONITEMRIGHT) != 0
                        && let Some(&id) = self.handles.get(&hit.hItem)
                    {
                        unsafe {
                            self.open_note(id);
                        }
                    }
                }
                TVN_SELCHANGEDW => {
                    let event = unsafe { &*(hdr as *const _ as *const NMTREEVIEWW) };
                    if event.itemNew.lParam > 0 {
                        unsafe {
                            self.open_note(event.itemNew.lParam as u64);
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
                            let previous = self.book.clone();
                            if let Some(note) = self.book.get_mut(id) {
                                note.title = title.to_owned();
                            }
                            if let Err(error) = self.book.save(&self.data_dir) {
                                self.book = previous;
                                report(self.hwnd, "메모 이름을 저장하지 못했습니다.", error);
                                return 0;
                            }
                            self.sync_tree_view();
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
                        self.dragging = Some(event.itemNew.lParam as u64);
                        unsafe {
                            self.tooltip.hide();
                        }
                        self.tree_hover = None;
                        self.drop_target = None;
                        self.publish_view();
                        unsafe {
                            SetCapture(self.hwnd);
                            SetTimer(self.hwnd, DRAG_SCROLL_TIMER, 160, None);
                            self.update_drag_feedback();
                            self.repaint_drag_feedback();
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
                    self.context_menu();
                },
                _ => {}
            }
        }
        0
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
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
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX => {
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
        if ctrl && key == VK_OEM_COMMA as u32 {
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
        } else if key == VK_ESCAPE as u32 && (app.dragging.is_some() || app.resizing) {
            app.cancel_interaction();
            handled = true;
        } else if ctrl && key == b'A' as u32 {
            if app.active.is_some()
                && (msg.hwnd == app.edit || msg.hwnd == app.tree || msg.hwnd == app.tabs)
            {
                SetFocus(app.edit);
                editing::select(app.edit, editing::CharRange { start: 0, end: -1 });
                handled = true;
            } else if msg.hwnd == app.find_input
                || msg.hwnd == app.replace_input
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
            app.rename_from_focus(msg.hwnd);
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

#[cfg(test)]
mod settings_tests {
    use super::*;
    use std::time::Instant;

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
            let root =
                std::env::temp_dir().join(format!("edot-settings-native-{}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            for repeat in [100, 6000] {
                let dir = root.join(repeat.to_string());
                let mut book = Notebook::load(&dir).unwrap();
                let id = book.add(0, "설정 테스트".into());
                let second = book.add(0, "숨겨진 편집기".into());
                let lazy = book.add(0, "아직 읽지 않은 탭".into());
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
                let before = window_text(editor);
                let hidden_before = window_text(hidden_editor);
                let disk_before = fs::read(app.book.file(&dir, id)).unwrap();
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
                assert_eq!(editing::selection(editor), selection);
                assert_eq!(SendMessageW(editor, EM_GETMODIFY, 0, 0), modified);
                assert_eq!(window_text(editor), edited);
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
