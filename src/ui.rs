use crate::drag::{DropTarget, DropZone};
use crate::model::Notebook;
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
    pub edit: HWND,
    pub toggle: HWND,
    pub add: HWND,
    pub tab_add: HWND,
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
    pub tree_scroll: HWND,
    pub visible: Rc<Vec<isize>>,
    pub find_visible: bool,
    pub replace_visible: bool,
    pub resizing: bool,
    pub tree_hover: Option<isize>,
    pub dragging: Option<u64>,
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
        format!("{source} → {action}   ·   Esc: 취소")
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
            theme::rounded(dc, banner, theme::SELECTED, theme::BLUE, p(6));
            theme::text(
                dc,
                "＋ 최상위로 이동",
                banner,
                self.theme.small,
                theme::BLUE,
                DT_CENTER,
            );
        }
    }
    pub unsafe fn paint_tree_scroll(&self, dc: HDC) {
        unsafe {
            let mut area = RECT::default();
            GetClientRect(self.tree_scroll, &mut area);
            theme::fill(dc, area, theme::SIDEBAR);
            let count = SendMessageW(self.tree, TVM_GETVISIBLECOUNT, 0, 0).max(1) as i32;
            let total = self.visible.len() as i32;
            if total <= count {
                return;
            }
            let first = SendMessageW(self.tree, TVM_GETNEXTITEM, TVGN_FIRSTVISIBLE as usize, 0);
            let index = self.visible.iter().position(|&h| h == first).unwrap_or(0) as i32;
            let thumb = (area.bottom * count / total)
                .max(self.theme.px(24))
                .min(area.bottom);
            let top = index * (area.bottom - thumb) / (total - count).max(1);
            theme::rounded(
                dc,
                rect(0, top, area.right, top + thumb),
                theme::rgb(168, 181, 198),
                theme::rgb(168, 181, 198),
                area.right,
            );
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
            theme::fill(dc, area, theme::WHITE);
            theme::fill(dc, rect(0, 0, area.right, p(36)), theme::BAR);
            theme::line(
                dc,
                &[(edge, p(36) - 1), (area.right, p(36) - 1)],
                theme::BORDER,
                1,
            );
            if self.sidebar_visible {
                theme::fill(dc, rect(0, 0, edge, area.bottom), theme::SIDEBAR);
                theme::line(dc, &[(edge, 0), (edge, area.bottom)], theme::BORDER, 1);
            }
            if self.active.is_none() {
                let cx = edge + (area.right - edge) / 2;
                let cy = area.bottom / 2 - p(40);
                theme::document(dc, cx - p(20), cy - p(54), p(40), theme::BLUE);
                theme::text(
                    dc,
                    "생각을 가볍게 기록하세요",
                    rect(edge, cy, area.right, cy + p(38)),
                    self.theme.heading,
                    theme::INK,
                    DT_CENTER,
                );
                theme::text(
                    dc,
                    "메모를 선택하거나 Ctrl + N으로 새 메모를 만드세요",
                    rect(edge, cy + p(40), area.right, cy + p(72)),
                    self.theme.ui,
                    theme::MUTED,
                    DT_CENTER,
                );
            }
        }
    }

    pub unsafe fn paint_search(&self, dc: HDC) {
        unsafe {
            let p = |n| self.theme.px(n);
            let mut area = RECT::default();
            GetClientRect(self.search_host, &mut area);
            theme::fill(dc, area, theme::WHITE);
            if !self.find_visible {
                return;
            }
            theme::rounded(
                dc,
                rect(p(3), p(4), area.right, area.bottom),
                theme::rgb(232, 236, 242),
                theme::rgb(232, 236, 242),
                p(12),
            );
            theme::rounded(
                dc,
                rect(0, 0, area.right - p(3), area.bottom - p(4)),
                theme::WHITE,
                theme::rgb(207, 215, 227),
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
                    theme::INK,
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
                    theme::WHITE,
                    if GetFocus() == field {
                        theme::BLUE
                    } else {
                        theme::BORDER
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
                theme::MUTED,
                DT_LEFT,
            );
        }
    }

    pub unsafe fn paint_tabs(&self, dc: HDC) {
        unsafe {
            let p = |v| self.theme.px(v);
            let mut area = RECT::default();
            GetClientRect(self.tabs, &mut area);
            theme::fill(dc, area, theme::BAR);
            theme::line(
                dc,
                &[(0, area.bottom - 1), (area.right, area.bottom - 1)],
                theme::BORDER,
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
                        theme::WHITE,
                        theme::BORDER,
                        p(6),
                    );
                    theme::line(
                        dc,
                        &[(left + p(4), 2), (right - p(4), 2)],
                        theme::BLUE,
                        p(2),
                    );
                } else {
                    if PtInRect(&item, mouse) != 0 {
                        theme::fill(dc, item, theme::HOVER);
                    }
                    theme::line(
                        dc,
                        &[(right - 1, p(7)), (right - 1, area.bottom - p(7))],
                        theme::BORDER,
                        1,
                    );
                }
                theme::document(
                    dc,
                    left + p(9),
                    (p(36) - p(15)) / 2,
                    p(15),
                    if active { theme::BLUE } else { theme::MUTED },
                );
                let title = self
                    .book
                    .get(tab.id)
                    .map(|n| n.title.as_str())
                    .unwrap_or("");
                theme::text(
                    dc,
                    title,
                    rect(left + p(30), 0, right - p(25), p(36)),
                    self.theme.tab,
                    theme::INK,
                    DT_LEFT,
                );
                let close_area = rect(right - p(24), p(7), right - p(3), p(29));
                if PtInRect(&close_area, mouse) != 0 {
                    theme::rounded(dc, close_area, theme::HOVER, theme::HOVER, p(4));
                }
                theme::close(dc, right - p(17), p(14), p(8), theme::MUTED);
            }
            RestoreDC(dc, saved);
            if self.tab_layout.overflow {
                let total = self.tab_layout.count as i32 * self.tab_layout.width;
                let thumb = (area.right * area.right / total).max(p(24));
                let max_scroll = (total - area.right).max(1);
                let x = self.tab_layout.offset * (area.right - thumb) / max_scroll;
                theme::fill(
                    dc,
                    rect(0, area.bottom - p(4), area.right, area.bottom),
                    theme::BORDER,
                );
                theme::rounded(
                    dc,
                    rect(x, area.bottom - p(4), x + thumb, area.bottom),
                    theme::MUTED,
                    theme::MUTED,
                    p(4),
                );
            }
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
            theme::fill(dc, area, theme::SIDEBAR);
            let selected = custom.nmcd.uItemState & CDIS_SELECTED != 0;
            let inside = self
                .drop_target
                .is_some_and(|target| target.note == Some(id) && target.zone == DropZone::Inside);
            let hovered = self.dragging.is_none() && self.tree_hover == Some(handle);
            if selected || inside || hovered {
                theme::rounded(
                    dc,
                    rect(2, area.top + 1, area.right - 2, area.bottom - 1),
                    if selected || inside {
                        theme::SELECTED
                    } else {
                        theme::HOVER
                    },
                    if inside {
                        theme::BLUE
                    } else if selected {
                        theme::SELECTED
                    } else {
                        theme::HOVER
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
                    theme::line(dc, &[(gx, area.top), (gx, area.bottom)], theme::BORDER, 1);
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
                        theme::INK,
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
                        theme::INK,
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
                    if selected { theme::BLUE } else { theme::MUTED },
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
                    theme::INK,
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
                        if self.dragging == Some(id) {
                            theme::BLUE
                        } else {
                            theme::rgb(157, 170, 189)
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
                theme::line(dc, &[(x, y), (area.right - p(4), y)], theme::BLUE, p(2));
                theme::rounded(
                    dc,
                    rect(x - p(3), y - p(3), x + p(3), y + p(3)),
                    theme::BLUE,
                    theme::BLUE,
                    p(6),
                );
            }
        }
    }

    pub unsafe fn paint_control(&self, draw: &DRAWITEMSTRUCT) {
        unsafe {
            let p = |v| self.theme.px(v);
            let dc = draw.hDC;
            let area = draw.rcItem;
            if draw.hwndItem == self.status {
                theme::fill(dc, area, theme::BAR);
                theme::line(dc, &[(0, 0), (area.right, 0)], theme::BORDER, 1);
                if self.dragging.is_some() {
                    theme::text(
                        dc,
                        &self.drag_description(),
                        rect(p(12), 0, area.right - p(8), area.bottom),
                        self.theme.small,
                        if self.drop_target.is_some() {
                            theme::BLUE
                        } else {
                            theme::MUTED
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
                    theme::document(dc, p(18), p(10), p(16), theme::MUTED);
                    theme::text(
                        dc,
                        title,
                        rect(p(44), 0, p(182).min(area.right / 3), area.bottom),
                        self.theme.small,
                        theme::INK,
                        DT_LEFT,
                    );
                    if area.right > p(660) {
                        theme::text(
                            dc,
                            &self.status_text,
                            rect(p(190), 0, area.right - p(420), area.bottom),
                            self.theme.small,
                            theme::MUTED,
                            DT_LEFT,
                        );
                    }
                    let mut start = 0u32;
                    SendMessageW(self.edit, EM_GETSEL, &mut start as *mut _ as usize, 0);
                    let line = SendMessageW(self.edit, EM_LINEFROMCHAR, start as usize, 0);
                    let line_start = SendMessageW(self.edit, EM_LINEINDEX, line as usize, 0);
                    let info = format!(
                        "Ln {}, Col {}   │   {}자   │   UTF-8   │   Plain Text",
                        line + 1,
                        start as isize - line_start + 1,
                        GetWindowTextLengthW(self.edit).max(0)
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
                        theme::MUTED,
                        DT_RIGHT,
                    );
                } else {
                    theme::text(
                        dc,
                        "Edot   ·   Plain Text",
                        rect(p(18), 0, area.right, area.bottom),
                        self.theme.small,
                        theme::MUTED,
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
            if let Some(index) = self
                .search_buttons
                .iter()
                .position(|&button| button == draw.hwndItem)
            {
                let checked = index == 0 && self.find_case_sensitive;
                let primary = index == 5;
                let color = if primary {
                    if hovered || pressed {
                        theme::rgb(25, 99, 220)
                    } else {
                        theme::BLUE
                    }
                } else if checked || pressed {
                    theme::SELECTED
                } else if hovered {
                    theme::HOVER
                } else {
                    theme::WHITE
                };
                theme::fill(dc, area, theme::WHITE);
                theme::rounded(
                    dc,
                    rect(1, 1, area.right - 1, area.bottom - 1),
                    color,
                    if focused {
                        theme::BLUE
                    } else if index == 4 {
                        theme::BORDER
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
                            theme::INK,
                            p(1),
                        );
                    }
                    3 => theme::close(dc, cx - p(4), cy - p(4), p(8), theme::MUTED),
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
                            theme::BLUE
                        } else {
                            theme::INK
                        },
                        DT_CENTER,
                    ),
                }
                return;
            }
            let base = if (draw.hwndItem != self.toggle && draw.hwndItem != self.add)
                || !self.sidebar_visible
            {
                theme::BAR
            } else {
                theme::SIDEBAR
            };
            theme::fill(dc, area, base);
            let color = if pressed {
                theme::SELECTED
            } else if hovered {
                theme::HOVER
            } else if draw.hwndItem == self.add {
                theme::WHITE
            } else {
                base
            };
            let border = if focused {
                theme::BLUE
            } else if draw.hwndItem == self.add {
                theme::BORDER
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
            if draw.hwndItem == self.add {
                theme::plus(dc, p(12), (area.bottom - p(12)) / 2, p(12), theme::INK);
                theme::text(
                    dc,
                    "새 메모",
                    rect(p(34), 0, area.right - p(8), area.bottom),
                    self.theme.ui,
                    theme::INK,
                    DT_LEFT,
                );
            } else if draw.hwndItem == self.tab_add {
                theme::plus(
                    dc,
                    (area.right - p(14)) / 2,
                    (area.bottom - p(14)) / 2,
                    p(14),
                    theme::INK,
                );
            } else if draw.hwndItem != self.toggle {
                let mut label = [0u16; 64];
                let n = GetWindowTextW(draw.hwndItem, label.as_mut_ptr(), 64).max(0) as usize;
                theme::text(
                    dc,
                    &String::from_utf16_lossy(&label[..n]),
                    area,
                    self.theme.small,
                    theme::INK,
                    DT_CENTER,
                );
            } else {
                let cx = area.right / 2;
                let cy = area.bottom / 2;
                let direction = if self.sidebar_visible { -1 } else { 1 };
                for offset in [-3, 3] {
                    theme::line(
                        dc,
                        &[
                            (cx + p(offset) - direction * p(2), cy - p(5)),
                            (cx + p(offset) + direction * p(2), cy),
                            (cx + p(offset) - direction * p(2), cy + p(5)),
                        ],
                        theme::INK,
                        p(1),
                    );
                }
            }
        }
    }
}
