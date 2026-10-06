//! Native tooltip timing and positioning with theme-aware rounded GDI rendering.
use crate::{
    theme::{self, Theme},
    ui,
};
use std::cell::RefCell;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Tool {
    owner: HWND,
    id: u64,
    bounds: [i32; 4],
}
pub struct Tooltip {
    hwnd: HWND,
    current: Option<Tool>,
    appearance: Box<RefCell<Appearance>>,
}
struct Appearance {
    font: HFONT,
    dpi: u32,
    dark: bool,
    ink: COLORREF,
    text: Vec<u16>,
}
impl Appearance {
    fn px(&self, value: i32) -> i32 {
        (value * self.dpi as i32 + 48) / 96
    }
    fn background(&self) -> COLORREF {
        if self.dark {
            theme::rgb(43, 43, 43)
        } else {
            theme::rgb(249, 249, 249)
        }
    }
    fn border(&self) -> COLORREF {
        if self.dark {
            theme::rgb(28, 28, 28)
        } else {
            theme::rgb(211, 211, 211)
        }
    }
}
impl Tooltip {
    pub unsafe fn new(owner: HWND) -> Self {
        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                TOOLTIPS_CLASSW,
                null(),
                WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
                0,
                0,
                0,
                0,
                owner,
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            SendMessageW(hwnd, TTM_SETDELAYTIME, TTDT_INITIAL as usize, 500);
            SendMessageW(hwnd, TTM_SETDELAYTIME, TTDT_RESHOW as usize, 100);
            SendMessageW(hwnd, TTM_SETDELAYTIME, TTDT_AUTOPOP as usize, 8000);
            let appearance = Box::new(RefCell::new(Appearance {
                font: GetStockObject(DEFAULT_GUI_FONT),
                dpi: 96,
                dark: false,
                ink: theme::INK,
                text: Vec::new(),
            }));
            SetWindowTheme(hwnd, windows_sys::w!(""), windows_sys::w!(""));
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            SetWindowLongPtrW(
                hwnd,
                GWL_STYLE,
                (style & !(WS_BORDER | WS_DLGFRAME)) as isize,
            );
            let class_style = GetClassLongPtrW(hwnd, GCL_STYLE) as u32;
            SetClassLongPtrW(hwnd, GCL_STYLE, (class_style | CS_DROPSHADOW) as isize);
            SetWindowSubclass(
                hwnd,
                Some(tooltip_proc),
                1,
                &*appearance as *const RefCell<Appearance> as usize,
            );
            Self {
                hwnd,
                current: None,
                appearance,
            }
        }
    }
    pub unsafe fn style(&self, theme: &Theme) {
        unsafe {
            {
                let mut appearance = self.appearance.borrow_mut();
                appearance.font = theme.ui;
                appearance.dpi = theme.dpi;
                appearance.dark = theme.dark;
                appearance.ink = theme.colors.ink;
            }
            self.dismiss();
            SendMessageW(self.hwnd, WM_SETFONT, theme.ui as usize, 0);
            SendMessageW(self.hwnd, TTM_SETMAXTIPWIDTH, 0, theme.px(420) as isize);
            let margin = theme::rect(theme.px(10), theme.px(7), theme.px(10), theme.px(7));
            SendMessageW(self.hwnd, TTM_SETMARGIN, 0, &margin as *const _ as isize);
            InvalidateRect(self.hwnd, null(), 0);
        }
    }
    pub unsafe fn dismiss(&self) {
        unsafe {
            SendMessageW(self.hwnd, TTM_POP, 0, 0);
        }
    }
    pub unsafe fn leave(&mut self, owner: HWND) {
        if self.current.is_some_and(|tool| tool.owner == owner) {
            unsafe {
                self.hide();
            }
        }
    }
    pub unsafe fn hide(&mut self) {
        unsafe {
            SendMessageW(self.hwnd, TTM_POP, 0, 0);
            if let Some(tool) = self.current.take() {
                let info = TTTOOLINFOW {
                    // V2 size also works without a Common Controls v6 manifest.
                    cbSize: std::mem::offset_of!(TTTOOLINFOW, lpReserved) as u32,
                    hwnd: tool.owner,
                    uId: 1,
                    ..Default::default()
                };
                SendMessageW(self.hwnd, TTM_DELTOOLW, 0, &info as *const _ as isize);
            }
        }
    }
    pub unsafe fn hover(&mut self, owner: HWND, id: u64, bounds: RECT, title: &str, point: POINT) {
        let key = Tool {
            owner,
            id,
            bounds: [bounds.left, bounds.top, bounds.right, bounds.bottom],
        };
        unsafe {
            if self.current != Some(key)
                || !self
                    .appearance
                    .borrow()
                    .text
                    .iter()
                    .copied()
                    .eq(title.encode_utf16().chain(Some(0)))
            {
                self.hide();
                let text = {
                    let mut appearance = self.appearance.borrow_mut();
                    appearance.text = title.encode_utf16().chain(Some(0)).collect();
                    appearance.text.as_mut_ptr()
                };
                let info = TTTOOLINFOW {
                    cbSize: std::mem::offset_of!(TTTOOLINFOW, lpReserved) as u32,
                    uFlags: TTF_TRANSPARENT,
                    hwnd: owner,
                    uId: 1,
                    rect: bounds,
                    lpszText: text,
                    ..Default::default()
                };
                SendMessageW(self.hwnd, TTM_ADDTOOLW, 0, &info as *const _ as isize);
                self.current = Some(key);
            }
            let mut screen = POINT::default();
            GetCursorPos(&mut screen);
            let event = MSG {
                hwnd: owner,
                message: WM_MOUSEMOVE,
                pt: screen,
                lParam: ((point.y as u32 & 0xffff) << 16 | (point.x as u32 & 0xffff)) as isize,
                time: GetMessageTime() as u32,
                ..Default::default()
            };
            SendMessageW(self.hwnd, TTM_RELAYEVENT, 0, &event as *const _ as isize);
        }
    }
}
unsafe fn paint(dc: HDC, area: RECT, appearance: &Appearance) {
    unsafe {
        let saved = SaveDC(dc);
        theme::rounded(
            dc,
            area,
            appearance.background(),
            appearance.border(),
            appearance.px(8),
        );
        let mut text_area = theme::rect(
            area.left + appearance.px(10),
            area.top + appearance.px(7),
            area.right - appearance.px(10),
            area.bottom - appearance.px(7),
        );
        SelectObject(dc, appearance.font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, appearance.ink);
        let len = appearance.text.len().saturating_sub(1);
        DrawTextW(
            dc,
            appearance.text.as_ptr(),
            len as i32,
            &mut text_area,
            DT_WORDBREAK | DT_NOPREFIX,
        );
        RestoreDC(dc, saved);
    }
}
unsafe extern "system" fn tooltip_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    reference: usize,
) -> LRESULT {
    unsafe {
        let appearance = &*(reference as *const RefCell<Appearance>);
        match msg {
            WM_ERASEBKGND => return 1,
            WM_NCPAINT | WM_NCCALCSIZE => return 0,
            WM_PAINT => {
                ui::paint_window(hwnd, |dc| {
                    let mut area = RECT::default();
                    GetClientRect(hwnd, &mut area);
                    paint(dc, area, &appearance.borrow());
                });
                return 0;
            }
            WM_PRINTCLIENT => {
                let mut area = RECT::default();
                GetClientRect(hwnd, &mut area);
                paint(wp as HDC, area, &appearance.borrow());
                return 0;
            }
            WM_WINDOWPOSCHANGED => {
                let result = DefSubclassProc(hwnd, msg, wp, lp);
                let mut area = RECT::default();
                GetWindowRect(hwnd, &mut area);
                let radius = appearance.borrow().px(8);
                let region = CreateRoundRectRgn(
                    0,
                    0,
                    area.right - area.left + 1,
                    area.bottom - area.top + 1,
                    radius,
                    radius,
                );
                let previous = CreateRectRgn(0, 0, 0, 0);
                let same = GetWindowRgn(hwnd, previous) > 0 && EqualRgn(region, previous) != 0;
                DeleteObject(previous);
                if same || SetWindowRgn(hwnd, region, 0) == 0 {
                    DeleteObject(region);
                }
                return result;
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(tooltip_proc), id);
                ui::release_buffer(hwnd);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
impl Drop for Tooltip {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetCapture, GetFocus};

    #[test]
    fn tooltips_render_both_themes_without_activation() {
        unsafe {
            let controls = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_BAR_CLASSES,
            };
            InitCommonControlsEx(&controls);
            let focus = GetFocus();
            let capture = GetCapture();
            let owner = CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                null(),
                0,
                0,
                0,
                640,
                480,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            );
            let mut tooltip = Tooltip::new(owner);
            assert!(!tooltip.hwnd.is_null());
            assert_eq!(
                GetWindowLongPtrW(tooltip.hwnd, GWL_STYLE) as u32 & WS_BORDER,
                0
            );
            assert_ne!(
                GetWindowLongPtrW(tooltip.hwnd, GWL_EXSTYLE) as u32 & WS_EX_NOACTIVATE,
                0
            );

            let width = 640;
            let height = 300;
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
            assert!(!dc.is_null() && !bitmap.is_null());
            let original = SelectObject(dc, bitmap);
            for (column, dark) in [false, true].into_iter().enumerate() {
                let theme = Theme::new(144, dark, "Consolas", 18);
                tooltip.style(&theme);
                assert_eq!(
                    SendMessageW(tooltip.hwnd, WM_GETFONT, 0, 0),
                    theme.ui as isize
                );
                let mut margin = RECT::default();
                SendMessageW(
                    tooltip.hwnd,
                    TTM_GETMARGIN,
                    0,
                    &mut margin as *mut _ as isize,
                );
                assert_eq!(
                    [margin.left, margin.top, margin.right, margin.bottom],
                    [15, 11, 15, 11]
                );
                let area = theme::rect(0, 0, 300, 200);
                tooltip.hover(owner, 1, area, "이전 제목", POINT::default());
                tooltip.hover(owner, 2, area, "쓰기 도구", POINT::default());
                assert_eq!(SendMessageW(tooltip.hwnd, TTM_GETTOOLCOUNT, 0, 0), 1);
                assert_eq!(IsWindowVisible(tooltip.hwnd), 0);
                assert_eq!(
                    tooltip.appearance.borrow().text,
                    "쓰기 도구"
                        .encode_utf16()
                        .chain(Some(0))
                        .collect::<Vec<_>>()
                );
                SetWindowPos(
                    tooltip.hwnd,
                    null_mut(),
                    0,
                    0,
                    180,
                    52,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let region = CreateRectRgn(0, 0, 0, 0);
                assert!(GetWindowRgn(tooltip.hwnd, region) > 0);
                assert_eq!(PtInRegion(region, 0, 0), 0);
                assert_ne!(PtInRegion(region, 90, 26), 0);
                DeleteObject(region);
                let x = column as i32 * 320;
                theme::fill(dc, theme::rect(x, 0, x + 320, height), theme.colors.white);
                let saved = SaveDC(dc);
                SetViewportOrgEx(dc, x + 24, 24, null_mut());
                SendMessageW(
                    tooltip.hwnd,
                    WM_PRINTCLIENT,
                    dc as usize,
                    PRF_CLIENT as isize,
                );
                assert_eq!(
                    GetPixel(dc, 90, 3),
                    tooltip.appearance.borrow().background()
                );
                assert_eq!(GetPixel(dc, 90, 0), tooltip.appearance.borrow().border());
                assert_eq!(
                    GetPixel(dc, 2, 26),
                    tooltip.appearance.borrow().background()
                );
                RestoreDC(dc, saved);
                tooltip.hover(
                    owner,
                    3,
                    area,
                    "아주 긴 메모 제목입니다.\n두 번째 줄도 여백 안에 표시됩니다.",
                    POINT::default(),
                );
                let mut layout = theme::rect(0, 0, 242, 0);
                let appearance = tooltip.appearance.borrow();
                let saved = SaveDC(dc);
                SelectObject(dc, appearance.font);
                DrawTextW(
                    dc,
                    appearance.text.as_ptr(),
                    appearance.text.len() as i32 - 1,
                    &mut layout,
                    DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
                );
                RestoreDC(dc, saved);
                let full_height = layout.bottom + appearance.px(7) * 2;
                drop(appearance);
                SetWindowPos(
                    tooltip.hwnd,
                    null_mut(),
                    0,
                    0,
                    272,
                    full_height,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let saved = SaveDC(dc);
                SetViewportOrgEx(dc, x + 24, 100, null_mut());
                SendMessageW(
                    tooltip.hwnd,
                    WM_PRINTCLIENT,
                    dc as usize,
                    PRF_CLIENT as isize,
                );
                RestoreDC(dc, saved);
                tooltip.leave(owner);
                assert_eq!(SendMessageW(tooltip.hwnd, TTM_GETTOOLCOUNT, 0, 0), 0);
            }
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
            bmp.extend_from_slice(&[0; 24]);
            bmp.extend_from_slice(bytes);
            let output =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tooltip-validation");
            std::fs::create_dir_all(&output).unwrap();
            std::fs::write(output.join("preview.bmp"), bmp).unwrap();
            SelectObject(dc, original);
            DeleteObject(bitmap);
            DeleteDC(dc);
            drop(tooltip);
            DestroyWindow(owner);
            assert_eq!(GetFocus(), focus);
            assert_eq!(GetCapture(), capture);
        }
    }
}
