use crate::drag::{DropTarget, DropZone};
use crate::model::Notebook;
use crate::scrollbar::{self, Kind, ScrollRange};
use crate::settings_controls;
use crate::tabs::TabLayout;
use crate::theme::{self, Theme, rect};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ptr::null_mut;
use std::rc::Rc;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone)]
pub struct RenderTab {
    pub id: u64,
}

/// Published separately from mutable application state so synchronous Win32
/// paint notifications can always draw the same UI, even during a mutation.
#[derive(Clone)]
pub struct View {
    pub hwnd: HWND,
    pub tree: HWND,
    pub tabs: HWND,
    pub tab_scroll: HWND,
    pub edit: HWND,
    pub editor_scroll: [HWND; 2],
    pub toggle: HWND,
    pub sidebar_tabs: [HWND; 2],
    pub tab_add: HWND,
    pub settings_button: HWND,
    pub settings_panel: Option<crate::settings::Panel>,
    pub settings_visible: bool,
    pub word_wrap: bool,
    pub zoom: u32,
    pub status: HWND,
    pub theme: Rc<Theme>,
    pub book: Rc<Notebook>,
    pub handles: Rc<HashMap<isize, u64>>,
    pub open: Rc<Vec<RenderTab>>,
    pub parents: Rc<HashSet<u64>>,
    pub active: Option<usize>,
    pub sidebar_width: i32,
    pub sidebar_visible: bool,
    pub status_text: String,
    pub tab_layout: TabLayout,
    pub tab_rename: Option<u64>,
    pub tree_scroll: HWND,
    pub visible: Rc<Vec<isize>>,
    pub find_visible: bool,
    pub replace_visible: bool,
    pub find_global: bool,
    pub global_panel: Option<crate::global_search::Panel>,
    pub global_replace: bool,
    pub global_case: bool,
    pub global_message: String,
    pub global_results: HWND,
    pub global_scroll: HWND,
    pub global_rows: Rc<Vec<crate::global_search::ResultRow>>,
    pub global_offset: usize,
    pub global_hover: Option<usize>,
    pub global_selected: Option<u64>,
    pub resizing: bool,
    pub tree_hover: Option<isize>,
    pub dragging: Option<u64>,
    pub drag_sources: Rc<Vec<u64>>,
    pub selected: Rc<HashSet<u64>>,
    pub tab_insertion: Option<usize>,
    pub drop_target: Option<DropTarget>,
    pub search_host: HWND,
    pub find_input: HWND,
    pub replace_input: HWND,
    pub search_buttons: [HWND; 6],
    pub find_case_sensitive: bool,
    pub find_message: String,
}
thread_local! {
    static VIEW: RefCell<Option<View>> = const {RefCell::new(None)};
    static BUFFERS: RefCell<HashMap<isize,BackBuffer>> = RefCell::new(HashMap::new());
}
pub fn publish(view: View) {
    VIEW.with(|v| *v.borrow_mut() = Some(view));
}
pub fn with_view(f: impl FnOnce(&View)) {
    let snapshot = VIEW.with(|v| v.borrow().clone());
    if let Some(view) = snapshot {
        f(&view);
    }
}
pub fn clear() {
    VIEW.with(|v| v.borrow_mut().take());
    BUFFERS.with(|b| b.borrow_mut().clear());
}
pub fn release_buffer(hwnd: HWND) {
    BUFFERS.with(|buffers| buffers.borrow_mut().remove(&(hwnd as isize)));
}

struct BackBuffer {
    dc: HDC,
    bitmap: HBITMAP,
    original: HGDIOBJ,
    width: i32,
    height: i32,
}
impl BackBuffer {
    unsafe fn new(dc: HDC) -> Self {
        unsafe {
            Self {
                dc: CreateCompatibleDC(dc),
                bitmap: null_mut(),
                original: null_mut(),
                width: 0,
                height: 0,
            }
        }
    }
    unsafe fn resize(&mut self, dc: HDC, w: i32, h: i32) {
        if w <= self.width && h <= self.height {
            return;
        }
        unsafe {
            let bitmap = CreateCompatibleBitmap(
                dc,
                ((w + 127) & !127).max(self.width),
                ((h + 127) & !127).max(self.height),
            );
            if bitmap.is_null() {
                return;
            }
            let old = SelectObject(self.dc, bitmap);
            if self.original.is_null() {
                self.original = old;
            }
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
            }
            self.bitmap = bitmap;
            self.width = ((w + 127) & !127).max(self.width);
            self.height = ((h + 127) & !127).max(self.height);
        }
    }
}
impl Drop for BackBuffer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.original);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
pub unsafe fn buffered(hwnd: HWND, dc: HDC, area: RECT, draw: impl FnOnce(HDC)) {
    unsafe {
        BUFFERS.with(|buffers| {
            let mut buffers = buffers.borrow_mut();
            let buffer = buffers
                .entry(hwnd as isize)
                .or_insert_with(|| BackBuffer::new(dc));
            buffer.resize(dc, area.right.max(1), area.bottom.max(1));
            let saved = SaveDC(buffer.dc);
            draw(buffer.dc);
            RestoreDC(buffer.dc, saved);
            BitBlt(
                dc,
                area.left,
                area.top,
                area.right - area.left,
                area.bottom - area.top,
                buffer.dc,
                area.left,
                area.top,
                SRCCOPY,
            );
        });
    }
}
pub unsafe fn paint_window(hwnd: HWND, draw: impl FnOnce(HDC)) {
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        let mut area = RECT::default();
        GetClientRect(hwnd, &mut area);
        buffered(hwnd, dc, area, draw);
        EndPaint(hwnd, &paint);
    }
}
impl View {
    pub unsafe fn splitter_hit(&self, point: POINT) -> bool {
        let mut area = RECT::default();
        unsafe {
            GetClientRect(self.hwnd, &mut area);
        }
        self.sidebar_visible
            && point.y > self.theme.px(36)
            && point.y < area.bottom
            && (point.x - self.sidebar_edge(area.right)).abs() <= self.theme.px(4)
    }

    fn drag_description(&self) -> String {
        let source = self
            .dragging
            .and_then(|id| self.book.get(id))
            .map(|n| n.title.as_str())
            .unwrap_or("");
        let action = match self.drop_target {
            Some(target) => {
                let title = target
                    .note
                    .and_then(|id| self.book.get(id))
                    .map(|n| n.title.as_str())
                    .unwrap_or("");
                match target.zone {
                    DropZone::Before => format!("{title} 위에 놓기"),
                    DropZone::After => format!("{title} 아래에 놓기"),
                    DropZone::Inside => format!("{title}의 하위로 이동"),
                    DropZone::Root => "최상위로 이동".to_owned(),
                }
            }
            None => "이 위치에는 놓을 수 없습니다".to_owned(),
        };
        if self.drag_sources.len() > 1 {
            format!(
                "{}개 항목 → {action}   ·   Esc: 취소",
                self.drag_sources.len()
            )
        } else {
            format!("{source} → {action}   ·   Esc: 취소")
        }
    }

    pub unsafe fn paint_drag_feedback(&self, dc: HDC) {
        if !matches!(
            self.drop_target,
            Some(DropTarget {
                zone: DropZone::Root,
                ..
            })
        ) {
            return;
        }
        unsafe {
            let p = |n| self.theme.px(n);
            let mut area = RECT::default();
            GetClientRect(GetParent(self.tree), &mut area);
            let last = SendMessageW(self.tree, TVM_GETNEXTITEM, TVGN_LASTVISIBLE as usize, 0);
            let mut row = RECT::default();
            *(&mut row as *mut RECT as *mut HTREEITEM) = last;
            if last != 0 {
                SendMessageW(self.tree, TVM_GETITEMRECT, 0, &mut row as *mut _ as isize);
            }
            let top = (row.bottom + p(4)).min((area.bottom - p(32)).max(0));
            let banner = rect(p(2), top, area.right - p(2), top + p(29));
            theme::rounded(
                dc,
                banner,
                self.theme.colors.selected,
                self.theme.colors.blue,
                p(6),
            );
            theme::text(
                dc,
                "＋ 최상위로 이동",
                banner,
                self.theme.small,
                self.theme.colors.blue,
                DT_CENTER,
            );
        }
    }
    pub unsafe fn scroll_range(&self, kind: Kind) -> ScrollRange {
        unsafe {
            match kind {
                Kind::Search => {
                    let mut area = RECT::default();
                    GetClientRect(self.global_results, &mut area);
                    ScrollRange {
                        min: 0,
                        max: self.global_rows.len() as i32 - 1,
                        page: (area.bottom / self.theme.px(60)).max(1) as u32,
                        position: self.global_offset as i32,
                    }
                }
                Kind::EditorVertical | Kind::EditorHorizontal => {
                    scrollbar::body_range(self.edit, kind.horizontal())
                }
                Kind::Tree => {
                    let count = SendMessageW(self.tree, TVM_GETVISIBLECOUNT, 0, 0).max(1) as u32;
                    let first =
                        SendMessageW(self.tree, TVM_GETNEXTITEM, TVGN_FIRSTVISIBLE as usize, 0);
                    ScrollRange {
                        min: 0,
                        max: self.visible.len() as i32 - 1,
                        page: count,
                        position: self.visible.iter().position(|&h| h == first).unwrap_or(0) as i32,
                    }
                }
                Kind::Dropdown { list, .. } => scrollbar::list_range(list),
                Kind::Tabs => ScrollRange {
                    min: 0,
                    max: self.tab_layout.count as i32 * self.tab_layout.width - 1,
                    page: self.tab_layout.viewport as u32,
                    position: self.tab_layout.offset,
                },
            }
        }
    }
    pub unsafe fn refresh_editor_scroll(&self) {
        unsafe {
            for (index, &control) in self.editor_scroll.iter().enumerate() {
                let show = self.active.is_some()
                    && (index == 0 || !self.word_wrap)
                    && scrollbar::body_range(self.edit, index == 1).extent() > 0;
                if (IsWindowVisible(control) != 0) != show {
                    ShowWindow(control, if show { SW_SHOWNA } else { SW_HIDE });
                }
                if show {
                    InvalidateRect(control, std::ptr::null(), 0);
                }
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
    pub unsafe fn paint_background(&self, dc: HDC) {
        unsafe {
            let p = |v| self.theme.px(v);
            let mut area = RECT::default();
            GetClientRect(self.hwnd, &mut area);
            let edge = self.sidebar_edge(area.right);
            theme::fill(dc, area, self.theme.colors.white);
            theme::fill(
                dc,
                rect(
                    0,
                    0,
                    area.right,
                    p(if self.tab_layout.overflow { 48 } else { 36 }),
                ),
                self.theme.colors.bar,
            );
            theme::line(
                dc,
                &[
                    (edge, p(if self.tab_layout.overflow { 48 } else { 36 }) - 1),
                    (
                        area.right,
                        p(if self.tab_layout.overflow { 48 } else { 36 }) - 1,
                    ),
                ],
                self.theme.colors.border,
                1,
            );
            if self.sidebar_visible {
                theme::fill(dc, rect(0, 0, edge, area.bottom), self.theme.colors.sidebar);
                theme::line(
                    dc,
                    &[(edge, 0), (edge, area.bottom)],
                    self.theme.colors.border,
                    1,
                );
            }
            if self.active.is_none() {
                let cx = edge + (area.right - edge) / 2;
                let cy = area.bottom / 2 - p(40);
                theme::document(dc, cx - p(20), cy - p(54), p(40), self.theme.colors.blue);
                theme::text(
                    dc,
                    "생각을 가볍게 기록하세요",
                    rect(edge, cy, area.right, cy + p(38)),
                    self.theme.heading,
                    self.theme.colors.ink,
                    DT_CENTER,
                );
                theme::text(
                    dc,
                    "메모를 선택하거나 Ctrl + N으로 새 메모를 만드세요",
                    rect(edge, cy + p(40), area.right, cy + p(72)),
                    self.theme.ui,
                    self.theme.colors.muted,
                    DT_CENTER,
                );
            }
        }
    }

    pub unsafe fn paint_search_host(&self, hwnd: HWND, dc: HDC) {
        unsafe {
            if let Some(panel) = self.global_panel.filter(|panel| panel.host == hwnd) {
                let p = |n| self.theme.px(n);
                let mut area = RECT::default();
                GetClientRect(hwnd, &mut area);
                theme::fill(dc, area, self.theme.colors.sidebar);
                for (field, y) in [(panel.find, 36), (panel.replace, 82)] {
                    if field == panel.replace && !self.global_replace {
                        continue;
                    }
                    theme::rounded(
                        dc,
                        rect(p(12), p(y), area.right - p(12), p(y + 36)),
                        self.theme.colors.white,
                        if GetFocus() == field {
                            self.theme.colors.blue
                        } else {
                            self.theme.colors.border
                        },
                        p(7),
                    );
                }
                let y = p(crate::global_search::message_y(
                    self.global_replace,
                    area.right < p(230),
                ));
                theme::text(
                    dc,
                    &self.global_message,
                    rect(p(12), y, area.right - p(12), y + p(38)),
                    self.theme.small,
                    self.theme.colors.muted,
                    DT_LEFT | DT_WORDBREAK,
                );
                let bottom = y + p(45);
                theme::fill(
                    dc,
                    rect(0, bottom, area.right, bottom + 1),
                    self.theme.colors.border,
                );
            } else {
                self.paint_search(dc);
            }
        }
    }

    pub unsafe fn paint_search(&self, dc: HDC) {
        unsafe {
            let p = |n| self.theme.px(n);
            let mut area = RECT::default();
            GetClientRect(self.search_host, &mut area);
            theme::fill(dc, area, self.theme.colors.white);
            if !self.find_visible {
                return;
            }
            theme::rounded(
                dc,
                rect(p(3), p(4), area.right, area.bottom),
                self.theme.colors.hover,
                self.theme.colors.hover,
                p(12),
            );
            theme::rounded(
                dc,
                rect(0, 0, area.right - p(3), area.bottom - p(4)),
                self.theme.colors.white,
                self.theme.colors.border,
                p(12),
            );
            if area.right > p(250) {
                theme::text(
                    dc,
                    if self.replace_visible {
                        "찾기 및 바꾸기"
                    } else {
                        "찾기"
                    },
                    rect(p(16), p(6), area.right - p(154), p(38)),
                    self.theme.ui,
                    self.theme.colors.ink,
                    DT_LEFT,
                );
            }
            for (field, top) in [(self.find_input, 42), (self.replace_input, 88)] {
                if field == self.replace_input && !self.replace_visible {
                    continue;
                }
                theme::rounded(
                    dc,
                    rect(p(16), p(top), area.right - p(20), p(top + 34)),
                    self.theme.colors.white,
                    if GetFocus() == field {
                        self.theme.colors.blue
                    } else {
                        self.theme.colors.border
                    },
                    p(7),
                );
            }
            let message_y = if self.replace_visible { 174 } else { 84 };
            theme::text(
                dc,
                &self.find_message,
                rect(p(16), p(message_y), area.right - p(20), p(message_y + 24)),
                self.theme.small,
                self.theme.colors.muted,
                DT_LEFT,
            );
        }
    }

    pub unsafe fn paint_global_results(&self, dc: HDC) {
        unsafe {
            let p = |n| self.theme.px(n);
            let mut area = RECT::default();
            GetClientRect(self.global_results, &mut area);
            theme::fill(dc, area, self.theme.colors.sidebar);
            let height = p(60);
            for (index, row) in self.global_rows.iter().enumerate().skip(self.global_offset) {
                let top = (index - self.global_offset) as i32 * height;
                if top >= area.bottom {
                    break;
                }
                let selected = self.global_selected == Some(row.id);
                if selected || self.global_hover == Some(index) {
                    theme::rounded(
                        dc,
                        rect(p(4), top + p(2), area.right - p(4), top + height - p(2)),
                        if selected {
                            self.theme.colors.selected
                        } else {
                            self.theme.colors.hover
                        },
                        if selected && GetFocus() == self.global_results {
                            self.theme.colors.blue
                        } else if selected {
                            self.theme.colors.selected
                        } else {
                            self.theme.colors.hover
                        },
                        p(7),
                    );
                }
                let title = self
                    .book
                    .get(row.id)
                    .map_or("삭제된 메모", |note| note.title.as_str());
                theme::text(
                    dc,
                    &format!("{title}  ·  {}곳", row.count),
                    rect(p(12), top + p(5), area.right - p(8), top + p(29)),
                    self.theme.ui,
                    self.theme.colors.ink,
                    DT_LEFT,
                );
                theme::text(
                    dc,
                    &format!("{}행  {}", row.line, row.preview),
                    rect(p(12), top + p(30), area.right - p(8), top + p(53)),
                    self.theme.small,
                    self.theme.colors.muted,
                    DT_LEFT,
                );
            }
            if self.global_rows.is_empty() {
                theme::text(
                    dc,
                    "메모 본문에서 검색합니다",
                    rect(p(12), p(12), area.right - p(12), p(40)),
                    self.theme.small,
                    self.theme.colors.muted,
                    DT_LEFT,
                );
            }
        }
    }

    pub unsafe fn paint_tabs(&self, dc: HDC) {
        unsafe {
            let p = |v| self.theme.px(v);
            let mut area = RECT::default();
            GetClientRect(self.tabs, &mut area);
            theme::fill(dc, area, self.theme.colors.bar);
            theme::line(
                dc,
                &[(0, area.bottom - 1), (area.right, area.bottom - 1)],
                self.theme.colors.border,
                1,
            );
            let mut mouse = POINT::default();
            GetCursorPos(&mut mouse);
            ScreenToClient(self.tabs, &mut mouse);
            let saved = SaveDC(dc);
            IntersectClipRect(dc, 0, 0, self.tab_layout.viewport, area.bottom);
            for (index, tab) in self.open.iter().enumerate() {
                let (left, right) = self.tab_layout.bounds(index);
                if right <= 0 || left >= area.right {
                    continue;
                }
                let item = rect(left, 1, right, area.bottom);
                let active = self.active == Some(index);
                if active {
                    theme::rounded(
                        dc,
                        rect(left, 1, right, area.bottom + p(4)),
                        self.theme.colors.white,
                        self.theme.colors.border,
                        p(6),
                    );
                    theme::line(
                        dc,
                        &[(left + p(4), 2), (right - p(4), 2)],
                        self.theme.colors.blue,
                        p(2),
                    );
                } else {
                    if PtInRect(&item, mouse) != 0 {
                        theme::fill(dc, item, self.theme.colors.hover);
                    }
                    theme::line(
                        dc,
                        &[(right - 1, p(7)), (right - 1, area.bottom - p(7))],
                        self.theme.colors.border,
                        1,
                    );
                }
                theme::document(
                    dc,
                    left + p(9),
                    (p(36) - p(15)) / 2,
                    p(15),
                    if active {
                        self.theme.colors.blue
                    } else {
                        self.theme.colors.muted
                    },
                );
                let title = self
                    .book
                    .get(tab.id)
                    .map(|n| n.title.as_str())
                    .unwrap_or("");
                if self.tab_rename == Some(tab.id) {
                    theme::rounded(
                        dc,
                        rect(left + p(30), p(6), right - p(25), p(30)),
                        self.theme.colors.white,
                        self.theme.colors.blue,
                        p(4),
                    );
                } else {
                    theme::text(
                        dc,
                        title,
                        rect(left + p(30), 0, right - p(25), p(36)),
                        self.theme.tab,
                        self.theme.colors.ink,
                        DT_LEFT,
                    );
                }
                let close_area = rect(right - p(24), p(7), right - p(3), p(29));
                if PtInRect(&close_area, mouse) != 0 {
                    theme::rounded(
                        dc,
                        close_area,
                        self.theme.colors.hover,
                        self.theme.colors.hover,
                        p(4),
                    );
                }
                theme::close(dc, right - p(17), p(14), p(8), self.theme.colors.muted);
            }
            if let Some(gap) = self.tab_insertion {
                let x = (gap as i32 * self.tab_layout.width - self.tab_layout.offset)
                    .clamp(p(1), (self.tab_layout.viewport - p(2)).max(p(1)));
                theme::line(
                    dc,
                    &[(x, p(3)), (x, area.bottom - p(3))],
                    self.theme.colors.blue,
                    p(2),
                );
            }
            RestoreDC(dc, saved);
        }
    }

    pub unsafe fn paint_tree_item(&self, custom: &NMTVCUSTOMDRAW) {
        unsafe {
            let p = |v| self.theme.px(v);
            let dc = custom.nmcd.hdc;
            let handle = custom.nmcd.dwItemSpec as HTREEITEM;
            let Some(&id) = self.handles.get(&handle) else {
                return;
            };
            let Some(note) = self.book.get(id) else {
                return;
            };
            let mut area = custom.nmcd.rc;
            let mut client = RECT::default();
            GetClientRect(GetParent(self.tree), &mut client);
            area.left = 0;
            area.right = client.right;
            theme::fill(dc, area, self.theme.colors.sidebar);
            let selected = self.selected.contains(&id);
            let inside = self
                .drop_target
                .is_some_and(|target| target.note == Some(id) && target.zone == DropZone::Inside);
            let hovered = self.dragging.is_none() && self.tree_hover == Some(handle);
            if selected || inside || hovered {
                theme::rounded(
                    dc,
                    rect(2, area.top + 1, area.right - 2, area.bottom - 1),
                    if selected || inside {
                        self.theme.colors.selected
                    } else {
                        self.theme.colors.hover
                    },
                    if inside {
                        self.theme.colors.blue
                    } else if selected {
                        self.theme.colors.selected
                    } else {
                        self.theme.colors.hover
                    },
                    p(6),
                );
            }
            let mut label = RECT::default();
            *(&mut label as *mut RECT as *mut HTREEITEM) = handle;
            SendMessageW(
                self.tree,
                TVM_GETITEMRECT,
                1,
                &mut label as *mut _ as LPARAM,
            );
            let x = label.left;
            let y = area.top + (area.bottom - area.top - p(18)) / 2;
            let children = self.parents.contains(&id);
            if custom.iLevel > 0 {
                for level in 0..custom.iLevel {
                    let gx = p(11 + level * 22);
                    theme::line(
                        dc,
                        &[(gx, area.top), (gx, area.bottom)],
                        self.theme.colors.border,
                        1,
                    );
                }
            }
            if children {
                let cx = x - p(11);
                let cy = (area.top + area.bottom) / 2;
                let mut state = TVITEMW {
                    mask: TVIF_STATE,
                    hItem: handle,
                    stateMask: TVIS_EXPANDED,
                    ..Default::default()
                };
                SendMessageW(self.tree, TVM_GETITEMW, 0, &mut state as *mut _ as isize);
                if state.state & TVIS_EXPANDED != 0 {
                    theme::line(
                        dc,
                        &[
                            (cx - p(4), cy - p(2)),
                            (cx, cy + p(2)),
                            (cx + p(4), cy - p(2)),
                        ],
                        self.theme.colors.ink,
                        p(1),
                    );
                } else {
                    theme::line(
                        dc,
                        &[
                            (cx - p(2), cy - p(4)),
                            (cx + p(2), cy),
                            (cx - p(2), cy + p(4)),
                        ],
                        self.theme.colors.ink,
                        p(1),
                    );
                }
                theme::folder(dc, x, y, p(18));
            } else {
                theme::document(
                    dc,
                    x,
                    y,
                    p(18),
                    if selected {
                        self.theme.colors.blue
                    } else {
                        self.theme.colors.muted
                    },
                );
            }
            let editing = SendMessageW(self.tree, TVM_GETEDITCONTROL, 0, 0) != 0
                && SendMessageW(self.tree, TVM_GETNEXTITEM, TVGN_CARET as usize, 0) == handle;
            if !editing {
                theme::text(
                    dc,
                    &note.title,
                    rect(x + p(27), area.top, area.right - p(20), area.bottom),
                    self.theme.ui,
                    self.theme.colors.ink,
                    DT_LEFT,
                );
            }
            for row in 0..3 {
                for col in 0..2 {
                    let gx = area.right - p(12) + col * p(4);
                    let gy = (area.top + area.bottom) / 2 - p(5) + row * p(5);
                    theme::fill(
                        dc,
                        rect(gx, gy, gx + p(2), gy + p(2)),
                        if self.dragging.is_some() && self.selected.contains(&id) {
                            self.theme.colors.blue
                        } else {
                            self.theme.colors.muted
                        },
                    );
                }
            }
            if let Some(target) = self.drop_target.filter(|target| target.note == Some(id))
                && matches!(target.zone, DropZone::Before | DropZone::After)
            {
                let y = if target.zone == DropZone::Before {
                    area.top + p(1)
                } else {
                    area.bottom - p(2)
                };
                theme::line(
                    dc,
                    &[(x, y), (area.right - p(4), y)],
                    self.theme.colors.blue,
                    p(2),
                );
                theme::rounded(
                    dc,
                    rect(x - p(3), y - p(3), x + p(3), y + p(3)),
                    self.theme.colors.blue,
                    self.theme.colors.blue,
                    p(6),
                );
            }
        }
    }

    pub unsafe fn paint_settings(&self, dc: HDC) {
        let Some(panel) = self.settings_panel else {
            return;
        };
        unsafe {
            let p = |v| self.theme.px(v);
            let mut area = RECT::default();
            GetClientRect(panel.host, &mut area);
            theme::fill(dc, area, self.theme.colors.white);
            theme::rounded(
                dc,
                rect(0, 0, area.right, area.bottom),
                self.theme.colors.white,
                self.theme.colors.border,
                p(8),
            );
            theme::text(
                dc,
                "설정",
                rect(p(20), p(10), area.right - p(80), p(40)),
                self.theme.heading,
                self.theme.colors.ink,
                DT_LEFT,
            );
            theme::text(
                dc,
                "변경 사항은 바로 적용되고 저장됩니다.",
                rect(p(20), p(274), area.right - p(20), p(300)),
                self.theme.small,
                self.theme.colors.muted,
                DT_LEFT,
            );
        }
    }

    pub unsafe fn paint_control(&self, draw: &DRAWITEMSTRUCT) {
        unsafe {
            let p = |v| self.theme.px(v);
            let dc = draw.hDC;
            let area = draw.rcItem;
            if let Some(panel) = self.settings_panel {
                if [panel.theme, panel.font, panel.size].contains(&draw.hwndItem) {
                    settings_controls::paint_item(draw, &self.theme);
                    return;
                }
                if draw.hwndItem == panel.wrap {
                    settings_controls::paint_switch(
                        dc,
                        area,
                        &self.theme,
                        self.word_wrap,
                        settings_controls::Feedback::read(draw.hwndItem, draw.itemState),
                    );
                    return;
                }
            }
            if draw.hwndItem == self.status {
                theme::fill(dc, area, self.theme.colors.bar);
                theme::line(dc, &[(0, 0), (area.right, 0)], self.theme.colors.border, 1);
                if self.dragging.is_some() {
                    theme::text(
                        dc,
                        &self.drag_description(),
                        rect(p(12), 0, area.right - p(8), area.bottom),
                        self.theme.small,
                        if self.drop_target.is_some() {
                            self.theme.colors.blue
                        } else {
                            self.theme.colors.muted
                        },
                        DT_LEFT,
                    );
                    return;
                }
                if let Some(index) = self.active {
                    let tab = &self.open[index];
                    let title = self
                        .book
                        .get(tab.id)
                        .map(|n| n.title.as_str())
                        .unwrap_or("");
                    theme::document(dc, p(18), p(10), p(16), self.theme.colors.muted);
                    theme::text(
                        dc,
                        title,
                        rect(p(44), 0, p(182).min(area.right / 3), area.bottom),
                        self.theme.small,
                        self.theme.colors.ink,
                        DT_LEFT,
                    );
                    if area.right > p(660) {
                        theme::text(
                            dc,
                            &self.status_text,
                            rect(p(190), 0, area.right - p(490), area.bottom),
                            self.theme.small,
                            self.theme.colors.muted,
                            DT_LEFT,
                        );
                    }
                    let mut start = 0u32;
                    SendMessageW(self.edit, EM_GETSEL, &mut start as *mut _ as usize, 0);
                    let line = SendMessageW(self.edit, EM_LINEFROMCHAR, start as usize, 0);
                    let line_start = SendMessageW(self.edit, EM_LINEINDEX, line as usize, 0);
                    let info = format!(
                        "Ln {}, Col {}   │   {}   │   {}%   │   UTF-8   │   Plain Text",
                        line + 1,
                        start as isize - line_start + 1,
                        crate::editing::character_count(self.edit),
                        self.zoom
                    );
                    theme::text(
                        dc,
                        &info,
                        rect(
                            p(190).min(area.right / 3),
                            0,
                            area.right - p(16),
                            area.bottom,
                        ),
                        self.theme.small,
                        self.theme.colors.muted,
                        DT_RIGHT,
                    );
                } else {
                    theme::text(
                        dc,
                        "Edot   ·   Plain Text",
                        rect(p(18), 0, area.right, area.bottom),
                        self.theme.small,
                        self.theme.colors.muted,
                        DT_LEFT,
                    );
                }
                return;
            }
            let mut mouse = POINT::default();
            GetCursorPos(&mut mouse);
            ScreenToClient(draw.hwndItem, &mut mouse);
            let hovered = PtInRect(&area, mouse) != 0;
            let pressed = draw.itemState & ODS_SELECTED != 0;
            let focused = draw.itemState & ODS_FOCUS != 0;
            let global_button = self.global_panel.and_then(|panel| {
                panel
                    .buttons
                    .iter()
                    .position(|&button| button == draw.hwndItem)
            });
            if let Some(index) = global_button.or_else(|| {
                self.search_buttons
                    .iter()
                    .position(|&button| button == draw.hwndItem)
            }) {
                let base = if global_button.is_some() {
                    self.theme.colors.sidebar
                } else {
                    self.theme.colors.white
                };
                let checked = (index == 0
                    && if global_button.is_some() {
                        self.global_case
                    } else {
                        self.find_case_sensitive
                    })
                    || (index == 6 && self.global_replace);
                let primary = index == 5;
                let color = if primary {
                    if hovered || pressed {
                        theme::rgb(25, 99, 220)
                    } else {
                        self.theme.colors.blue
                    }
                } else if checked || pressed {
                    self.theme.colors.selected
                } else if hovered {
                    self.theme.colors.hover
                } else {
                    base
                };
                theme::fill(dc, area, base);
                theme::rounded(
                    dc,
                    rect(1, 1, area.right - 1, area.bottom - 1),
                    color,
                    if focused {
                        self.theme.colors.blue
                    } else if index == 4 {
                        self.theme.colors.border
                    } else {
                        color
                    },
                    p(6),
                );
                let cx = area.right / 2;
                let cy = area.bottom / 2;
                match index {
                    1 | 2 => {
                        let direction = if index == 1 { -1 } else { 1 };
                        theme::line(
                            dc,
                            &[
                                (cx - p(4), cy - direction * p(2)),
                                (cx, cy + direction * p(2)),
                                (cx + p(4), cy - direction * p(2)),
                            ],
                            self.theme.colors.ink,
                            p(1),
                        );
                    }
                    6 => {
                        let points = if self.global_replace {
                            [
                                (cx - p(3), cy - p(2)),
                                (cx, cy + p(2)),
                                (cx + p(3), cy - p(2)),
                            ]
                        } else {
                            [
                                (cx - p(2), cy - p(3)),
                                (cx + p(2), cy),
                                (cx - p(2), cy + p(3)),
                            ]
                        };
                        theme::line(dc, &points, self.theme.colors.ink, p(1));
                    }
                    3 => theme::close(dc, cx - p(4), cy - p(4), p(8), self.theme.colors.muted),
                    _ => theme::text(
                        dc,
                        match index {
                            0 => "Aa",
                            4 => "바꾸기",
                            _ => "모두 바꾸기",
                        },
                        area,
                        self.theme.small,
                        if primary {
                            theme::WHITE
                        } else if checked {
                            self.theme.colors.blue
                        } else {
                            self.theme.colors.ink
                        },
                        DT_CENTER,
                    ),
                }
                return;
            }
            let base = if self
                .settings_panel
                .is_some_and(|panel| draw.hwndItem == panel.close)
            {
                self.theme.colors.white
            } else if (draw.hwndItem != self.toggle && !self.sidebar_tabs.contains(&draw.hwndItem))
                || !self.sidebar_visible
            {
                self.theme.colors.bar
            } else {
                self.theme.colors.sidebar
            };
            theme::fill(dc, area, base);
            let color =
                if pressed || (draw.hwndItem == self.settings_button && self.settings_visible) {
                    self.theme.colors.selected
                } else if hovered {
                    self.theme.colors.hover
                } else {
                    base
                };
            let border = if focused {
                self.theme.colors.blue
            } else {
                color
            };
            theme::rounded(
                dc,
                rect(1, 1, area.right - 1, area.bottom - 1),
                color,
                border,
                p(6),
            );
            if let Some(index) = self
                .sidebar_tabs
                .iter()
                .position(|&tab| tab == draw.hwndItem)
            {
                let selected = (index == 1) == self.find_global;
                if selected {
                    theme::fill(
                        dc,
                        rect(p(3), area.bottom - p(3), area.right - p(3), area.bottom),
                        self.theme.colors.blue,
                    );
                }
                let label = if area.right < p(70) {
                    if index == 0 { "메모" } else { "검색" }
                } else if index == 0 {
                    "메모 목록"
                } else {
                    "전체 검색"
                };
                theme::text(
                    dc,
                    label,
                    area,
                    self.theme.small,
                    if selected {
                        self.theme.colors.blue
                    } else {
                        self.theme.colors.muted
                    },
                    DT_CENTER,
                );
            } else if draw.hwndItem == self.settings_button {
                theme::gear(
                    dc,
                    (area.right - p(18)) / 2,
                    (area.bottom - p(18)) / 2,
                    p(18),
                    self.theme.colors.ink,
                );
            } else if draw.hwndItem == self.tab_add {
                theme::plus(
                    dc,
                    (area.right - p(14)) / 2,
                    (area.bottom - p(14)) / 2,
                    p(14),
                    self.theme.colors.ink,
                );
            } else if draw.hwndItem != self.toggle {
                let mut label = [0u16; 64];
                let n = GetWindowTextW(draw.hwndItem, label.as_mut_ptr(), 64).max(0) as usize;
                theme::text(
                    dc,
                    &String::from_utf16_lossy(&label[..n]),
                    area,
                    self.theme.small,
                    self.theme.colors.ink,
                    DT_CENTER,
                );
            } else {
                // Use the same pixel distance on both axes to keep a 45-degree
                // diagonal even when DPI scaling rounds the coordinates.
                let step = p(4);
                let spacing = p(6);
                let left = (area.left + area.right - step - spacing) / 2;
                let cy = (area.top + area.bottom) / 2;
                for offset in [0, spacing] {
                    let x = left + offset;
                    let (edge, tip) = if self.sidebar_visible {
                        (x + step, x)
                    } else {
                        (x, x + step)
                    };
                    theme::line(
                        dc,
                        &[(edge, cy - step), (tip, cy), (edge, cy + step)],
                        self.theme.colors.ink,
                        p(1),
                    );
                }
            }
        }
    }
}
