//! Windows services only; all controls and editing are rendered by GPUI.
use std::{fs, io, path::Path, ptr::null};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    System::Threading::CreateMutexW,
};

pub struct StoreLock(HANDLE);
impl StoreLock {
    pub fn acquire(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let canonical = fs::canonicalize(dir)?;
        let hash = canonical
            .to_string_lossy()
            .to_lowercase()
            .bytes()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
        let name: Vec<u16> = format!("Local\\Edot-{hash:016x}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let handle = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let lock = Self(handle);
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Err(io::Error::other(
                "같은 노트 저장소가 이미 다른 창에서 열려 있습니다.",
            ));
        }
        Ok(lock)
    }
}
impl Drop for StoreLock {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Suppress Windows' own caption tips while keeping native hit testing and clicks.
pub fn suppress_caption_tooltips(window: &gpui::Window) -> windows::core::Result<()> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{Foundation::HWND, UI::Shell::SetWindowSubclass};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return Ok(());
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Ok(());
    };
    // SAFETY: GPUI calls this on the owning UI thread. The callback has no owned
    // state and removes itself on WM_NCDESTROY, after GPUI's window lifetime ends.
    unsafe {
        SetWindowSubclass(
            HWND(handle.hwnd.get() as *mut _),
            Some(caption_tooltip_proc),
            0x45444f54,
            0,
        )
        .ok()
    }
}

unsafe extern "system" fn caption_tooltip_proc(
    hwnd: windows::Win32::Foundation::HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    subclass: usize,
    _: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::{
        Foundation::LRESULT,
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass},
            WindowsAndMessaging::{
                HTCLOSE, HTMAXBUTTON, HTMINBUTTON, WM_NCDESTROY, WM_NCMOUSEHOVER,
            },
        },
    };
    if message == WM_NCMOUSEHOVER && matches!(wparam.0 as u32, HTMINBUTTON | HTMAXBUTTON | HTCLOSE)
    {
        return LRESULT(0);
    }
    // SAFETY: Forward the original message unchanged; detach only this callback.
    unsafe {
        if message == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(caption_tooltip_proc), subclass);
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}
