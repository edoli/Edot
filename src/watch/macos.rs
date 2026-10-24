//! FSEvents semantics, kept separate from the native lifetime/FFI adapter.
use super::Changes;
use std::{io, path::Path};

#[cfg(target_os = "macos")]
mod native;
#[cfg(target_os = "macos")]
pub use native::Watcher;

const LOST_EVENTS: u32 = 0x01 | 0x02 | 0x04 | 0x08;
const ROOT_UNAVAILABLE: u32 = 0x20 | 0x80;
const STRUCTURAL: u32 = 0x100 | 0x200 | 0x800;
const RENAMED: u32 = 0x800;
const IS_DIRECTORY: u32 = 0x20000;
// ItemCreated..ItemXattrMod, plus ItemCloned (clonefile can create a note).
const ITEM_CHANGED: u32 = 0xff00 | 0x400000;

fn classify(root: &Path, path: &Path, flags: u32) -> io::Result<Changes> {
    if flags & ROOT_UNAVAILABLE != 0 {
        return Err(io::Error::other(
            "Space 경로가 이동·삭제되거나 볼륨이 해제되었습니다",
        ));
    }
    if flags & LOST_EVENTS != 0 {
        return Ok(Changes::all());
    }
    let mut changes = Changes::default();
    let Ok(relative) = path.strip_prefix(root) else {
        return Ok(changes);
    };
    if flags & ITEM_CHANGED == 0 {
        return Ok(changes);
    }
    if flags & IS_DIRECTORY != 0 {
        if (relative == Path::new("notes") || relative == Path::new(".edot"))
            && flags & STRUCTURAL != 0
        {
            changes.rescan = true;
        }
    } else if flags & RENAMED != 0 && relative.parent() == Some(Path::new("notes")) {
        // FSEvents rename batches need not pair source and destination; reconcile both.
        changes.rescan = true;
    } else if let Some(relative) = relative.to_str() {
        changes.relative_path(relative);
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fsevents_filter_paths_coalescing_renames_and_invalidated_roots() {
        let root = Path::new("/space");
        let event = |path: &str, flags| classify(root, &root.join(path), flags).unwrap();
        assert!(event("notes/7.txt", 0x1000).notes.contains(&7));
        assert!(event("notes/7.txt", 0x400000).notes.contains(&7));
        assert!(event("notes/7.txt", 0x200).notes.contains(&7));
        assert!(event(".edot/tree.tsv", 0x1000).tree);
        for path in [
            ".edot/session.bin",
            "notes/7.tmp",
            "notes/7.TXT",
            "Notes/7.txt",
            "trash/7.txt",
            ".EDOT/tree.tsv",
        ] {
            assert!(event(path, 0x1000).is_empty(), "{path}");
        }
        assert!(event("notes", IS_DIRECTORY | 0x1000).is_empty());
        assert!(event("notes", IS_DIRECTORY | RENAMED).rescan);
        assert!(event("notes/7.tmp", RENAMED).rescan);
        assert!(event(".edot/tree.tsv", RENAMED).tree);
        assert!(event("unrelated", LOST_EVENTS).rescan);
        assert!(
            classify(root, Path::new("/other/notes/7.txt"), 0x1000)
                .unwrap()
                .is_empty()
        );
        assert!(classify(root, root, ROOT_UNAVAILABLE).is_err());
    }
}
