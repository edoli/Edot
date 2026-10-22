use std::path::Path;

pub fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    same_existing_path(left, right).unwrap_or_else(|| same_unresolved_path(left, right))
}

#[cfg(unix)]
fn same_existing_path(left: &Path, right: &Path) -> Option<bool> {
    use std::os::unix::fs::MetadataExt;

    let left = std::fs::metadata(left).ok()?;
    let right = std::fs::metadata(right).ok()?;
    Some(left.dev() == right.dev() && left.ino() == right.ino())
}

#[cfg(windows)]
fn same_existing_path(left: &Path, right: &Path) -> Option<bool> {
    use std::{
        fs::{File, OpenOptions},
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, GetFileInformationByHandle,
    };

    fn identity(path: &Path) -> Option<(File, [u32; 3])> {
        let file = OpenOptions::new()
            .access_mode(0)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .ok()?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // Both handles stay open until comparison finishes, preventing file-ID
        // reuse if a path is removed while the other handle is being opened.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return None;
        }
        Some((
            file,
            [
                info.dwVolumeSerialNumber,
                info.nFileIndexHigh,
                info.nFileIndexLow,
            ],
        ))
    }

    let (_left_handle, left) = identity(left)?;
    let (_right_handle, right) = identity(right)?;
    Some(left == right)
}

#[cfg(not(any(windows, unix)))]
fn same_existing_path(left: &Path, right: &Path) -> Option<bool> {
    Some(std::fs::canonicalize(left).ok()? == std::fs::canonicalize(right).ok()?)
}

#[cfg(not(windows))]
fn same_unresolved_path(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(windows)]
fn same_unresolved_path(left: &Path, right: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

    let left: Vec<u16> = left.as_os_str().encode_wide().collect();
    let right: Vec<u16> = right.as_os_str().encode_wide().collect();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return false;
    };
    if left.is_empty() || right.is_empty() {
        return left == right;
    }
    // Explicit UTF-16 lengths preserve non-Unicode paths and avoid locale or
    // replacement-character folding when filesystem identity is unavailable.
    unsafe {
        CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) == CSTR_EQUAL
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "edot-path-eq-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn existing_aliases_match_but_separate_objects_do_not() {
        let root = fixture();
        let folder = root.join("한글 Space");
        let other = root.join("other");
        fs::create_dir(&folder).unwrap();
        fs::create_dir(&other).unwrap();
        assert!(same_path(&folder, &folder.join(".")));
        assert!(same_path(&folder, &other.join("../한글 Space")));
        assert!(same_path(&folder, &fs::canonicalize(&folder).unwrap()));
        assert!(!same_path(&folder, &other));
        let first = root.join("first.txt");
        let alias = root.join("alias.txt");
        fs::write(&first, "shared").unwrap();
        fs::hard_link(&first, &alias).unwrap();
        assert!(same_path(&first, &alias));
    }

    #[test]
    fn unresolved_case_policy_is_native_and_does_not_expand_unicode() {
        let root = fixture();
        let upper = root.join("Missing-Ä");
        let lower = root.join("missing-ä");
        assert_eq!(same_path(&upper, &lower), cfg!(windows));
        assert!(same_path(&upper, &upper));
        assert!(!same_path(&root.join("Straße"), &root.join("Strasse")));
    }

    #[cfg(unix)]
    #[test]
    fn unix_preserves_case_and_non_utf8_names_and_follows_symlinks() {
        use std::{
            ffi::OsString,
            os::unix::{ffi::OsStringExt, fs::symlink},
        };

        let root = fixture();
        let upper = root.join("Space");
        let lower = root.join("space");
        fs::create_dir(&upper).unwrap();
        // This works on both case-sensitive and case-insensitive macOS volumes.
        if lower.exists() {
            assert!(same_path(&upper, &lower));
        } else {
            fs::create_dir(&lower).unwrap();
            assert!(!same_path(&upper, &lower));
            let config = root.join("config");
            super::super::initialize_metadata(&upper).unwrap();
            super::super::initialize_metadata(&lower).unwrap();
            super::super::Registry::register(&config, &upper).unwrap();
            let registry = super::super::Registry::register(&config, &lower).unwrap();
            assert_eq!(registry.entries.len(), 2);
            let registry = super::super::Registry::remove(&config, &upper).unwrap();
            assert_eq!(registry.entries.len(), 1);
            assert_eq!(registry.entries[0].path, fs::canonicalize(&lower).unwrap());
        }
        let alias = root.join("alias");
        symlink(&upper, &alias).unwrap();
        assert!(same_path(&upper, &alias));
        let first = root.join(OsString::from_vec(vec![b'n', 0xff]));
        let second = root.join(OsString::from_vec(vec![b'n', 0xfe]));
        assert!(!same_path(&first, &second));
        assert!(same_path(&first, &first));
    }

    #[cfg(windows)]
    #[test]
    fn windows_preserves_utf16_names_and_existing_case_aliases() {
        use std::{ffi::OsString, os::windows::ffi::OsStringExt};

        let root = fixture();
        let folder = root.join("MixedCase-Ä");
        fs::create_dir(&folder).unwrap();
        assert!(same_path(&folder, &root.join("mixedcase-ä")));
        let first = root.join(OsString::from_wide(&[0xd800]));
        let second = root.join(OsString::from_wide(&[0xd801]));
        assert!(!same_path(&first, &second));
        assert!(same_path(&first, &first));
    }
}
