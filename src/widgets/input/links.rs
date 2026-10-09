//! Web URL detection, decorations and exact glyph hit testing.
use super::*;

pub(super) fn ranges(text: &str) -> Vec<Range<usize>> {
    static PREFIX: std::sync::LazyLock<regex_lite::Regex> = std::sync::LazyLock::new(|| {
        regex_lite::Regex::new(r"(?i)https?://|www\.").expect("valid URL prefix pattern")
    });
    let mut links = Vec::new();
    let mut previous_end = 0;
    for prefix in PREFIX.find_iter(text) {
        let start = prefix.start();
        if start < previous_end
            || text[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '@'))
        {
            continue;
        }
        let tail = &text[prefix.end()..];
        let mut end = prefix.end()
            + tail
                .char_indices()
                .find(|(_, c)| {
                    c.is_whitespace()
                        || c.is_control()
                        || matches!(c, '<' | '>' | '"' | '\'' | '`' | '\\')
                })
                .map_or(tail.len(), |(i, _)| i);
        let candidate = &text[start..end];
        let mut balance = [0i32; 3];
        for c in candidate.chars() {
            match c {
                '(' => balance[0] += 1,
                ')' => balance[0] -= 1,
                '[' => balance[1] += 1,
                ']' => balance[1] -= 1,
                '{' => balance[2] += 1,
                '}' => balance[2] -= 1,
                _ => {}
            }
        }
        while let Some(c) = text[start..end].chars().next_back() {
            let bracket = match c {
                ')' => Some(0),
                ']' => Some(1),
                '}' => Some(2),
                _ => None,
            };
            if let Some(index) = bracket.filter(|index| balance[*index] < 0) {
                balance[index] += 1;
            } else if !matches!(
                c,
                '.' | ','
                    | ';'
                    | ':'
                    | '!'
                    | '?'
                    | '。'
                    | '，'
                    | '、'
                    | '；'
                    | '：'
                    | '！'
                    | '？'
                    | '…'
            ) {
                break;
            }
            end -= c.len_utf8();
        }
        if end <= prefix.end() {
            continue;
        }
        let authority = text[prefix.end()..end]
            .split(['/', '?', '#'])
            .next()
            .unwrap();
        let www = text[start..prefix.end()].eq_ignore_ascii_case("www.");
        if !authority.chars().any(char::is_alphanumeric)
            || www && (authority.starts_with('.') || authority.ends_with('.'))
        {
            continue;
        }
        links.push(start..end);
        previous_end = end;
    }
    links
}

pub(super) fn text_runs(
    len: usize,
    tabs: &[(usize, usize, usize)],
    links: &[Range<usize>],
    base: &TextRun,
    color: Hsla,
) -> Vec<TextRun> {
    let mut runs = Vec::new();
    if links.is_empty() {
        return runs;
    }
    let mut offset = 0;
    for link in links {
        let start = layout::display_index(link.start, tabs);
        let end = layout::display_index(link.end, tabs);
        if start > offset {
            runs.push(TextRun {
                len: start - offset,
                ..base.clone()
            });
        }
        runs.push(TextRun {
            len: end - start,
            color,
            underline: Some(UnderlineStyle {
                thickness: px(1.),
                color: Some(color),
                wavy: false,
            }),
            ..base.clone()
        });
        offset = end;
    }
    if offset < len {
        runs.push(TextRun {
            len: len - offset,
            ..base.clone()
        });
    }
    runs
}

impl InputState {
    pub(super) fn link_at(&self, position: Point<Pixels>) -> Option<Range<usize>> {
        if !self.multiline
            || self.is_composing()
            || self.content.is_empty()
            || self
                .layout_key
                .as_ref()
                .is_none_or(|key| key.0 != self.revision)
            || !self.bounds.contains(&position)
            || [false, true].into_iter().any(|axis| {
                self.scrollbar_geometry(axis)
                    .is_some_and(|(track, _)| track.contains(&position))
            })
        {
            return None;
        }
        let local =
            position - self.bounds.origin - self.scroll - point(self.content_padding.left, px(0.));
        let index = self
            .lines
            .partition_point(|line| line.top <= local.y)
            .checked_sub(1)?;
        let line = &self.lines[index];
        if local.y >= line.top + line.line.size(self.line_height).height {
            return None;
        }
        let byte = line.source_index(
            line.line
                .index_for_position(point(local.x, local.y - line.top), self.line_height)
                .ok()?,
        );
        let link = line.links.iter().find(|link| link.contains(&byte))?;
        Some(line.start + link.start..line.start + link.end)
    }

    pub(super) fn open_link_at(&self, position: Point<Pixels>, cx: &App) -> bool {
        let Some(range) = self.link_at(position) else {
            return false;
        };
        let url = &self.content[range];
        if url
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("www."))
        {
            cx.open_url(&format!("https://{url}"));
        } else {
            cx.open_url(url);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{TextRun, layout, ranges, text_runs};

    #[test]
    fn web_urls_keep_balanced_paths_unicode_and_queries_but_exclude_surrounding_punctuation() {
        let text = "한글\t(https://example.com/a_(b)).\r\n[HTTPS://예시.한국/경로?q=1&v=2#부분], www.example.org/path! <http://localhost:3000/> https://[::1]/ testwww.example.com https:// www. \u{3000}https://example.org/?next=https://other.org";
        let links = ranges(text);
        assert_eq!(
            links
                .iter()
                .map(|range| &text[range.clone()])
                .collect::<Vec<_>>(),
            [
                "https://example.com/a_(b)",
                "HTTPS://예시.한국/경로?q=1&v=2#부분",
                "www.example.org/path",
                "http://localhost:3000/",
                "https://[::1]/",
                "https://example.org/?next=https://other.org"
            ]
        );
        assert!(ranges("file:///notes/a.txt javascript:alert(1) no link").is_empty());
        let text = "앞\twww.example.com 뒤";
        let (display, tabs) = layout::expand_tabs(text);
        let base = TextRun {
            len: display.len(),
            ..Default::default()
        };
        let runs = text_runs(
            display.len(),
            &tabs,
            &ranges(text),
            &base,
            gpui::rgb(0x2674f5).into(),
        );
        assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), display.len());
        assert_eq!(runs.iter().filter(|run| run.underline.is_some()).count(), 1);
        assert_eq!(runs[1].len, "www.example.com".len());
    }
}
