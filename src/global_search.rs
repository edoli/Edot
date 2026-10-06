use crate::editing::CharRange;
use crate::model::replace_file;
use std::{collections::HashSet, fs, io, ops::Range, path::PathBuf, sync::Arc};
// Bound interactive result memory; replacement still considers every match.
pub const MAX_RESULTS: usize = 20_000;
pub const ROW_HEIGHT: i32 = 26;

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
    pub id: u64,
    pub count: usize,
    pub matches: Vec<Match>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchKey {
    pub id: u64,
    pub range: CharRange,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleRow {
    pub group: usize,
    pub hit: Option<usize>, // None is the collapsible memo header
}
pub fn visible_rows(groups: &[ResultRow], collapsed: &HashSet<u64>) -> Vec<VisibleRow> {
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
    id: u64,
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
    fn grouped_previews_keep_every_position_across_unicode_and_line_endings() {
        let text = "🙂 needle needle\r\n한글 needle\rneedle\n";
        let ranges = [
            CharRange { start: 3, end: 9 },
            CharRange { start: 10, end: 16 },
            CharRange { start: 20, end: 26 },
            CharRange { start: 27, end: 33 },
        ];
        let matches = match_previews(text, &ranges);
        assert_eq!(
            matches
                .iter()
                .map(|hit| (hit.range, hit.line, hit.column))
                .collect::<Vec<_>>(),
            vec![
                (ranges[0], 1, 4),
                (ranges[1], 1, 11),
                (ranges[2], 2, 4),
                (ranges[3], 3, 1)
            ]
        );
        for hit in &matches {
            assert_eq!(&hit.preview[hit.highlight.clone()], "needle");
        }
        assert!(Arc::ptr_eq(&matches[0].preview, &matches[1].preview));
        let groups = [ResultRow {
            id: 7,
            count: 4,
            matches,
        }];
        let open = visible_rows(&groups, &HashSet::new());
        assert_eq!(open.len(), 5);
        assert_eq!(
            open[2],
            VisibleRow {
                group: 0,
                hit: Some(1)
            }
        );
        assert_eq!(
            visible_rows(&groups, &HashSet::from([7])),
            vec![VisibleRow {
                group: 0,
                hit: None
            }]
        );
        let long = format!("{}needle{}", "가".repeat(1000), "🙂".repeat(1000));
        let preview = match_previews(
            &long,
            &[CharRange {
                start: 1000,
                end: 1006,
            }],
        );
        assert_eq!(&preview[0].preview[preview[0].highlight.clone()], "needle");
        assert!(preview[0].preview.starts_with('…') && preview[0].preview.ends_with('…'));
        assert!(preview[0].preview.chars().count() <= 92);
    }

    #[test]
    #[ignore = "opt-in pure search performance measurement"]
    fn global_search_preview_cost() {
        let pattern = crate::search::Pattern::compile("needle", false, false).unwrap();
        for lines in [200, 5000, 15000] {
            let text = "🙂 needle needle\r\n".repeat(lines);
            let mut times = Vec::new();
            for _ in 0..5 {
                let start = std::time::Instant::now();
                let group = scan(1, &text, &pattern, MAX_RESULTS).unwrap();
                times.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(group.count, lines * 2);
                assert_eq!(group.matches.len(), (lines * 2).min(MAX_RESULTS));
                if group.matches.len() > 1 {
                    assert!(Arc::ptr_eq(
                        &group.matches[0].preview,
                        &group.matches[1].preview
                    ));
                }
            }
            times.sort_by(f64::total_cmp);
            println!(
                "global-search bytes={} total={} shown={} median_ms={:.3} max_ms={:.3}",
                text.len(),
                lines * 2,
                (lines * 2).min(MAX_RESULTS),
                times[2],
                times[4]
            );
        }
    }

    #[test]
    fn regex_empty_results_keep_line_coordinates_and_eof_preview() {
        let text = "🙂\r\n한글\r끝\n";
        let pattern = crate::search::Pattern::compile("^", true, true).unwrap();
        let mut ranges = Vec::new();
        pattern.visit_text(text, |range| {
            ranges.push(range);
            true
        });
        let hits = match_previews(text, &ranges);
        assert_eq!(
            hits.iter().map(|hit| hit.line).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        assert!(
            hits.iter()
                .all(|hit| hit.column == 1 && hit.highlight.is_empty())
        );
        assert!(hits[3].preview.is_empty());
    }

    #[test]
    #[ignore = "opt-in regular expression compilation and result measurement"]
    fn regex_search_cost() {
        for lines in [200, 5000, 15000] {
            let text = "🙂 needle needle\r\n".repeat(lines);
            let mut compile = Vec::new();
            let mut scan = Vec::new();
            for _ in 0..5 {
                let start = std::time::Instant::now();
                let pattern = crate::search::Pattern::compile(r"n(e+)dle", false, true).unwrap();
                compile.push(start.elapsed().as_secs_f64() * 1000.0);
                let start = std::time::Instant::now();
                let mut ranges = Vec::new();
                let count = pattern.visit_text(&text, |range| {
                    if ranges.len() < MAX_RESULTS {
                        ranges.push(range);
                    }
                    true
                });
                let previews = match_previews(&text, &ranges);
                scan.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(count, lines * 2);
                assert_eq!(previews.len(), count.min(MAX_RESULTS));
            }
            compile.sort_by(f64::total_cmp);
            scan.sort_by(f64::total_cmp);
            println!(
                "regex-search bytes={} total={} shown={} compile_median_ms={:.3} scan_median_ms={:.3} scan_max_ms={:.3}",
                text.len(),
                lines * 2,
                (lines * 2).min(MAX_RESULTS),
                compile[2],
                scan[2],
                scan[4]
            );
        }
    }

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
