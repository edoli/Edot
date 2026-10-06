use std::ptr::null_mut;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;

pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}
pub const WHITE: COLORREF = rgb(255, 255, 255);
pub const SIDEBAR: COLORREF = rgb(245, 247, 250);
pub const BAR: COLORREF = rgb(241, 244, 248);
pub const BORDER: COLORREF = rgb(220, 226, 234);
pub const INK: COLORREF = rgb(31, 41, 55);
pub const MUTED: COLORREF = rgb(107, 119, 137);
pub const BLUE: COLORREF = rgb(38, 116, 245);
pub const SELECTED: COLORREF = rgb(221, 235, 255);
pub const HOVER: COLORREF = rgb(232, 237, 244);

#[derive(Clone, Copy)]
pub struct Colors {
    pub white: COLORREF,
    pub sidebar: COLORREF,
    pub bar: COLORREF,
    pub border: COLORREF,
    pub ink: COLORREF,
    pub muted: COLORREF,
    pub blue: COLORREF,
    pub selected: COLORREF,
    pub hover: COLORREF,
}
impl Colors {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                white: rgb(30, 33, 39),
                sidebar: rgb(24, 27, 33),
                bar: rgb(21, 24, 29),
                border: rgb(62, 68, 80),
                ink: rgb(229, 233, 240),
                muted: rgb(159, 171, 190),
                blue: rgb(107, 164, 255),
                selected: rgb(43, 65, 98),
                hover: rgb(45, 50, 60),
            }
        } else {
            Self {
                white: WHITE,
                sidebar: SIDEBAR,
                bar: BAR,
                border: BORDER,
                ink: INK,
                muted: MUTED,
                blue: BLUE,
                selected: SELECTED,
                hover: HOVER,
            }
        }
    }
}

pub struct Theme {
    pub dpi: u32,
    pub dark: bool,
    pub colors: Colors,
    pub ui: HFONT,
    pub tab: HFONT,
    pub editor: HFONT,
    pub small: HFONT,
    pub heading: HFONT,
    pub white_brush: HBRUSH,
    pub sidebar_brush: HBRUSH,
}

impl Theme {
    pub unsafe fn new(dpi: u32, dark: bool, editor_font: &str, editor_size: u32) -> Self {
        let dpi = dpi.max(96);
        let colors = Colors::new(dark);
        let font = |name: &str, size: i32, weight: i32| {
            let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            unsafe {
                CreateFontW(
                    -(size * dpi as i32 / 96),
                    0,
                    0,
                    0,
                    weight,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    DEFAULT_PITCH as u32,
                    name.as_ptr(),
                )
            }
        };
        Self {
            dpi,
            dark,
            colors,
            ui: font("Segoe UI", 15, 400),
            tab: font("Segoe UI", 13, 400),
            editor: font(editor_font, editor_size.clamp(8, 72) as i32, 400),
            small: font("Segoe UI", 13, 400),
            heading: font("Segoe UI", 21, 600),
            white_brush: unsafe { CreateSolidBrush(colors.white) },
            sidebar_brush: unsafe { CreateSolidBrush(colors.sidebar) },
        }
    }
    pub fn px(&self, value: i32) -> i32 {
        (value * self.dpi as i32 + 48) / 96
    }
}

impl Drop for Theme {
    fn drop(&mut self) {
        unsafe {
            for object in [
                self.ui,
                self.tab,
                self.editor,
                self.small,
                self.heading,
                self.white_brush,
                self.sidebar_brush,
            ] {
                DeleteObject(object);
            }
        }
    }
}

pub fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

pub unsafe fn fill(dc: HDC, area: RECT, color: COLORREF) {
    unsafe {
        let brush = GetStockObject(DC_BRUSH);
        let previous = SetDCBrushColor(dc, color);
        FillRect(dc, &area, brush);
        SetDCBrushColor(dc, previous);
    }
}

pub unsafe fn rounded(dc: HDC, area: RECT, color: COLORREF, border: COLORREF, radius: i32) {
    unsafe {
        let saved = SaveDC(dc);
        let brush = GetStockObject(DC_BRUSH);
        let pen = GetStockObject(DC_PEN);
        SetDCBrushColor(dc, color);
        SetDCPenColor(dc, border);
        SelectObject(dc, brush);
        SelectObject(dc, pen);
        RoundRect(
            dc,
            area.left,
            area.top,
            area.right,
            area.bottom,
            radius,
            radius,
        );
        RestoreDC(dc, saved);
    }
}

pub unsafe fn text(dc: HDC, value: &str, mut area: RECT, font: HFONT, color: COLORREF, align: u32) {
    if value.is_empty() {
        return;
    }
    let value: Vec<u16> = value.encode_utf16().collect();
    unsafe {
        let saved = SaveDC(dc);
        SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        DrawTextW(
            dc,
            value.as_ptr(),
            value.len() as i32,
            &mut area,
            DT_NOPREFIX
                | DT_END_ELLIPSIS
                | align
                | if align & DT_WORDBREAK == 0 {
                    DT_SINGLELINE | DT_VCENTER
                } else {
                    0
                },
        );
        RestoreDC(dc, saved);
    }
}

pub unsafe fn line(dc: HDC, points: &[(i32, i32)], color: COLORREF, width: i32) {
    if points.is_empty() {
        return;
    }
    unsafe {
        let pen = CreatePen(PS_SOLID, width.max(1), color);
        let old = SelectObject(dc, pen);
        MoveToEx(dc, points[0].0, points[0].1, null_mut());
        for &(x, y) in &points[1..] {
            LineTo(dc, x, y);
        }
        SelectObject(dc, old);
        DeleteObject(pen);
    }
}

pub unsafe fn document(dc: HDC, x: i32, y: i32, size: i32, color: COLORREF) {
    let u = |v: i32| v * size / 18;
    unsafe {
        line(
            dc,
            &[
                (x + u(3), y + u(1)),
                (x + u(11), y + u(1)),
                (x + u(15), y + u(5)),
                (x + u(15), y + u(17)),
                (x + u(3), y + u(17)),
                (x + u(3), y + u(1)),
            ],
            color,
            (size / 12).max(1),
        );
        line(
            dc,
            &[
                (x + u(10), y + u(1)),
                (x + u(10), y + u(6)),
                (x + u(15), y + u(6)),
            ],
            color,
            (size / 12).max(1),
        );
        line(
            dc,
            &[(x + u(6), y + u(10)), (x + u(12), y + u(10))],
            color,
            1,
        );
        line(
            dc,
            &[(x + u(6), y + u(13)), (x + u(10), y + u(13))],
            color,
            1,
        );
    }
}

pub unsafe fn folder(dc: HDC, x: i32, y: i32, size: i32) {
    let u = |v: i32| v * size / 18;
    unsafe {
        rounded(
            dc,
            rect(x, y + u(3), x + u(8), y + u(9)),
            rgb(176, 210, 255),
            BLUE,
            u(3),
        );
        rounded(
            dc,
            rect(x, y + u(6), x + u(18), y + u(17)),
            rgb(146, 192, 252),
            BLUE,
            u(3),
        );
    }
}

pub unsafe fn close(dc: HDC, x: i32, y: i32, size: i32, color: COLORREF) {
    unsafe {
        line(
            dc,
            &[(x, y), (x + size, y + size)],
            color,
            (size / 6).max(1),
        );
        line(
            dc,
            &[(x + size, y), (x, y + size)],
            color,
            (size / 6).max(1),
        );
    }
}

pub unsafe fn plus(dc: HDC, x: i32, y: i32, size: i32, color: COLORREF) {
    unsafe {
        line(
            dc,
            &[(x, y + size / 2), (x + size, y + size / 2)],
            color,
            (size / 8).max(1),
        );
        line(
            dc,
            &[(x + size / 2, y), (x + size / 2, y + size)],
            color,
            (size / 8).max(1),
        );
    }
}

pub unsafe fn gear(dc: HDC, x: i32, y: i32, size: i32, color: COLORREF) {
    let cx = x + size / 2;
    let cy = y + size / 2;
    let radius = size as f64 / 2.0;
    let mut points = [(0, 0); 33];
    for (i, point) in points.iter_mut().enumerate() {
        let angle = i as f64 * std::f64::consts::TAU / 32.0;
        let r = radius * if i % 4 == 1 || i % 4 == 2 { 1.0 } else { 0.73 };
        *point = (
            cx + (r * angle.cos()).round() as i32,
            cy + (r * angle.sin()).round() as i32,
        );
    }
    unsafe {
        line(dc, &points, color, (size / 12).max(1));
        let saved = SaveDC(dc);
        SelectObject(dc, GetStockObject(NULL_BRUSH));
        SelectObject(dc, GetStockObject(DC_PEN));
        SetDCPenColor(dc, color);
        let r = (size / 5).max(2);
        Ellipse(dc, cx - r, cy - r, cx + r + 1, cy + r + 1);
        RestoreDC(dc, saved);
    }
}
