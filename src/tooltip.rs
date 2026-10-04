//! A single native tooltip, registered only for the item under the pointer.
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::HFONT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::*;
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
    text: Vec<u16>,
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
            Self {
                hwnd,
                current: None,
                text: Vec::new(),
            }
        }
    }
    pub unsafe fn style(&self, font: HFONT, max_width: i32) {
        unsafe {
            SendMessageW(self.hwnd, WM_SETFONT, font as usize, 0);
            SendMessageW(self.hwnd, TTM_SETMAXTIPWIDTH, 0, max_width as isize);
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
                    .text
                    .iter()
                    .copied()
                    .eq(title.encode_utf16().chain(Some(0)))
            {
                self.hide();
                self.text = title.encode_utf16().chain(Some(0)).collect();
                let info = TTTOOLINFOW {
                    cbSize: std::mem::offset_of!(TTTOOLINFOW, lpReserved) as u32,
                    uFlags: TTF_TRANSPARENT,
                    hwnd: owner,
                    uId: 1,
                    rect: bounds,
                    lpszText: self.text.as_mut_ptr(),
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
impl Drop for Tooltip {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
        }
    }
}
