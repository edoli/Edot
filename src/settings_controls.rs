//! Shared visual treatment for the settings dropdowns and wrap switch.
//! Native combo/button input, accessibility and selection semantics are retained.
use crate::{
    scrollbar::{self, Kind},
    theme::{self, Theme, rect},
    ui,
};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::SystemServices::SS_NOTIFY;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Default)]
pub struct Feedback {
    pub hovered: bool,
    pub focused: bool,
    pub pressed: bool,
    pub disabled: bool,
}
impl Feedback {
    pub unsafe fn read(hwnd: HWND, flags: u32) -> Self {
        unsafe {
            let mut point = POINT::default();
            GetCursorPos(&mut point);
            Self {
                hovered: WindowFromPoint(point) == hwnd,
                focused: GetFocus() == hwnd || flags & ODS_FOCUS != 0,
                pressed: flags & ODS_SELECTED != 0,
                disabled: IsWindowEnabled(hwnd) == 0 || flags & ODS_DISABLED != 0,
            }
        }
    }
}
pub unsafe fn attach_dropdown(hwnd: HWND) {
    unsafe {
        SetWindowSubclass(hwnd, Some(dropdown_proc), 1, 0);
        confirm_selection(hwnd);
        let mut info = COMBOBOXINFO {
            cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32,
            ..Default::default()
        };
        if GetComboBoxInfo(hwnd, &mut info) != 0 {
            let state = Box::into_raw(Box::new(ListState {
                combo: hwnd,
                bar: null_mut(),
                updating: false,
                mouse_navigation: false,
                hovered: None,
            }));
            if SetWindowSubclass(info.hwndList, Some(list_proc), 1, state as usize) == 0 {
                drop(Box::from_raw(state));
                return;
            }
            SetPropW(info.hwndList, LIST_STATE, state as HANDLE);
            let bar = CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                windows_sys::w!("목록 스크롤"),
                WS_CHILD | SS_NOTIFY,
                0,
                0,
                0,
                0,
                info.hwndList,
                null_mut(),
                GetWindowLongPtrW(info.hwndList, GWLP_HINSTANCE) as HINSTANCE,
                null(),
            );
            (*state).bar = bar;
            if !bar.is_null() {
                scrollbar::attach(
                    bar,
                    Kind::Dropdown {
                        list: info.hwndList,
                        combo: hwnd,
                    },
                );
            }
            let style = GetWindowLongPtrW(info.hwndList, GWL_STYLE) as u32;
            SetWindowLongPtrW(
                info.hwndList,
                GWL_STYLE,
                ((style & !(WS_VSCROLL | WS_HSCROLL)) | WS_CLIPCHILDREN) as isize,
            );
            ShowScrollBar(info.hwndList, SB_BOTH, 0);
        }
    }
}
struct ListState {
    combo: HWND,
    bar: HWND,
    updating: bool,
    mouse_navigation: bool,
    hovered: Option<u32>,
}
const RESTORE_CONFIRMED: u32 = WM_APP + 37;
pub unsafe fn defer_restore(combo: HWND) {
    unsafe {
        PostMessageW(combo, RESTORE_CONFIRMED, 0, 0);
    }
}
const CONFIRMED: *const u16 = windows_sys::w!("Edot.DropdownConfirmed");
pub unsafe fn confirmed_selection(combo: HWND) -> i32 {
    unsafe {
        let value = GetPropW(combo, CONFIRMED) as isize;
        if value == 0 {
            SendMessageW(combo, CB_GETCURSEL, 0, 0) as i32
        } else {
            value as i32 - 2
        }
    }
}
pub unsafe fn confirm_selection(combo: HWND) {
    unsafe {
        let selected = SendMessageW(combo, CB_GETCURSEL, 0, 0) as i32;
        SetPropW(combo, CONFIRMED, (selected + 2) as usize as HANDLE);
        let mut info = COMBOBOXINFO {
            cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32,
            ..Default::default()
        };
        if GetComboBoxInfo(combo, &mut info) != 0 {
            InvalidateRect(info.hwndList, null(), 0);
        }
        InvalidateRect(combo, null(), 0);
    }
}
pub unsafe fn restore_selection(combo: HWND) {
    unsafe {
        let selected = confirmed_selection(combo);
        SendMessageW(combo, CB_SETCURSEL, selected as usize, 0);
    }
}
const LIST_STATE: *const u16 = windows_sys::w!("Edot.DropdownListState");
unsafe fn list_state(list: HWND) -> *mut ListState {
    unsafe { GetPropW(list, LIST_STATE) as *mut ListState }
}
unsafe fn popup_bar(combo: HWND) -> HWND {
    unsafe {
        let mut info = COMBOBOXINFO {
            cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32,
            ..Default::default()
        };
        if GetComboBoxInfo(combo, &mut info) != 0 {
            let state = list_state(info.hwndList);
            if !state.is_null() {
                return (*state).bar;
            }
        }
        null_mut()
    }
}
unsafe fn refresh_list(list: HWND, state: *mut ListState) {
    unsafe {
        if (*state).updating || (*state).bar.is_null() {
            return;
        }
        (*state).updating = true;
        if GetWindowLongPtrW(list, GWL_STYLE) as u32 & (WS_VSCROLL | WS_HSCROLL) != 0 {
            ShowScrollBar(list, SB_BOTH, 0);
        }
        let mut area = RECT::default();
        GetClientRect(list, &mut area);
        let width = 12 * windows_sys::Win32::UI::HiDpi::GetDpiForWindow(list).max(96) as i32 / 96;
        let bar = (*state).bar;
        let mut bounds = RECT::default();
        GetWindowRect(bar, &mut bounds);
        let mut origin = POINT {
            x: bounds.left,
            y: bounds.top,
        };
        ScreenToClient(list, &mut origin);
        let x = (area.right - width).max(0);
        if origin.x != x
            || origin.y != 0
            || bounds.right - bounds.left != width
            || bounds.bottom - bounds.top != area.bottom
        {
            SetWindowPos(
                bar,
                HWND_TOP,
                x,
                0,
                width,
                area.bottom,
                SWP_NOACTIVATE | SWP_NOREDRAW,
            );
        }
        let overflow = scrollbar::list_range(list).extent() > 0;
        if (GetWindowLongPtrW(bar, GWL_STYLE) as u32 & WS_VISIBLE != 0) != overflow {
            ShowWindow(bar, if overflow { SW_SHOWNA } else { SW_HIDE });
        }
        InvalidateRect(bar, null(), 0);
        (*state).updating = false;
    }
}
pub unsafe fn style_dropdown(hwnd: HWND, theme: &Theme) {
    unsafe {
        SendMessageW(hwnd, CB_SETITEMHEIGHT, usize::MAX, theme.px(34) as isize);
        SendMessageW(hwnd, CB_SETITEMHEIGHT, 0, theme.px(36) as isize);
        SendMessageW(hwnd, CB_SETMINVISIBLE, 8, 0);
        let mut info = COMBOBOXINFO {
            cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32,
            ..Default::default()
        };
        if GetComboBoxInfo(hwnd, &mut info) != 0 {
            let state = list_state(info.hwndList);
            if !state.is_null() {
                refresh_list(info.hwndList, state);
            }
        }
        InvalidateRect(hwnd, null(), 0);
    }
}
unsafe extern "system" fn dropdown_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        if matches!(msg, WM_MOUSEWHEEL | WM_MOUSEHWHEEL)
            && SendMessageW(hwnd, CB_GETDROPPEDSTATE, 0, 0) != 0
        {
            let bar = popup_bar(hwnd);
            if !bar.is_null() {
                return SendMessageW(bar, msg, wp, lp);
            }
        }
        match msg {
            RESTORE_CONFIRMED => {
                restore_selection(hwnd);
                return 0;
            }
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                ui::paint_window(hwnd, |dc| {
                    ui::with_view(|v| paint_dropdown(hwnd, dc, &v.theme))
                });
                return 0;
            }
            WM_PRINT => return DefWindowProcW(hwnd, msg, wp, lp),
            WM_PRINTCLIENT => {
                ui::with_view(|v| paint_dropdown(hwnd, wp as HDC, &v.theme));
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
            }
            WM_NCDESTROY => {
                ui::release_buffer(hwnd);
                RemoveWindowSubclass(hwnd, Some(dropdown_proc), id);
                RemovePropW(hwnd, CONFIRMED);
            }
            _ => {}
        }
        let result = DefSubclassProc(hwnd, msg, wp, lp);
        if matches!(
            msg,
            WM_MOUSEMOVE
                | WM_MOUSELEAVE
                | WM_SETFOCUS
                | WM_KILLFOCUS
                | WM_LBUTTONDOWN
                | WM_LBUTTONUP
                | WM_KEYDOWN
                | WM_CHAR
                | CB_SETCURSEL
                | CB_SHOWDROPDOWN
                | WM_ENABLE
                | WM_THEMECHANGED
        ) {
            InvalidateRect(hwnd, null(), 0);
        }
        result
    }
}
unsafe extern "system" fn list_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    reference: usize,
) -> LRESULT {
    unsafe {
        let state = reference as *mut ListState;
        if msg == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(list_proc), id);
            RemovePropW(hwnd, LIST_STATE);
            drop(Box::from_raw(state));
            return DefSubclassProc(hwnd, msg, wp, lp);
        }
        if msg == WM_STYLECHANGING && wp as i32 == GWL_STYLE && lp != 0 {
            let style = &mut *(lp as *mut STYLESTRUCT);
            style.styleNew = (style.styleNew & !(WS_VSCROLL | WS_HSCROLL)) | WS_CLIPCHILDREN;
            return 0;
        }
        let bar = (*state).bar;
        if msg == WM_ERASEBKGND {
            let mut area = RECT::default();
            GetClientRect(hwnd, &mut area);
            ui::with_view(|view| theme::fill(wp as HDC, area, view.theme.colors.white));
            return 1;
        }
        if msg == WM_PRINTCLIENT {
            // Prepare the child before native animation takes its snapshot.
            refresh_list(hwnd, state);
            let result = DefSubclassProc(hwnd, msg, wp, lp);
            if !bar.is_null() && GetWindowLongPtrW(bar, GWL_STYLE) as u32 & WS_VISIBLE != 0 {
                let dc = wp as HDC;
                let saved = SaveDC(dc);
                let mut bounds = RECT::default();
                GetWindowRect(bar, &mut bounds);
                MapWindowPoints(null_mut(), hwnd, &mut bounds as *mut RECT as *mut POINT, 2);
                IntersectClipRect(dc, bounds.left, bounds.top, bounds.right, bounds.bottom);
                OffsetViewportOrgEx(dc, bounds.left, bounds.top, null_mut());
                SendMessageW(bar, WM_PRINTCLIENT, wp, PRF_CLIENT as isize);
                RestoreDC(dc, saved);
            }
            return result;
        }
        if matches!(msg, WM_MOUSEMOVE | WM_MOUSELEAVE | WM_KEYDOWN | WM_CHAR) {
            let old_hover = (*state).hovered;
            let old_mode = (*state).mouse_navigation;
            if matches!(msg, WM_KEYDOWN | WM_CHAR) {
                (*state).mouse_navigation = false;
                (*state).hovered = None;
            } else {
                (*state).mouse_navigation = true;
                (*state).hovered = None;
                if msg == WM_MOUSEMOVE {
                    let point = POINT {
                        x: lp as u16 as i16 as i32,
                        y: (lp as u32 >> 16) as u16 as i16 as i32,
                    };
                    let mut area = RECT::default();
                    GetClientRect(hwnd, &mut area);
                    if !bar.is_null() && GetWindowLongPtrW(bar, GWL_STYLE) as u32 & WS_VISIBLE != 0
                    {
                        area.right -= 12
                            * windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as i32
                            / 96;
                    }
                    if PtInRect(&area, point) != 0 {
                        let item = SendMessageW(hwnd, LB_ITEMFROMPOINT, 0, lp) as u32;
                        if item >> 16 == 0 {
                            (*state).hovered = Some(item & 0xffff);
                        }
                    }
                    let mut tracking = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut tracking);
                }
            }
            if old_hover != (*state).hovered || old_mode != (*state).mouse_navigation {
                InvalidateRect(hwnd, null(), 0);
            }
        }
        if !bar.is_null() {
            if matches!(msg, WM_MOUSEWHEEL | WM_MOUSEHWHEEL) {
                return SendMessageW(bar, msg, wp, lp);
            }
            if matches!(
                msg,
                WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP
            ) {
                let mut point = POINT {
                    x: lp as u16 as i16 as i32,
                    y: (lp as u32 >> 16) as u16 as i16 as i32,
                };
                MapWindowPoints(hwnd, bar, &mut point, 1);
                let mut area = RECT::default();
                GetClientRect(bar, &mut area);
                if scrollbar::interacting(bar)
                    || (GetWindowLongPtrW(bar, GWL_STYLE) as u32 & WS_VISIBLE != 0
                        && PtInRect(&area, point) != 0)
                {
                    let coordinates =
                        (point.x as u16 as u32 | ((point.y as u16 as u32) << 16)) as isize;
                    return SendMessageW(
                        bar,
                        if msg == WM_LBUTTONDBLCLK {
                            WM_LBUTTONDOWN
                        } else {
                            msg
                        },
                        wp,
                        coordinates,
                    );
                }
            }
            if matches!(msg, WM_CAPTURECHANGED | WM_CANCELMODE) || (msg == WM_SHOWWINDOW && wp == 0)
            {
                (*state).hovered = None;
                (*state).mouse_navigation = false;
                SendMessageW(bar, WM_CANCELMODE, 0, 0);
            }
        }
        // Native selection, outside clicks and Escape remain with the popup.
        // The child scrollbar borrows its capture instead of dismissing it.
        if msg == WM_NCPAINT {
            let result = DefSubclassProc(hwnd, msg, wp, lp);
            let dc = GetWindowDC(hwnd);
            if !dc.is_null() {
                let mut bounds = RECT::default();
                GetWindowRect(hwnd, &mut bounds);
                let saved = SaveDC(dc);
                // Leave the client rows and shared child scrollbar untouched.
                ExcludeClipRect(
                    dc,
                    2,
                    2,
                    bounds.right - bounds.left - 2,
                    bounds.bottom - bounds.top - 2,
                );
                ui::with_view(|v| {
                    theme::fill(
                        dc,
                        rect(0, 0, bounds.right - bounds.left, bounds.bottom - bounds.top),
                        v.theme.colors.white,
                    );
                    theme::rounded(
                        dc,
                        rect(0, 0, bounds.right - bounds.left, bounds.bottom - bounds.top),
                        v.theme.colors.white,
                        v.theme.colors.border,
                        v.theme.px(8),
                    );
                });
                RestoreDC(dc, saved);
                ReleaseDC(hwnd, dc);
            }
            return result;
        }
        let result = DefSubclassProc(hwnd, msg, wp, lp);
        if msg == WM_SIZE {
            let mut bounds = RECT::default();
            GetWindowRect(hwnd, &mut bounds);
            let radius = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) as i32 * 8 / 96;
            let region = CreateRoundRectRgn(
                0,
                0,
                bounds.right - bounds.left + 1,
                bounds.bottom - bounds.top + 1,
                radius,
                radius,
            );
            if !region.is_null() {
                let previous = CreateRectRgn(0, 0, 0, 0);
                let same = !previous.is_null()
                    && GetWindowRgn(hwnd, previous) > 0
                    && EqualRgn(previous, region) != 0;
                if !previous.is_null() {
                    DeleteObject(previous);
                }
                if same || SetWindowRgn(hwnd, region, 1) == 0 {
                    DeleteObject(region);
                }
            }
            InvalidateRect((*state).combo, null(), 0);
        }
        if matches!(
            msg,
            WM_SIZE
                | WM_SHOWWINDOW
                | WM_PAINT
                | WM_KEYDOWN
                | WM_CHAR
                | WM_MOUSEMOVE
                | WM_LBUTTONUP
                | LB_SETTOPINDEX
                | LB_SETCURSEL
                | LB_SETITEMHEIGHT
                | WM_VSCROLL
        ) {
            refresh_list(hwnd, state);
        }
        result
    }
}
unsafe fn item_text(hwnd: HWND, index: u32) -> String {
    unsafe {
        if index == u32::MAX {
            return String::new();
        }
        let len = SendMessageW(hwnd, CB_GETLBTEXTLEN, index as usize, 0);
        if !(0..32).contains(&len) {
            return String::new();
        }
        let mut text = [0u16; 32];
        SendMessageW(
            hwnd,
            CB_GETLBTEXT,
            index as usize,
            text.as_mut_ptr() as isize,
        );
        String::from_utf16_lossy(&text[..len as usize])
    }
}
unsafe fn paint_dropdown(hwnd: HWND, dc: HDC, theme: &Theme) {
    unsafe {
        let mut area = RECT::default();
        GetClientRect(hwnd, &mut area);
        let opened = SendMessageW(hwnd, CB_GETDROPPEDSTATE, 0, 0) != 0;
        let feedback = Feedback {
            pressed: opened,
            ..Feedback::read(hwnd, 0)
        };
        let index = confirmed_selection(hwnd) as u32;
        paint_field(dc, area, theme, &item_text(hwnd, index), feedback);
    }
}
pub unsafe fn paint_field(dc: HDC, area: RECT, theme: &Theme, label: &str, feedback: Feedback) {
    unsafe {
        let p = |n| theme.px(n);
        let colors = theme.colors;
        theme::fill(dc, area, colors.white);
        let border = if feedback.focused || feedback.pressed {
            colors.blue
        } else if feedback.hovered {
            colors.muted
        } else {
            colors.border
        };
        let field = rect(
            area.left + p(1),
            area.top + p(1),
            area.right - p(1),
            area.bottom - p(1),
        );
        theme::rounded(
            dc,
            field,
            if feedback.hovered || feedback.pressed {
                colors.bar
            } else {
                colors.white
            },
            border,
            p(10),
        );
        if feedback.focused || feedback.pressed {
            theme::rounded(
                dc,
                rect(
                    field.left + p(1),
                    field.top + p(1),
                    field.right - p(1),
                    field.bottom - p(1),
                ),
                if feedback.hovered || feedback.pressed {
                    colors.bar
                } else {
                    colors.white
                },
                border,
                p(8),
            );
        }
        theme::text(
            dc,
            label,
            rect(area.left + p(14), area.top, area.right - p(40), area.bottom),
            theme.ui,
            if feedback.disabled {
                colors.muted
            } else {
                colors.ink
            },
            DT_LEFT,
        );
        let x = area.right - p(22);
        let y = (area.top + area.bottom) / 2;
        theme::line(
            dc,
            &[(x - p(4), y - p(2)), (x, y + p(2)), (x + p(4), y - p(2))],
            if feedback.pressed {
                colors.blue
            } else {
                colors.muted
            },
            p(1),
        );
    }
}
pub unsafe fn paint_item(draw: &DRAWITEMSTRUCT, theme: &Theme) {
    unsafe {
        let p = |n| theme.px(n);
        let mut area = draw.rcItem;
        let dc = draw.hDC;
        let bar = popup_bar(draw.hwndItem);
        if !bar.is_null() && GetWindowLongPtrW(bar, GWL_STYLE) as u32 & WS_VISIBLE != 0 {
            area.right -= p(12);
        }
        let label = item_text(draw.hwndItem, draw.itemID);
        if draw.itemState & ODS_COMBOBOXEDIT != 0 {
            theme::fill(dc, area, theme.colors.white);
            theme::text(
                dc,
                &item_text(draw.hwndItem, confirmed_selection(draw.hwndItem) as u32),
                area,
                theme.ui,
                theme.colors.ink,
                DT_LEFT,
            );
            return;
        }
        let mut info = COMBOBOXINFO {
            cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32,
            ..Default::default()
        };
        let state = if GetComboBoxInfo(draw.hwndItem, &mut info) != 0 {
            list_state(info.hwndList)
        } else {
            null_mut()
        };
        let hovered = draw.itemState & ODS_SELECTED != 0
            && (state.is_null()
                || !(*state).mouse_navigation
                || (*state).hovered == Some(draw.itemID));
        let selected = confirmed_selection(draw.hwndItem) == draw.itemID as i32;
        theme::fill(dc, area, theme.colors.white);
        if hovered || selected {
            theme::rounded(
                dc,
                rect(
                    area.left + p(4),
                    area.top + p(2),
                    area.right - p(4),
                    area.bottom - p(2),
                ),
                if hovered {
                    theme.colors.selected
                } else {
                    theme.colors.bar
                },
                if hovered {
                    theme.colors.selected
                } else {
                    theme.colors.bar
                },
                p(8),
            );
        }
        theme::text(
            dc,
            &label,
            rect(area.left + p(14), area.top, area.right - p(36), area.bottom),
            theme.ui,
            if draw.itemState & ODS_DISABLED != 0 {
                theme.colors.muted
            } else {
                theme.colors.ink
            },
            DT_LEFT,
        );
        if selected {
            let x = area.right - p(24);
            let y = (area.top + area.bottom) / 2;
            theme::line(
                dc,
                &[(x - p(4), y), (x - p(1), y + p(3)), (x + p(5), y - p(3))],
                theme.colors.blue,
                p(2),
            );
        }
    }
}
fn blend(first: COLORREF, second: COLORREF, percent: u32) -> COLORREF {
    let channel = |shift: u32| {
        let a = (first >> shift) & 255u32;
        let b = (second >> shift) & 255u32;
        (a * (100 - percent) + b * percent) / 100
    };
    theme::rgb(channel(0) as u8, channel(8) as u8, channel(16) as u8)
}
pub unsafe fn paint_switch(dc: HDC, area: RECT, theme: &Theme, enabled: bool, feedback: Feedback) {
    unsafe {
        let p = |n| theme.px(n);
        let colors = theme.colors;
        theme::fill(dc, area, colors.white);
        theme::text(
            dc,
            "자동 줄 바꿈",
            rect(area.left + p(2), area.top, area.right - p(110), area.bottom),
            theme.ui,
            colors.ink,
            DT_LEFT,
        );
        theme::text(
            dc,
            if enabled { "켜짐" } else { "꺼짐" },
            rect(
                area.right - p(98),
                area.top,
                area.right - p(64),
                area.bottom,
            ),
            theme.small,
            if enabled { colors.blue } else { colors.muted },
            DT_RIGHT,
        );
        let x = area.right - p(54);
        let y = (area.top + area.bottom - p(26)) / 2;
        if feedback.focused {
            theme::rounded(
                dc,
                rect(x - p(3), y - p(3), x + p(51), y + p(29)),
                colors.white,
                colors.blue,
                p(32),
            );
        }
        let base = if enabled { colors.blue } else { colors.bar };
        let fill = if feedback.disabled {
            colors.border
        } else if feedback.pressed {
            blend(base, colors.ink, 15)
        } else if feedback.hovered {
            if enabled {
                blend(base, theme::WHITE, 10)
            } else {
                colors.hover
            }
        } else {
            base
        };
        let border = if enabled {
            fill
        } else if feedback.hovered {
            colors.muted
        } else {
            colors.border
        };
        theme::rounded(dc, rect(x, y, x + p(48), y + p(26)), fill, border, p(26));
        let knob_x = x + p(if enabled { 25 } else { 3 });
        // A one-pixel edge and a short shadow keep the white knob distinct.
        theme::rounded(
            dc,
            rect(knob_x, y + p(4), knob_x + p(20), y + p(24)),
            blend(fill, colors.ink, 18),
            blend(fill, colors.ink, 18),
            p(20),
        );
        theme::rounded(
            dc,
            rect(knob_x, y + p(3), knob_x + p(20), y + p(23)),
            theme::WHITE,
            blend(theme::WHITE, colors.border, 35),
            p(20),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null_mut;

    // Hidden native controls and an off-screen DIB. No mouse input, focus
    // changes or visible dropdowns are used while the desktop is occupied.
    #[test]
    fn styled_dropdown_preserves_native_selection_and_renders_settings() {
        unsafe {
            let parent = CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                windows_sys::w!("settings-render-test"),
                0,
                0,
                0,
                640,
                700,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            );
            assert!(!parent.is_null());
            let combo = CreateWindowExW(
                0,
                windows_sys::w!("COMBOBOX"),
                windows_sys::w!("theme"),
                WS_CHILD
                    | CBS_DROPDOWNLIST as u32
                    | CBS_OWNERDRAWFIXED as u32
                    | CBS_HASSTRINGS as u32,
                0,
                0,
                320,
                250,
                parent,
                null_mut(),
                null_mut(),
                null(),
            );
            assert!(!combo.is_null());
            attach_dropdown(combo);
            for label in [
                windows_sys::w!("밝게"),
                windows_sys::w!("어둡게"),
                windows_sys::w!("시스템 설정 사용"),
            ] {
                SendMessageW(combo, CB_ADDSTRING, 0, label as isize);
            }
            SendMessageW(combo, CB_SETCURSEL, 0, 0);
            confirm_selection(combo);
            SendMessageW(combo, WM_KEYDOWN, VK_DOWN as usize, 0);
            assert_eq!(SendMessageW(combo, CB_GETCURSEL, 0, 0), 1);
            SendMessageW(combo, WM_KEYDOWN, VK_HOME as usize, 0);
            assert_eq!(SendMessageW(combo, CB_GETCURSEL, 0, 0), 0);
            assert_eq!(item_text(combo, 0), "밝게");
            assert_eq!(item_text(combo, u32::MAX), "");
            let original_focus = GetFocus();
            let mut combo_info = COMBOBOXINFO {
                cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32,
                ..Default::default()
            };
            assert_ne!(GetComboBoxInfo(combo, &mut combo_info), 0);
            assert_eq!(IsWindowVisible(combo_info.hwndList), 0);
            for popup_width in [320, 400] {
                SetWindowPos(
                    combo_info.hwndList,
                    null_mut(),
                    0,
                    0,
                    popup_width,
                    240,
                    SWP_NOZORDER | SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOREDRAW,
                );
                let region = CreateRectRgn(0, 0, 0, 0);
                assert!(GetWindowRgn(combo_info.hwndList, region) > 0);
                assert_eq!(PtInRegion(region, 0, 0), 0);
                assert_ne!(PtInRegion(region, 20, 20), 0);
                DeleteObject(region);
                assert_eq!(IsWindowVisible(combo_info.hwndList), 0);
            }

            let width = 720i32;
            let height = 500i32;
            let dc = CreateCompatibleDC(null_mut());
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0);
            assert!(!dc.is_null() && !bitmap.is_null() && !pixels.is_null());
            let original_bitmap = SelectObject(dc, bitmap);
            let theme = Theme::new(96, false, "Consolas", 18);
            SendMessageW(combo, CB_SETCURSEL, 1, 0); // Native transient hover/preview.
            assert_eq!(confirmed_selection(combo), 0);
            let state = list_state(combo_info.hwndList);
            (*state).mouse_navigation = true;
            (*state).hovered = Some(1);
            let hover_area = rect(0, 0, 320, 36);
            paint_item(
                &DRAWITEMSTRUCT {
                    hwndItem: combo,
                    hDC: dc,
                    itemID: 1,
                    itemState: ODS_SELECTED,
                    rcItem: hover_area,
                    ..Default::default()
                },
                &theme,
            );
            let blue_pixels = |dc: HDC| {
                (0..36)
                    .flat_map(|y| (284..316).map(move |x| (x, y)))
                    .filter(|&(x, y)| GetPixel(dc, x, y) == theme.colors.blue)
                    .count()
            };
            assert_eq!(blue_pixels(dc), 0); // Hover never gets a checkmark.
            (*state).hovered = None; // Pointer left, native ODS_SELECTED remains.
            paint_item(
                &DRAWITEMSTRUCT {
                    hwndItem: combo,
                    hDC: dc,
                    itemID: 1,
                    itemState: ODS_SELECTED,
                    rcItem: hover_area,
                    ..Default::default()
                },
                &theme,
            );
            assert_eq!(blue_pixels(dc), 0);
            assert_eq!(GetPixel(dc, 10, 18), theme.colors.white);
            paint_item(
                &DRAWITEMSTRUCT {
                    hwndItem: combo,
                    hDC: dc,
                    itemID: 0,
                    rcItem: hover_area,
                    ..Default::default()
                },
                &theme,
            );
            assert!(blue_pixels(dc) > 0); // The original choice keeps its check.
            confirm_selection(combo);
            assert_eq!(confirmed_selection(combo), 1);
            SendMessageW(combo, CB_SETCURSEL, 2, 0);
            restore_selection(combo);
            assert_eq!(SendMessageW(combo, CB_GETCURSEL, 0, 0), 1);
            assert_eq!(confirmed_selection(combo), 1);
            SendMessageW(combo, CB_SETCURSEL, 0, 0);
            confirm_selection(combo);
            (*state).mouse_navigation = false;
            for (column, dark) in [(0, false), (1, true)] {
                let theme = Theme::new(96, dark, "Consolas", 18);
                style_dropdown(combo, &theme);
                assert_eq!(
                    SendMessageW(combo, CB_GETITEMHEIGHT, usize::MAX, 0),
                    theme.px(34) as isize
                );
                assert_eq!(
                    SendMessageW(combo, CB_GETITEMHEIGHT, 0, 0),
                    theme.px(36) as isize
                );
                let saved = SaveDC(dc);
                SetViewportOrgEx(dc, column * 360, 0, null_mut());
                theme::fill(dc, rect(0, 0, 360, height), theme.colors.white);
                theme::text(
                    dc,
                    if dark {
                        "어두운 테마"
                    } else {
                        "밝은 테마"
                    },
                    rect(20, 10, 340, 45),
                    theme.heading,
                    theme.colors.ink,
                    DT_LEFT,
                );
                for (row, label, feedback) in [
                    (0, "시스템 설정 사용", Feedback::default()),
                    (
                        1,
                        "Consolas",
                        Feedback {
                            hovered: true,
                            ..Default::default()
                        },
                    ),
                    (
                        2,
                        "Consolas",
                        Feedback {
                            focused: true,
                            pressed: true,
                            ..Default::default()
                        },
                    ),
                ] {
                    paint_field(
                        dc,
                        rect(20, 60 + row * 48, 340, 100 + row * 48),
                        &theme,
                        label,
                        feedback,
                    );
                }
                for index in 0..3 {
                    paint_item(
                        &DRAWITEMSTRUCT {
                            hwndItem: combo,
                            hDC: dc,
                            itemID: index as u32,
                            itemState: if index == 0 { ODS_SELECTED } else { 0 },
                            rcItem: rect(20, 216 + index * 36, 340, 252 + index * 36),
                            ..Default::default()
                        },
                        &theme,
                    );
                }
                paint_switch(
                    dc,
                    rect(20, 338, 340, 378),
                    &theme,
                    false,
                    Feedback::default(),
                );
                paint_switch(
                    dc,
                    rect(20, 386, 340, 426),
                    &theme,
                    true,
                    Feedback {
                        hovered: true,
                        ..Default::default()
                    },
                );
                paint_switch(
                    dc,
                    rect(20, 434, 340, 474),
                    &theme,
                    true,
                    Feedback {
                        focused: true,
                        pressed: true,
                        ..Default::default()
                    },
                );
                RestoreDC(dc, saved);
            }
            // Populate an overflowing native list without opening a popup.
            for index in 0..80 {
                let label: Vec<u16> = format!("Font {index:02}")
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                SendMessageW(combo, CB_ADDSTRING, 0, label.as_ptr() as isize);
            }
            let popup = combo_info.hwndList;
            let state = list_state(popup);
            assert!(!state.is_null() && !(*state).bar.is_null());
            let bar = (*state).bar;
            let range = scrollbar::list_range(popup);
            assert!(range.extent() > 0);
            refresh_list(popup, state);
            assert_ne!(GetWindowLongPtrW(bar, GWL_STYLE) as u32 & WS_VISIBLE, 0);
            assert_eq!(GetWindowLongPtrW(popup, GWL_STYLE) as u32 & WS_VSCROLL, 0);
            scrollbar::set_list_position(popup, range.extent() as i32);
            assert_eq!(scrollbar::list_range(popup).position as i64, range.extent());
            assert_eq!(SendMessageW(combo, CB_GETCURSEL, 0, 0), 0);
            scrollbar::set_list_position(popup, 0);
            let wheel = ((-30i16 as u16 as usize) << 16) as WPARAM;
            for _ in 0..3 {
                SendMessageW(bar, WM_MOUSEWHEEL, wheel, 0);
            }
            assert_eq!(scrollbar::list_range(popup).position, 0);
            SendMessageW(bar, WM_MOUSEWHEEL, wheel, 0);
            let mut lines = 0u32;
            SystemParametersInfoW(
                SPI_GETWHEELSCROLLLINES,
                0,
                &mut lines as *mut _ as *mut _,
                0,
            );
            let expected = if lines == u32::MAX {
                range.page as i64
            } else {
                i64::from(lines)
            }
            .min(range.extent());
            assert_eq!(scrollbar::list_range(popup).position as i64, expected);
            assert_eq!(SendMessageW(combo, CB_GETCURSEL, 0, 0), 0);
            SendMessageW(combo, WM_KEYDOWN, VK_END as usize, 0);
            assert_eq!(SendMessageW(combo, CB_GETCURSEL, 0, 0), 82);
            SendMessageW(combo, WM_KEYDOWN, VK_HOME as usize, 0);
            assert_eq!(SendMessageW(combo, CB_GETCURSEL, 0, 0), 0);
            assert_eq!(GetWindowLongPtrW(popup, GWL_STYLE) as u32 & WS_VSCROLL, 0);
            SendMessageW(bar, WM_CANCELMODE, 0, 0);
            assert!(!scrollbar::interacting(bar));
            assert_eq!(IsWindowVisible(popup), 0);
            GdiFlush();
            let bytes =
                std::slice::from_raw_parts(pixels as *const u8, (width * height * 4) as usize);
            let mut bmp = Vec::with_capacity(54 + bytes.len());
            bmp.extend_from_slice(b"BM");
            bmp.extend_from_slice(&(54 + bytes.len() as u32).to_le_bytes());
            bmp.extend_from_slice(&[0; 4]);
            bmp.extend_from_slice(&54u32.to_le_bytes());
            bmp.extend_from_slice(&40u32.to_le_bytes());
            bmp.extend_from_slice(&width.to_le_bytes());
            bmp.extend_from_slice(&(-height).to_le_bytes());
            bmp.extend_from_slice(&1u16.to_le_bytes());
            bmp.extend_from_slice(&32u16.to_le_bytes());
            bmp.extend_from_slice(&0u32.to_le_bytes());
            bmp.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            bmp.extend_from_slice(&[0; 16]);
            bmp.extend_from_slice(bytes);
            std::fs::create_dir_all("target/settings-controls-validation").unwrap();
            std::fs::write("target/settings-controls-validation/preview.bmp", bmp).unwrap();
            assert_eq!(GetFocus(), original_focus);
            assert_eq!(SendMessageW(combo, CB_GETCURSEL, 0, 0), 0);
            SelectObject(dc, original_bitmap);
            DeleteObject(bitmap);
            DeleteDC(dc);
            DestroyWindow(parent);
        }
    }
}
