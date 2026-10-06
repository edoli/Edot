//! Reusable Win32 pane resizing. Sizes and limits use 96-DPI logical pixels;
//! pointer anchors use screen coordinates so moving the bar cannot shift a drag.
use crate::theme::{self, Theme, rect};
use crate::ui;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// Both axes are part of the reusable API; the app currently resizes only width.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Horizontal,
    Vertical,
}
impl Axis {
    fn coordinate(self, point: POINT) -> i32 {
        match self {
            Self::Horizontal => point.x,
            Self::Vertical => point.y,
        }
    }
    fn cursor(self) -> *const u16 {
        match self {
            Self::Horizontal => IDC_SIZEWE,
            Self::Vertical => IDC_SIZENS,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Config {
    pub axis: Axis,
    pub value: i32,
    pub min: i32,
    pub max: i32,
    pub dpi: u32,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Feedback {
    pub hovered: bool,
    pub dragging: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Begin,
    Resize(i32),
    End,
}
pub type Paint = unsafe fn(HWND, HDC, Axis, Feedback);
pub type Notify = unsafe fn(HWND, Event);
pub const HIT_RADIUS: i32 = 4;

#[derive(Clone, Copy)]
struct Drag {
    config: Config,
    pointer: i32,
    last: i32,
}
impl Drag {
    fn value(self, pointer: i32) -> i32 {
        let dpi = i64::from(self.config.dpi.max(96));
        let delta = i64::from(pointer) - i64::from(self.pointer);
        let scaled = (delta * 96 + delta.signum() * (dpi / 2)) / dpi;
        (i64::from(self.config.value) + scaled)
            .clamp(i64::from(self.config.min), i64::from(self.config.max)) as i32
    }
}
struct State {
    config: Config,
    paint: Paint,
    notify: Notify,
    hovered: bool,
    drag: Option<Drag>,
}
const STATE: *const u16 = windows_sys::w!("Edot.SplitterState");

unsafe fn state(hwnd: HWND) -> *mut State {
    unsafe { GetPropW(hwnd, STATE) as *mut State }
}

/// Attach to an SS_NOTIFY child. The owner supplies pane layout/backgrounds and
/// handles Resize events. No App access or focus changes occur in this module.
pub unsafe fn attach(hwnd: HWND, config: Config, paint: Paint, notify: Notify) -> bool {
    unsafe {
        if !state(hwnd).is_null() {
            return false;
        }
        let state = Box::into_raw(Box::new(State {
            config,
            paint,
            notify,
            hovered: false,
            drag: None,
        }));
        if SetPropW(hwnd, STATE, state as HANDLE) == 0 {
            drop(Box::from_raw(state));
            return false;
        }
        if SetWindowSubclass(hwnd, Some(procedure), 1, 0) == 0 {
            RemovePropW(hwnd, STATE);
            drop(Box::from_raw(state));
            return false;
        }
        configure(hwnd, config);
        true
    }
}

/// Refresh limits/value after layout. A live drag retains its original anchor;
/// changing its axis or DPI ends it before applying the new configuration.
pub unsafe fn configure(hwnd: HWND, mut config: Config) {
    unsafe {
        config.dpi = config.dpi.max(96);
        config.max = config.max.max(config.min);
        config.value = config.value.clamp(config.min, config.max);
        let current = state(hwnd);
        if current.is_null() {
            return;
        }
        if (*current).config.axis != config.axis || (*current).config.dpi != config.dpi {
            cancel(hwnd);
        }
        let current = state(hwnd);
        if !current.is_null() {
            (*current).config = config;
        }
    }
}
pub unsafe fn feedback(hwnd: HWND) -> Feedback {
    unsafe {
        let state = state(hwnd);
        if state.is_null() {
            Feedback::default()
        } else {
            Feedback {
                hovered: (*state).hovered,
                dragging: (*state).drag.is_some(),
            }
        }
    }
}
pub unsafe fn interacting(hwnd: HWND) -> bool {
    unsafe { feedback(hwnd).dragging }
}
pub unsafe fn cancel(hwnd: HWND) {
    unsafe { finish(hwnd, false) }
}

unsafe fn finish(hwnd: HWND, hover: bool) {
    unsafe {
        let state = state(hwnd);
        if state.is_null() {
            return;
        }
        let ended = (*state).drag.take().is_some();
        let changed = (*state).hovered != hover;
        (*state).hovered = hover;
        let notify = (*state).notify;
        if GetCapture() == hwnd {
            ReleaseCapture();
            // Capture loss is synchronous and clears hover in the nested call.
            // A normal release over the bar should retain its hover feedback.
            set_hover(hwnd, hover);
        }
        if ended || changed {
            InvalidateRect(hwnd, null(), 0);
        }
        if ended {
            notify(hwnd, Event::End);
        }
    }
}
unsafe fn set_hover(hwnd: HWND, hover: bool) {
    unsafe {
        let state = state(hwnd);
        if !state.is_null() && (*state).hovered != hover {
            (*state).hovered = hover;
            InvalidateRect(hwnd, null(), 0);
        }
    }
}
unsafe fn render(hwnd: HWND, dc: HDC) {
    unsafe {
        let state = state(hwnd);
        if !state.is_null() {
            // Paint callbacks only draw; pass a copied snapshot rather than
            // exposing mutable input state to owner rendering code.
            let paint = (*state).paint;
            let axis = (*state).config.axis;
            let feedback = feedback(hwnd);
            paint(hwnd, dc, axis, feedback);
        }
    }
}

/// Draw the common separator over the owner's already complete background.
pub unsafe fn paint(dc: HDC, area: RECT, theme: &Theme, axis: Axis, feedback: Feedback) {
    unsafe {
        let active = feedback.hovered || feedback.dragging;
        let radius = if active { theme.px(1) } else { 0 };
        let line = match axis {
            Axis::Horizontal => {
                let center = (area.left + area.right - 1) / 2;
                rect(center - radius, area.top, center + radius + 1, area.bottom)
            }
            Axis::Vertical => {
                let center = (area.top + area.bottom - 1) / 2;
                rect(area.left, center - radius, area.right, center + radius + 1)
            }
        };
        theme::fill(
            dc,
            line,
            if active {
                theme.colors.blue
            } else {
                theme.colors.border
            },
        );
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                ui::paint_window(hwnd, |dc| render(hwnd, dc));
                return 0;
            }
            WM_PRINT => return DefWindowProcW(hwnd, msg, wp, lp),
            WM_PRINTCLIENT => {
                let saved = SaveDC(wp as HDC);
                render(hwnd, wp as HDC);
                RestoreDC(wp as HDC, saved);
                return 0;
            }
            WM_SETCURSOR => {
                let current = state(hwnd);
                if !current.is_null() {
                    SetCursor(LoadCursorW(null_mut(), (*current).config.axis.cursor()));
                    return 1;
                }
            }
            WM_LBUTTONDOWN | WM_MOUSEMOVE | WM_LBUTTONUP => {
                let current = state(hwnd);
                if current.is_null() {
                    return 0;
                }
                let mut area = RECT::default();
                GetClientRect(hwnd, &mut area);
                let mut point = POINT {
                    x: lp as i16 as i32,
                    y: (lp >> 16) as i16 as i32,
                };
                let inside = PtInRect(&area, point) != 0;
                set_hover(hwnd, inside);
                ClientToScreen(hwnd, &mut point);
                let config = (*current).config;
                let notify = (*current).notify;
                let pointer = config.axis.coordinate(point);
                match msg {
                    WM_LBUTTONDOWN => {
                        (*current).drag = Some(Drag {
                            config,
                            pointer,
                            last: config.value,
                        });
                        InvalidateRect(hwnd, null(), 0);
                        SetCapture(hwnd);
                        notify(hwnd, Event::Begin);
                    }
                    WM_MOUSEMOVE => {
                        if let Some(drag) = (*current).drag {
                            SetCursor(LoadCursorW(null_mut(), config.axis.cursor()));
                            let value = drag.value(pointer);
                            if value != drag.last {
                                (*current).drag.as_mut().unwrap().last = value;
                                notify(hwnd, Event::Resize(value));
                            }
                        }
                        if IsWindow(hwnd) != 0 {
                            let mut track = TRACKMOUSEEVENT {
                                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                                dwFlags: TME_LEAVE,
                                hwndTrack: hwnd,
                                dwHoverTime: 0,
                            };
                            TrackMouseEvent(&mut track);
                        }
                    }
                    WM_LBUTTONUP => finish(hwnd, inside),
                    _ => {}
                }
                return 0;
            }
            WM_MOUSELEAVE => {
                set_hover(hwnd, false);
                return 0;
            }
            WM_CAPTURECHANGED | WM_CANCELMODE => {
                cancel(hwnd);
                return 0;
            }
            WM_SHOWWINDOW if wp == 0 => {
                cancel(hwnd);
            }
            WM_KEYDOWN if wp == VK_ESCAPE as usize => {
                cancel(hwnd);
                return 0;
            }
            WM_NCDESTROY => {
                cancel(hwnd);
                let state = RemovePropW(hwnd, STATE) as *mut State;
                RemoveWindowSubclass(hwnd, Some(procedure), id);
                ui::release_buffer(hwnd);
                if !state.is_null() {
                    drop(Box::from_raw(state));
                }
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drag_uses_screen_anchor_dpi_and_limits() {
        for axis in [Axis::Horizontal, Axis::Vertical] {
            let drag = Drag {
                config: Config {
                    axis,
                    value: 240,
                    min: 150,
                    max: 600,
                    dpi: 144,
                },
                pointer: -300,
                last: 240,
            };
            assert_eq!(drag.value(-300), 240);
            assert_eq!(drag.value(-255), 270);
            assert_eq!(drag.value(-345), 210);
            assert_eq!(drag.value(-10000), 150);
            assert_eq!(drag.value(10000), 600);
            let point = POINT { x: -255, y: -345 };
            assert_eq!(
                drag.value(axis.coordinate(point)),
                if axis == Axis::Horizontal { 270 } else { 210 }
            );
        }
    }
}
