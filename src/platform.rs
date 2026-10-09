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
