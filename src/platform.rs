//! Native Windows caption behavior.
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
