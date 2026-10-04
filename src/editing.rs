//! Messages and layouts from the Windows SDK Richedit.h (system Msftedit.dll).
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_USER};
pub const EM_EXGETSEL: u32 = WM_USER + 52;
#[cfg(test)]
pub const EM_EXLINEFROMCHAR: u32 = WM_USER + 54;
pub const EM_EXSETSEL: u32 = WM_USER + 55;
pub const EM_SETBKGNDCOLOR: u32 = WM_USER + 67;
pub const EM_SETCHARFORMAT: u32 = WM_USER + 68;
pub const EM_SETTARGETDEVICE: u32 = WM_USER + 72;
pub const EM_GETSCROLLPOS: u32 = WM_USER + 221;
pub const EM_SETSCROLLPOS: u32 = WM_USER + 222;
pub const EM_SETEVENTMASK: u32 = WM_USER + 69;
pub const EM_REDO: u32 = WM_USER + 84;
pub const EM_STOPGROUPTYPING: u32 = WM_USER + 88;
pub const EM_SETTEXTMODE: u32 = WM_USER + 89;
pub const EM_FINDTEXTEXW: u32 = WM_USER + 124;
pub const EN_SELCHANGE: u32 = 0x0702;
pub const ENM_CHANGE: isize = 1;
pub const ENM_SELCHANGE: isize = 0x00080000;

#[repr(C)]
#[derive(Default)]
struct CharFormat {
    size: u32,
    mask: u32,
    effects: u32,
    height: i32,
    offset: i32,
    color: u32,
    charset: u8,
    pitch: u8,
    face: [u16; 32],
}

pub unsafe fn text_color(edit: HWND, color: u32) {
    let format = CharFormat {
        size: std::mem::size_of::<CharFormat>() as u32,
        mask: 0x40000000,
        color,
        ..Default::default()
    }; // CFM_COLOR; clear CFE_AUTOCOLOR
    unsafe {
        // Plain-text RichEdit has one format for the whole document. Changing
        // its default format also colors existing text without selecting it.
        SendMessageW(edit, EM_SETCHARFORMAT, 0, &format as *const _ as isize);
    }
}

#[cfg(test)]
pub unsafe fn format(edit: HWND) -> (String, u32) {
    let mut format = CharFormat {
        size: std::mem::size_of::<CharFormat>() as u32,
        ..Default::default()
    };
    unsafe {
        SendMessageW(edit, WM_USER + 58, 0, &mut format as *mut _ as isize);
    }
    let len = format
        .face
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(format.face.len());
    (String::from_utf16_lossy(&format.face[..len]), format.color)
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct CharRange {
    pub start: i32,
    pub end: i32,
}
#[repr(C)]
struct FindText {
    range: CharRange,
    text: *const u16,
    result: CharRange,
}
pub unsafe fn selection(edit: HWND) -> CharRange {
    let mut range = CharRange::default();
    unsafe {
        SendMessageW(edit, EM_EXGETSEL, 0, &mut range as *mut _ as isize);
    }
    range
}
pub unsafe fn select(edit: HWND, range: CharRange) {
    unsafe {
        SendMessageW(edit, EM_EXSETSEL, 0, &range as *const _ as isize);
    }
}
pub unsafe fn find(
    edit: HWND,
    query: &str,
    start: i32,
    end: i32,
    backwards: bool,
    case: bool,
) -> Option<CharRange> {
    if query.is_empty() {
        return None;
    }
    let query: Vec<u16> = query.encode_utf16().chain(Some(0)).collect();
    let mut request = FindText {
        range: CharRange { start, end },
        text: query.as_ptr(),
        result: CharRange::default(),
    };
    let flags = if backwards { 0 } else { 1 } | if case { 4 } else { 0 }; // FR_DOWN, FR_MATCHCASE
    let found =
        unsafe { SendMessageW(edit, EM_FINDTEXTEXW, flags, &mut request as *mut _ as isize) };
    (found >= 0).then_some(request.result)
}

/// RichEdit search offsets count UTF-16 code units and one CR per paragraph.
pub fn replace_ranges(text: &str, ranges: &[CharRange], replacement: &str) -> String {
    let mut units: Vec<u16> = text
        .replace("\r\n", "\r")
        .replace('\n', "\r")
        .encode_utf16()
        .collect();
    let replacement: Vec<u16> = replacement.encode_utf16().collect();
    for range in ranges.iter().rev() {
        units.splice(
            range.start as usize..range.end as usize,
            replacement.iter().copied(),
        );
    }
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_preserves_unicode_and_paragraph_offsets() {
        let text = "🙂 가나다\r\nABC 가나다";
        let ranges = [
            CharRange { start: 3, end: 6 },
            CharRange { start: 11, end: 14 },
        ];
        assert_eq!(replace_ranges(text, &ranges, "끝"), "🙂 끝\rABC 끝");
        assert_eq!(replace_ranges(text, &ranges, ""), "🙂 \rABC ");
    }
}
