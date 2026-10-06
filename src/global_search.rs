use crate::editing::{self, CharRange};
use crate::model::replace_file;
use std::{fs, io, path::PathBuf, ptr::null_mut};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Hidden until styled and laid out; reused when switching sidebar modes.
#[derive(Clone, Copy)]
pub struct Panel {
    pub host: HWND,
    pub find: HWND,
    pub replace: HWND,
    pub buttons: [HWND; 7], // case, previous, next, close, one, all, disclosure
}
pub fn message_y(replace: bool, narrow: bool) -> i32 {
    if !replace {
        82
    } else if narrow {
        204
    } else {
        170
    }
}

#[derive(Clone, Debug)]
pub struct ResultRow {
    pub id: u64,
    pub count: usize,
    pub line: usize,
    pub preview: String,
}

pub struct Job {
    pub query: String,
    pub case: bool,
    pub ids: Vec<u64>,
    pub next: usize,
    pub rows: Vec<ResultRow>,
    pub errors: usize,
}

/// One unshown, plain-text RichEdit shares the in-file search semantics.
pub struct Scanner(HWND);
impl Scanner {
    pub fn clear(&mut self) {
        unsafe {
            SetWindowTextW(self.0, windows_sys::w!(""));
        }
    }
    pub fn new() -> io::Result<Self> {
        unsafe {
            let edit = CreateWindowExW(
                0,
                windows_sys::w!("RICHEDIT50W"),
                windows_sys::w!(""),
                WS_POPUP | ES_MULTILINE as u32,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            );
            if edit.is_null() {
                return Err(io::Error::last_os_error());
            }
            SendMessageW(edit, editing::EM_SETTEXTMODE, 1, 0);
            SendMessageW(edit, WM_USER + 53, 0, 0x7fff_ffff);
            Ok(Self(edit))
        }
    }

    pub fn ranges(&mut self, text: &str, query: &str, case: bool) -> Vec<CharRange> {
        unsafe {
            let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            SetWindowTextW(self.0, text.as_ptr());
            let mut ranges = Vec::new();
            editing::each_match(self.0, query, case, |range| ranges.push(range));
            ranges
        }
    }

    pub fn scan(&mut self, id: u64, text: &str, query: &str, case: bool) -> Option<ResultRow> {
        unsafe {
            let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            SetWindowTextW(self.0, units.as_ptr());
            let mut first = None;
            let count = editing::each_match(self.0, query, case, |range| {
                first.get_or_insert(range);
            });
            let range = first?;
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            let mut offset = 0;
            let byte = normalized
                .char_indices()
                .find_map(|(byte, ch)| {
                    if offset >= range.start {
                        Some(byte)
                    } else {
                        offset += ch.len_utf16() as i32;
                        None
                    }
                })
                .unwrap_or(normalized.len());
            let line = normalized[..byte].matches('\n').count() + 1;
            let start = normalized[..byte].rfind('\n').map_or(0, |pos| pos + 1);
            let end = normalized[byte..]
                .find('\n')
                .map_or(normalized.len(), |pos| byte + pos);
            let preview_start = normalized[start..byte]
                .char_indices()
                .rev()
                .nth(23)
                .map_or(start, |(at, _)| start + at);
            let excerpt: String = normalized[preview_start..end]
                .trim()
                .chars()
                .take(90)
                .collect();
            let preview = format!(
                "{}{}",
                if preview_start > start { "…" } else { "" },
                excerpt
            );
            Some(ResultRow {
                id,
                count,
                line,
                preview,
            })
        }
    }
}
impl Drop for Scanner {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0);
        }
    }
}

/// Map RichEdit paragraph/UTF-16 positions back to original UTF-8 without
/// changing unrelated CRLF, LF, CR, Unicode, or the trailing newline.
pub fn replace(text: &str, ranges: &[CharRange], replacement: &str) -> String {
    let mut chars = text.char_indices().peekable();
    let mut units = 0;
    let mut byte = 0;
    let mut last = 0;
    let mut output = String::new();
    for range in ranges {
        let mut boundaries = [0; 2];
        for (index, target) in [range.start, range.end].into_iter().enumerate() {
            while units < target {
                let Some((at, ch)) = chars.next() else {
                    break;
                };
                units += ch.len_utf16() as i32;
                byte = at + ch.len_utf8();
                if ch == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
                    byte = chars.next().unwrap().0 + 1;
                }
            }
            boundaries[index] = byte;
        }
        output.push_str(&text[last..boundaries[0]]);
        output.push_str(replacement);
        last = boundaries[1];
    }
    output.push_str(&text[last..]);
    output
}

pub struct Change {
    pub id: u64,
    pub path: PathBuf,
    pub before_disk: Vec<u8>,
    pub after: String,
    pub count: usize,
}

/// Validate snapshots before writing; restore already written files on failure.
pub fn commit(changes: &[Change]) -> io::Result<()> {
    for change in changes {
        if fs::read(&change.path)? != change.before_disk {
            return Err(io::Error::other(format!(
                "다른 프로그램에서 파일을 변경했습니다: {}",
                change.path.display()
            )));
        }
    }
    for (index, change) in changes.iter().enumerate() {
        if let Err(error) = replace_file(&change.path, change.after.as_bytes()) {
            let mut failures = Vec::new();
            for previous in changes[..index].iter().rev() {
                if let Err(restore) = replace_file(&previous.path, &previous.before_disk) {
                    failures.push(format!("{}: {restore}", previous.path.display()));
                }
            }
            return Err(io::Error::other(format!(
                "{}: {error}{}",
                change.path.display(),
                if failures.is_empty() {
                    String::new()
                } else {
                    format!("\n복원 실패: {}", failures.join("\n"))
                }
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_preserves_mixed_line_endings_and_unicode() {
        let text = "🙂 가나다\r\nABC 가나다\n끝\r마지막\r\n";
        let ranges = [
            CharRange { start: 3, end: 6 },
            CharRange { start: 11, end: 14 },
        ];
        assert_eq!(
            replace(text, &ranges, "바꿈"),
            "🙂 바꿈\r\nABC 바꿈\n끝\r마지막\r\n"
        );
        assert_eq!(replace(text, &ranges, ""), "🙂 \r\nABC \n끝\r마지막\r\n");
    }
    #[test]
    fn batch_write_rolls_back_and_rejects_external_changes() {
        let dir = std::env::temp_dir().join(format!(
            "edot-global-replace-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let changes: Vec<_> = (0..2)
            .map(|id| {
                let path = dir.join(format!("{id}.txt"));
                fs::write(&path, b"original").unwrap();
                Change {
                    id,
                    path,
                    before_disk: b"original".to_vec(),
                    after: "changed".into(),
                    count: 1,
                }
            })
            .collect();
        fs::create_dir(dir.join("1.tmp")).unwrap();
        assert!(commit(&changes).is_err());
        assert_eq!(fs::read(&changes[0].path).unwrap(), b"original");
        assert_eq!(fs::read(&changes[1].path).unwrap(), b"original");
        fs::remove_dir(dir.join("1.tmp")).unwrap();
        fs::write(&changes[1].path, b"external").unwrap();
        assert!(commit(&changes).is_err());
        assert_eq!(fs::read(&changes[0].path).unwrap(), b"original");
        fs::write(&changes[1].path, b"original").unwrap();
        commit(&changes).unwrap();
        assert_eq!(fs::read(&changes[0].path).unwrap(), b"changed");
        assert_eq!(fs::read(&changes[1].path).unwrap(), b"changed");
        fs::remove_dir_all(dir).unwrap();
    }
}
