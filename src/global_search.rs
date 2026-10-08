use crate::editing::CharRange;
use crate::model::NoteId;
use crate::storage::replace_file;
use std::{collections::HashSet, fs, io, ops::Range, path::PathBuf, sync::Arc};
// Bound interactive result memory; replacement still considers every match.
pub const MAX_RESULTS: usize = 20_000;

#[derive(Clone, Debug)]
pub struct Match {
    pub range: CharRange,
    pub line: usize,
    pub column: usize,
    pub preview: Arc<str>,
    pub highlight: Range<usize>, // UTF-8 byte boundaries in the preview
}
#[derive(Clone, Debug)]
pub struct ResultRow {
    pub id: NoteId,
    pub count: usize,
    pub matches: Vec<Match>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchKey {
    pub id: NoteId,
    pub range: CharRange,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleRow {
    pub group: usize,
    pub hit: Option<usize>, // None is the collapsible memo header
}
pub fn visible_rows(groups: &[ResultRow], collapsed: &HashSet<NoteId>) -> Vec<VisibleRow> {
    let mut rows = Vec::new();
    for (group, result) in groups.iter().enumerate() {
        rows.push(VisibleRow { group, hit: None });
        if !collapsed.contains(&result.id) {
            rows.extend((0..result.matches.len()).map(|hit| VisibleRow {
                group,
                hit: Some(hit),
            }));
        }
    }
    rows
}

/// Walk source text only once for native positions and line/column coordinates.
/// Nearby matches can share a short preview, rather than storing whole files.
fn match_previews(text: &str, ranges: &[CharRange]) -> Vec<Match> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut cursor = normalized.char_indices().peekable();
    let mut units = 0;
    let mut line = 1;
    let mut line_start = 0;
    let mut line_units = 0;
    let mut cached: Option<(usize, usize, Arc<str>)> = None;
    let mut cached_line_end = (usize::MAX, 0);
    let mut matches = Vec::with_capacity(ranges.len());
    for &range in ranges {
        while units < range.start {
            let Some((at, ch)) = cursor.next() else {
                break;
            };
            units += ch.len_utf16() as i32;
            if ch == '\n' {
                line += 1;
                line_start = at + 1;
                line_units = units;
            }
        }
        let start = cursor.peek().map_or(normalized.len(), |(at, _)| *at);
        let hit_line = line;
        let column = (range.start - line_units) as usize + 1;
        if cached_line_end.0 != line_start {
            cached_line_end = (
                line_start,
                normalized[start..]
                    .find('\n')
                    .map_or(normalized.len(), |at| start + at),
            );
        }
        let excerpt_start = normalized[line_start..start]
            .char_indices()
            .rev()
            .nth(23)
            .map_or(line_start, |(at, _)| line_start + at);
        let excerpt_end = normalized[excerpt_start..cached_line_end.1]
            .char_indices()
            .nth(90)
            .map_or(cached_line_end.1, |(at, _)| excerpt_start + at);
        while units < range.end {
            let Some((at, ch)) = cursor.next() else {
                break;
            };
            units += ch.len_utf16() as i32;
            if ch == '\n' {
                line += 1;
                line_start = at + 1;
                line_units = units;
            }
        }
        let end = cursor.peek().map_or(normalized.len(), |(at, _)| *at);
        let leading = if excerpt_start > cached_line_end.0 {
            "…"
        } else {
            ""
        };
        let preview = if let Some((a, b, preview)) = &cached
            && *a == excerpt_start
            && *b == excerpt_end
        {
            Arc::clone(preview)
        } else {
            let preview: Arc<str> = format!(
                "{leading}{}{}",
                normalized[excerpt_start..excerpt_end].replace('\t', " "),
                if excerpt_end < cached_line_end.1 {
                    "…"
                } else {
                    ""
                }
            )
            .into();
            cached = Some((excerpt_start, excerpt_end, Arc::clone(&preview)));
            preview
        };
        let highlight = leading.len() + start - excerpt_start
            ..leading.len() + end.min(excerpt_end) - excerpt_start;
        matches.push(Match {
            range,
            line: hit_line,
            column,
            preview,
            highlight,
        });
    }
    matches
}

pub fn scan(
    id: NoteId,
    text: &str,
    pattern: &crate::search::Pattern,
    limit: usize,
) -> Option<ResultRow> {
    let mut ranges = Vec::new();
    let count = pattern.visit_text(text, |range| {
        if ranges.len() < limit {
            ranges.push(range);
        }
        true
    });
    (count > 0).then(|| ResultRow {
        id,
        count,
        matches: match_previews(text, &ranges),
    })
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
    pub id: NoteId,
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
