//! File replacement shared by note, session, settings and bulk-search persistence.
use std::{fs, io, path::Path};

pub fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}
