use crate::theme::rect;
use crate::{app, editing, theme, ui};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Default, Debug)]
pub struct ScrollRange {
    pub min: i32,
    pub max: i32,
    pub page: u32,
    pub position: i32,
}

impl ScrollRange {
    pub unsafe fn read(editor: HWND, horizontal: bool) -> Self {
        let mut info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            ..Default::default()
        };
        unsafe {
            GetScrollInfo(
                editor,
                if horizontal { SB_HORZ } else { SB_VERT },
                &mut info,
            );
        }
        Self {
            min: info.nMin,
            max: info.nMax,
            page: info.nPage,
            position: info.nPos,
        }
    }

    pub fn extent(self) -> i64 {
        // RichEdit leaves the default 0..100, page=0 range on disabled bars.
        if self.page == 0 {
            return 0;
        }
        (i64::from(self.max) - i64::from(self.min) + 1 - i64::from(self.page.max(1))).max(0)
    }

    pub fn thumb(self, length: i32, minimum: i32) -> (i32, i32) {
        let length = length.max(0);
        let total = (i64::from(self.max) - i64::from(self.min) + 1).max(1);
        let size = (i64::from(length) * i64::from(self.page) / total)
            .clamp(i64::from(minimum.min(length).max(0)), i64::from(length))
            as i32;
        let offset = (i64::from(self.position) - i64::from(self.min)).clamp(0, self.extent());
        let start = (offset * i64::from(length - size) / self.extent().max(1)) as i32;
        (start, size)
    }
}

#[derive(Clone, Copy)]
pub struct ThumbDrag {
    pub range: ScrollRange,
    pub pointer: i32,
    pub length: i32,
    pub minimum: i32,
}

impl ThumbDrag {
    pub fn position(self, pointer: i32) -> i32 {
        let (_, size) = self.range.thumb(self.length, self.minimum);
        let travel = (self.length - size).max(1);
        let delta = (i64::from(pointer) - i64::from(self.pointer)) * self.range.extent()
            / i64::from(travel);
        (i64::from(self.range.position) + delta).clamp(
            i64::from(self.range.min),
            i64::from(self.range.min) + self.range.extent(),
        ) as i32
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    EditorVertical,
    EditorHorizontal,
    Tree,
    Tabs,
    Dropdown { list: HWND, combo: HWND },
}
impl Kind {
    pub fn horizontal(self) -> bool {
        matches!(self, Self::EditorHorizontal | Self::Tabs)
    }
}
#[derive(Clone, Copy)]
enum Gesture {
    Thumb(ThumbDrag),
    Page(i32),
}
#[derive(Default)]
struct WheelDelta(i32);
impl WheelDelta {
    fn take(&mut self, delta: i32) -> i32 {
        self.0 += delta;
        let ticks = self.0 / WHEEL_DELTA as i32;
        self.0 %= WHEEL_DELTA as i32;
        ticks
    }
}
struct State {
    kind: Kind,
    gesture: Option<Gesture>,
    wheel: WheelDelta,
}
const REPEAT: usize = 1;
const STATE_PROPERTY: *const u16 = windows_sys::w!("Edot.ScrollState");
pub unsafe fn attach(hwnd: HWND, kind: Kind) {
    let state = Box::into_raw(Box::new(State {
        kind,
        gesture: None,
        wheel: WheelDelta::default(),
    }));
    unsafe {
        if SetWindowSubclass(hwnd, Some(procedure), 1, state as usize) == 0 {
            drop(Box::from_raw(state));
        } else {
            SetPropW(hwnd, STATE_PROPERTY, state as HANDLE);
        }
    }
}
unsafe fn snapshot(hwnd: HWND, kind: Kind) -> (ScrollRange, i32, i32) {
    unsafe {
        let mut area = RECT::default();
        GetClientRect(hwnd, &mut area);
        let mut range = ScrollRange::default();
        if let Kind::Dropdown { list, .. } = kind {
            range = list_range(list);
        } else {
            ui::with_view(|v| range = v.scroll_range(kind));
        }
        (
            range,
            if kind.horizontal() {
                area.right
            } else {
                area.bottom
            },
            24 * GetDpiForWindow(hwnd).max(96) as i32 / 96,
        )
    }
}
pub unsafe fn list_range(list: HWND) -> ScrollRange {
    unsafe {
        let mut area = RECT::default();
        GetClientRect(list, &mut area);
        let height = SendMessageW(list, LB_GETITEMHEIGHT, 0, 0).max(1) as i32;
        ScrollRange {
            min: 0,
            max: SendMessageW(list, LB_GETCOUNT, 0, 0).max(0) as i32 - 1,
            page: (area.bottom / height).max(1) as u32,
            position: SendMessageW(list, LB_GETTOPINDEX, 0, 0).max(0) as i32,
        }
    }
}
pub unsafe fn set_list_position(list: HWND, target: i32) {
    unsafe {
        let range = list_range(list);
        let target = target.clamp(0, range.extent() as i32);
        if target != range.position {
            SendMessageW(list, LB_SETTOPINDEX, target as usize, 0);
        }
    }
}
unsafe fn set_position(kind: Kind, position: i32) {
    unsafe {
        if let Kind::Dropdown { list, .. } = kind {
            set_list_position(list, position);
        } else {
            app::scroll_to(kind, position);
        }
    }
}
unsafe fn captured(hwnd: HWND, kind: Kind) -> bool {
    unsafe {
        let capture = GetCapture();
        match kind {
            Kind::Dropdown { list, combo } => capture == list || capture == combo,
            _ => capture == hwnd,
        }
    }
}
// Dropdowns retain the native popup capture so a thumb click does not close
// the menu. Other app scrollbars own and release their capture as before.
unsafe fn capture(hwnd: HWND, kind: Kind) {
    unsafe {
        match kind {
            Kind::Dropdown { list, .. } => {
                if !captured(hwnd, kind) {
                    SetCapture(list);
                }
            }
            _ => {
                SetCapture(hwnd);
            }
        }
    }
}
pub unsafe fn interacting(hwnd: HWND) -> bool {
    unsafe {
        let state = GetPropW(hwnd, STATE_PROPERTY) as *const State;
        !state.is_null() && (*state).gesture.is_some()
    }
}
unsafe fn page(hwnd: HWND, kind: Kind, direction: i32) {
    unsafe {
        let mut pointer = POINT::default();
        GetCursorPos(&mut pointer);
        ScreenToClient(hwnd, &mut pointer);
        let mut area = RECT::default();
        GetClientRect(hwnd, &mut area);
        let (range, length, minimum) = snapshot(hwnd, kind);
        let (start, size) = range.thumb(length, minimum);
        let coordinate = if kind.horizontal() {
            pointer.x
        } else {
            pointer.y
        };
        if PtInRect(&area, pointer) != 0
            && ((direction < 0 && coordinate < start)
                || (direction > 0 && coordinate >= start + size))
        {
            set_position(
                kind,
                (i64::from(range.position) + i64::from(direction) * i64::from(range.page.max(1)))
                    .clamp(0, range.extent()) as i32,
            );
        }
    }
}
unsafe extern "system" fn procedure(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    reference: usize,
) -> LRESULT {
    unsafe {
        let state = reference as *mut State;
        let kind = (*state).kind;
        let coordinate = if kind.horizontal() {
            lp as u16 as i16 as i32
        } else {
            (lp as u32 >> 16) as u16 as i16 as i32
        };
        match msg {
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                ui::paint_window(hwnd, |dc| ui::with_view(|v| paint(dc, v, kind, hwnd)));
                return 0;
            }
            WM_PRINT => return DefWindowProcW(hwnd, msg, wp, lp),
            WM_PRINTCLIENT => {
                // Dropdown animation renders to a supplied DC, not through WM_PAINT.
                ui::with_view(|v| paint(wp as HDC, v, kind, hwnd));
                return 0;
            }
            WM_LBUTTONDOWN => {
                let (range, length, minimum) = snapshot(hwnd, kind);
                if range.extent() > 0 {
                    let (start, size) = range.thumb(length, minimum);
                    capture(hwnd, kind);
                    (*state).gesture = Some(if (start..start + size).contains(&coordinate) {
                        Gesture::Thumb(ThumbDrag {
                            range,
                            pointer: coordinate,
                            length,
                            minimum,
                        })
                    } else {
                        Gesture::Page(if coordinate < start { -1 } else { 1 })
                    });
                    if let Some(Gesture::Page(direction)) = (*state).gesture {
                        page(hwnd, kind, direction);
                        SetTimer(hwnd, REPEAT, 400, None);
                    }
                    InvalidateRect(hwnd, null(), 0);
                }
                return 0;
            }
            WM_MOUSEMOVE => {
                let mut tracking = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut tracking);
                if captured(hwnd, kind)
                    && let Some(Gesture::Thumb(drag)) = (*state).gesture
                {
                    set_position(kind, drag.position(coordinate));
                }
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_MOUSELEAVE => {
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_TIMER if wp == REPEAT => {
                if captured(hwnd, kind) {
                    SetTimer(hwnd, REPEAT, 60, None);
                    if let Some(Gesture::Page(direction)) = (*state).gesture {
                        page(hwnd, kind, direction);
                    }
                } else {
                    KillTimer(hwnd, REPEAT);
                }
                return 0;
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED | WM_CANCELMODE => {
                (*state).gesture = None;
                KillTimer(hwnd, REPEAT);
                if !matches!(kind, Kind::Dropdown { .. }) && GetCapture() == hwnd {
                    ReleaseCapture();
                }
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta =
                    (wp >> 16) as u16 as i16 as i32 * if msg == WM_MOUSEWHEEL { -1 } else { 1 };
                let ticks = (*state).wheel.take(delta);
                if ticks != 0 {
                    let (range, _, _) = snapshot(hwnd, kind);
                    let mut lines = 3u32;
                    SystemParametersInfoW(
                        if msg == WM_MOUSEHWHEEL {
                            SPI_GETWHEELSCROLLCHARS
                        } else {
                            SPI_GETWHEELSCROLLLINES
                        },
                        0,
                        &mut lines as *mut _ as *mut _,
                        0,
                    );
                    let unit = match kind {
                        Kind::Tree | Kind::Dropdown { .. } => 1,
                        Kind::Tabs => {
                            let mut pixels = 32;
                            ui::with_view(|v| pixels = v.theme.px(32));
                            pixels
                        }
                        _ => line_height(ui_editor()),
                    };
                    let step = if lines == u32::MAX {
                        range.page as i64
                    } else {
                        i64::from(lines) * i64::from(unit)
                    };
                    set_position(
                        kind,
                        (i64::from(range.position) + i64::from(ticks) * step)
                            .clamp(0, range.extent()) as i32,
                    );
                }
                return 0;
            }
            WM_NCDESTROY => {
                KillTimer(hwnd, REPEAT);
                ui::release_buffer(hwnd);
                RemoveWindowSubclass(hwnd, Some(procedure), id);
                RemovePropW(hwnd, STATE_PROPERTY);
                drop(Box::from_raw(state));
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
unsafe fn paint(dc: HDC, view: &ui::View, kind: Kind, control: HWND) {
    unsafe {
        let mut area = RECT::default();
        let horizontal = kind.horizontal();
        GetClientRect(control, &mut area);
        theme::fill(
            dc,
            area,
            match kind {
                Kind::Tree => view.theme.colors.sidebar,
                Kind::Tabs => view.theme.colors.bar,
                _ => view.theme.colors.white,
            },
        );
        let mut pointer = POINT::default();
        GetCursorPos(&mut pointer);
        ScreenToClient(control, &mut pointer);
        let hovered = PtInRect(&area, pointer) != 0 && {
            let mut screen = pointer;
            ClientToScreen(control, &mut screen);
            WindowFromPoint(screen) == control
        };
        let pressed = if matches!(kind, Kind::Dropdown { .. }) {
            interacting(control)
        } else {
            GetCapture() == control
        };
        if hovered || pressed {
            theme::fill(dc, area, view.theme.colors.hover);
        }
        let range = view.scroll_range(kind);
        if range.extent() == 0 {
            return;
        }
        let length = if horizontal { area.right } else { area.bottom };
        let (start, size) = range.thumb(length, view.theme.px(24));
        let thickness = view.theme.px(if hovered || pressed { 8 } else { 6 });
        let cross = if horizontal { area.bottom } else { area.right };
        let inset = ((cross - thickness) / 2).max(0);
        let thumb = if horizontal {
            rect(start, inset, start + size, cross - inset)
        } else {
            rect(inset, start, cross - inset, start + size)
        };
        let color = if pressed {
            view.theme.colors.blue
        } else if hovered {
            view.theme.colors.ink
        } else {
            view.theme.colors.muted
        };
        theme::rounded(dc, thumb, color, color, thickness);
    }
}

fn ui_editor() -> HWND {
    let mut editor = null_mut();
    ui::with_view(|v| editor = v.edit);
    editor
}
pub unsafe fn line_height(editor: HWND) -> i32 {
    unsafe {
        let dc = GetDC(editor);
        let font = SendMessageW(editor, WM_GETFONT, 0, 0);
        let old = SelectObject(dc, font as _);
        let mut metrics = TEXTMETRICW::default();
        GetTextMetricsW(dc, &mut metrics);
        SelectObject(dc, old);
        ReleaseDC(editor, dc);
        metrics.tmHeight.max(1)
    }
}
const OFFSET_X: *const u16 = windows_sys::w!("Edot.BodyOffsetX");
const OFFSET_Y: *const u16 = windows_sys::w!("Edot.BodyOffsetY");
const ANCHOR_X: *const u16 = windows_sys::w!("Edot.BodyAnchorX");
const ANCHOR_Y: *const u16 = windows_sys::w!("Edot.BodyAnchorY");
const WRAP: *const u16 = windows_sys::w!("Edot.BodyWrap");
unsafe fn padding(editor: HWND, horizontal: bool) -> i32 {
    unsafe { (if horizontal { 12 } else { 8 }) * GetDpiForWindow(editor) as i32 / 96 }
}
unsafe fn offset(editor: HWND, horizontal: bool) -> i32 {
    unsafe {
        let value = GetPropW(editor, if horizontal { OFFSET_X } else { OFFSET_Y }) as isize;
        if value == 0 {
            -padding(editor, horizontal)
        } else {
            value as i32 - 65536
        }
    }
}
unsafe fn store_offset(editor: HWND, horizontal: bool, value: i32) {
    unsafe {
        SetPropW(
            editor,
            if horizontal { OFFSET_X } else { OFFSET_Y },
            (value + 65536) as usize as HANDLE,
        );
        // SB_BOTTOM may stop a few pixels before GetScrollInfo's nominal end.
        // Preserve the actual native anchor instead of assuming exact equality.
        SetPropW(
            editor,
            if horizontal { ANCHOR_X } else { ANCHOR_Y },
            (ScrollRange::read(editor, horizontal).position as usize + 1) as HANDLE,
        );
    }
}
pub unsafe fn configure_body(editor: HWND, wrap: bool) {
    unsafe {
        let previous = GetPropW(editor, WRAP) as usize;
        SetPropW(editor, WRAP, if wrap { 2usize } else { 1usize } as HANDLE);
        let margin = if wrap {
            padding(editor, true) as usize
        } else {
            0
        };
        SendMessageW(
            editor,
            EM_SETMARGINS,
            (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize,
            (margin | margin << 16) as isize,
        );
        if previous != if wrap { 2 } else { 1 } {
            store_offset(editor, true, if wrap { 0 } else { -padding(editor, true) });
        }
    }
}
pub unsafe fn body_range(editor: HWND, horizontal: bool) -> ScrollRange {
    unsafe {
        let raw = ScrollRange::read(editor, horizontal);
        if horizontal && GetPropW(editor, WRAP) as usize == 2 {
            return ScrollRange::default();
        }
        let p = padding(editor, horizontal);
        let mut area = RECT::default();
        GetClientRect(GetParent(GetParent(editor)), &mut area);
        let page = if horizontal { area.right } else { area.bottom }.max(1) as u32;
        let total = if raw.page != 0 {
            raw.max - raw.min + 1
        } else if horizontal {
            page as i32 - 2 * p
        } else {
            let lines = SendMessageW(editor, EM_GETLINECOUNT, 0, 0).max(1) as i32;
            lines.saturating_mul(line_height(editor))
        };
        let extent = (total + 2 * p - page as i32).max(0);
        ScrollRange {
            min: 0,
            max: (total + 2 * p - 1).max(0),
            page,
            position: if offset(editor, horizontal) >= p {
                extent
            } else {
                (raw.position + p + offset(editor, horizontal)).clamp(0, extent)
            },
        }
    }
}
pub unsafe fn set_body_position(editor: HWND, horizontal: bool, target: i32) {
    unsafe {
        if horizontal && GetPropW(editor, WRAP) as usize == 2 {
            store_offset(editor, true, 0);
            layout_body(editor);
            return;
        }
        let range = body_range(editor, horizontal);
        let target = target.clamp(0, range.extent() as i32);
        let raw = ScrollRange::read(editor, horizontal);
        let p = padding(editor, horizontal);
        let native = (target - p).clamp(0, raw.extent() as i32);
        set_native_position(editor, horizontal, native);
        store_offset(editor, horizontal, (target - p - native).clamp(-p, p));
        layout_body(editor);
    }
}
unsafe fn position_window(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) -> bool {
    unsafe {
        let mut bounds = RECT::default();
        GetWindowRect(hwnd, &mut bounds);
        let mut origin = POINT {
            x: bounds.left,
            y: bounds.top,
        };
        ScreenToClient(GetParent(hwnd), &mut origin);
        if origin.x == x
            && origin.y == y
            && bounds.right - bounds.left == width
            && bounds.bottom - bounds.top == height
        {
            return false;
        }
        SetWindowPos(
            hwnd,
            null_mut(),
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW | SWP_NOCOPYBITS,
        );
        true
    }
}
// The native client keeps the full viewport size. Only virtual empty space at
// the document edges changes the clipping child; middle positions use the full
// boundary. Moving the editor at the far edge also clips its native scrollbars.
pub unsafe fn layout_body(editor: HWND) {
    unsafe {
        if GetPropW(editor, WRAP).is_null() {
            return;
        }
        let clip = GetParent(editor);
        let host = GetParent(clip);
        let mut area = RECT::default();
        GetClientRect(host, &mut area);
        for horizontal in [false, true] {
            let raw = ScrollRange::read(editor, horizontal);
            let edge = offset(editor, horizontal);
            if (edge < 0 && raw.position > 0)
                || (edge > 0
                    && raw.position as isize + 1
                        != GetPropW(editor, if horizontal { ANCHOR_X } else { ANCHOR_Y }) as isize)
            {
                store_offset(editor, horizontal, 0);
            }
        }
        let ox = offset(editor, true).clamp(-padding(editor, true), padding(editor, true));
        let oy = offset(editor, false).clamp(-padding(editor, false), padding(editor, false));
        let left = (-ox).max(0);
        let top = (-oy).max(0);
        let right = ox.max(0);
        let bottom = oy.max(0);
        let changed = position_window(
            clip,
            left,
            top,
            (area.right - left - right).max(0),
            (area.bottom - top - bottom).max(0),
        );
        let wrap = GetPropW(editor, WRAP) as usize == 2;
        let changed = position_window(
            editor,
            -right,
            -bottom,
            area.right + GetSystemMetricsForDpi(SM_CXVSCROLL, GetDpiForWindow(editor)),
            area.bottom
                + if wrap {
                    0
                } else {
                    GetSystemMetricsForDpi(SM_CYHSCROLL, GetDpiForWindow(editor))
                },
        ) || changed;
        if changed {
            RedrawWindow(
                host,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
            );
        }
    }
}
unsafe fn set_native_position(editor: HWND, horizontal: bool, target: i32) {
    unsafe {
        let range = ScrollRange::read(editor, horizontal);
        if target == range.position {
            return;
        }
        if target == 0 || i64::from(target) >= range.extent() {
            SendMessageW(
                editor,
                if horizontal { WM_HSCROLL } else { WM_VSCROLL },
                if target == 0 { SB_TOP } else { SB_BOTTOM } as usize,
                0,
            );
            return;
        }
        // Let RichEdit move a single axis whenever the native thumb message
        // can represent it. EM_SETSCROLLPOS re-clamps BOTH axes.
        if (0..=u16::MAX as i32).contains(&target) {
            SendMessageW(
                editor,
                if horizontal { WM_HSCROLL } else { WM_VSCROLL },
                SB_THUMBPOSITION as usize | ((target as usize) << 16),
                0,
            );
            return;
        }
        let other = ScrollRange::read(editor, !horizontal);
        let first_line = SendMessageW(editor, EM_GETFIRSTVISIBLELINE, 0, 0);
        let mut position = POINT {
            x: ScrollRange::read(editor, true).position,
            y: ScrollRange::read(editor, false).position,
        };
        if horizontal {
            position.x = target;
        } else {
            position.y = target;
        }
        SendMessageW(
            editor,
            editing::EM_SETSCROLLPOS,
            0,
            &position as *const _ as LPARAM,
        );
        if ScrollRange::read(editor, !horizontal).position != other.position {
            let message = if horizontal { WM_VSCROLL } else { WM_HSCROLL };
            if i64::from(other.position - other.min) >= other.extent() - 1 {
                SendMessageW(editor, message, SB_BOTTOM as usize, 0);
            } else if (0..=u16::MAX as i32).contains(&other.position) {
                SendMessageW(
                    editor,
                    message,
                    SB_THUMBPOSITION as usize | ((other.position as usize) << 16),
                    0,
                );
            } else if horizontal {
                let delta = first_line - SendMessageW(editor, EM_GETFIRSTVISIBLELINE, 0, 0);
                SendMessageW(editor, EM_LINESCROLL, 0, delta);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fine_wheel_deltas_accumulate_without_losing_direction() {
        let mut wheel = WheelDelta::default();
        assert_eq!(wheel.take(30), 0);
        assert_eq!(wheel.take(30), 0);
        assert_eq!(wheel.take(30), 0);
        assert_eq!(wheel.take(30), 1);
        assert_eq!(wheel.take(-60), 0);
        assert_eq!(wheel.take(60), 0);
        assert_eq!(wheel.take(-300), -2);
        assert_eq!(wheel.take(-60), -1);
    }
    #[test]
    fn dragging_keeps_the_grab_position_and_reaches_both_ends() {
        let drag = ThumbDrag {
            range: ScrollRange {
                min: 0,
                max: 2_000_000,
                page: 600,
                position: 123_457,
            },
            pointer: 71,
            length: 500,
            minimum: 24,
        };
        assert_eq!(drag.position(71), 123_457);
        assert!(drag.position(72) > 123_457);
        assert_eq!(drag.position(71), 123_457);
        assert_eq!(drag.position(i32::MIN), 0);
        assert_eq!(i64::from(drag.position(i32::MAX)), drag.range.extent());
    }
    #[test]
    fn thumb_reaches_both_ends_with_large_pixel_ranges() {
        let range = ScrollRange {
            min: 0,
            max: 2_000_000,
            page: 600,
            position: 0,
        };
        assert_eq!(range.thumb(500, 24), (0, 24));
        let drag = ThumbDrag {
            range,
            pointer: 0,
            length: 500,
            minimum: 24,
        };
        let end = drag.position(500);
        assert!(end > 65_535);
        assert_eq!(
            ScrollRange {
                position: end,
                ..range
            }
            .thumb(500, 24),
            (476, 24)
        );
        assert_eq!(drag.position(-5), 0);
        assert_eq!(ThumbDrag { length: 0, ..drag }.position(0), 0);
        assert_eq!(range.thumb(10, 24), (0, 10));
        assert_eq!(
            ScrollRange {
                max: 100,
                ..Default::default()
            }
            .extent(),
            0
        );
        assert_eq!(
            ScrollRange {
                min: 0,
                max: 99,
                page: 100,
                position: 0
            }
            .extent(),
            0
        );
    }
}
