//! Exclusive Space ownership for the lifetime of the returned guard.
#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::StoreLock;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::StoreLock;

#[cfg(test)]
mod tests {
    use super::StoreLock;
    use std::{fs, path::Path, process::Command};

    const PROBE_PATH: &str = "EDOT_TEST_STORE_LOCK_PATH";
    const PROBE_EXPECTED: &str = "EDOT_TEST_STORE_LOCK_EXPECTED";

    fn probe(path: &Path, expected: bool) {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "store_lock::tests::child_process_probe",
                "--nocapture",
            ])
            .env(PROBE_PATH, path)
            .env(
                PROBE_EXPECTED,
                if expected { "acquired" } else { "blocked" },
            )
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    #[test]
    fn child_process_probe() {
        let Some(path) = std::env::var_os(PROBE_PATH) else {
            return;
        };
        let result = StoreLock::acquire(Path::new(&path));
        assert_eq!(
            result.is_ok(),
            std::env::var(PROBE_EXPECTED).unwrap() == "acquired"
        );
        // Exit without running the guard's destructor, as on abrupt shutdown.
        std::process::exit(0);
    }

    #[test]
    fn excludes_aliases_and_processes_then_releases_on_drop_or_exit() {
        let root = std::env::temp_dir().join(format!(
            "edot-store-lock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = root.join("한글 Space");
        let second = root.join("other");
        let lock = StoreLock::acquire(&first).unwrap();
        fs::write(first.join("memo.txt"), "keep me").unwrap();
        assert!(StoreLock::acquire(&first).is_err());
        assert!(StoreLock::acquire(&first.join(".")).is_err());
        let other_lock = StoreLock::acquire(&second).unwrap();
        probe(&first, false);
        probe(&second, false);
        drop(lock);
        probe(&first, true);
        let _lock = StoreLock::acquire(&first).unwrap();
        assert_eq!(
            fs::read_to_string(first.join("memo.txt")).unwrap(),
            "keep me"
        );
        drop(other_lock);
        probe(&second, true);
    }

    #[cfg(unix)]
    #[test]
    fn unix_uses_one_persistent_file_for_symlink_aliases_and_distinguishes_case() {
        use std::os::unix::fs::{MetadataExt, symlink};

        let root = std::env::temp_dir().join(format!(
            "edot-store-lock-unix-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let upper = root.join("Space");
        let lower = root.join("space");
        let alias = root.join("alias");
        let lock = StoreLock::acquire(&upper).unwrap();
        symlink(&upper, &alias).unwrap();
        assert!(StoreLock::acquire(&alias).is_err());
        probe(&alias, false);
        if !lower.exists() {
            let _other = StoreLock::acquire(&lower).unwrap();
        }
        let file = upper.join(".edot/.store.lock");
        let inode = fs::metadata(&file).unwrap().ino();
        drop(lock);
        assert_eq!(fs::metadata(&file).unwrap().ino(), inode);
        let _lock = StoreLock::acquire(&alias).unwrap();
        assert_eq!(fs::metadata(&file).unwrap().ino(), inode);
    }
}
