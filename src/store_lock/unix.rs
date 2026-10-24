use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::Path,
};

pub struct StoreLock {
    _file: File,
}

impl StoreLock {
    pub fn acquire(dir: &Path) -> io::Result<Self> {
        let metadata = dir.join(".edot");
        fs::create_dir_all(&metadata)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(metadata.join(".store.lock"))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(io::Error::other(
                "같은 노트 저장소가 이미 다른 창에서 열려 있습니다.",
            )),
            Err(TryLockError::Error(error)) => Err(error),
        }
        // Keep the file on disk: removing it lets another process lock a new
        // inode while an existing process still owns the previous one.
        // Closing the guard's file releases ownership, including on process exit.
    }
}
