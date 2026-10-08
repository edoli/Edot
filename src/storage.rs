//! File replacement shared by note, session, settings and bulk-search persistence.
use std::{
    fs,
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

pub fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

/// Publish a complete config from a unique temporary file in the same directory.
pub fn replace_config_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("설정 경로가 잘못되었습니다."))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".edot-config-{}-{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::File::create_new(&tmp)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
