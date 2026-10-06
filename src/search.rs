//! Shared literal/regex search with stable UTF-16 paragraph coordinates.
use crate::editing::CharRange;
use regex_lite::{Regex, RegexBuilder};
use std::sync::Arc;

#[derive(Clone)]
pub enum Pattern {
    Literal { query: Arc<str>, case: bool },
    Regex(Arc<Regex>),
}

#[derive(Default)]
pub struct Cache {
    key: Option<(String, bool, bool)>,
    value: Option<Result<Pattern, String>>,
}
impl Cache {
    pub fn get(&mut self, query: &str, case: bool, regex: bool) -> Result<Pattern, String> {
        if self
            .key
            .as_ref()
            .is_none_or(|key| key.0 != query || key.1 != case || key.2 != regex)
        {
            self.value = Some(Pattern::compile(query, case, regex));
            self.key = Some((query.to_owned(), case, regex));
        }
        self.value.as_ref().unwrap().clone()
    }
}

pub struct Edit {
    pub range: CharRange,
    pub replacement: String,
}

fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

struct Positions<'a> {
    text: &'a str,
    byte: usize,
    units: i32,
}
impl<'a> Positions<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            byte: 0,
            units: 0,
        }
    }
    fn at(&mut self, byte: usize) -> i32 {
        self.units += self.text[self.byte..byte].encode_utf16().count() as i32;
        self.byte = byte;
        self.units
    }
    fn range(&mut self, start: usize, end: usize) -> CharRange {
        CharRange {
            start: self.at(start),
            end: self.at(end),
        }
    }
}

impl Pattern {
    pub fn compile(query: &str, case: bool, regex: bool) -> Result<Self, String> {
        if !regex {
            return Ok(Self::Literal {
                query: query.into(),
                case,
            });
        }
        RegexBuilder::new(query)
            .case_insensitive(!case)
            .multi_line(true)
            .size_limit(256 * 1024)
            .build()
            .map(|regex| Self::Regex(Arc::new(regex)))
            .map_err(|error| {
                format!(
                    "정규식 오류: {}",
                    error.to_string().lines().last().unwrap_or("잘못된 표현식")
                )
            })
    }

    /// UTF-8 matches are mapped in one sequential pass without a per-character table.
    pub fn visit_text(&self, text: &str, mut visit: impl FnMut(CharRange) -> bool) -> usize {
        let Self::Regex(regex) = self else {
            let Self::Literal { query, case } = self else {
                unreachable!()
            };
            return visit_literal(text, query, *case, visit);
        };
        let text = normalized(text);
        let mut positions = Positions::new(&text);
        let mut count = 0;
        for hit in regex.find_iter(&text) {
            count += 1;
            if !visit(positions.range(hit.start(), hit.end())) {
                break;
            }
        }
        count
    }

    pub fn edits_text(&self, text: &str, replacement: &str) -> Vec<Edit> {
        let Self::Regex(regex) = self else {
            let Self::Literal { query, case } = self else {
                unreachable!()
            };
            return literal_ranges(text, query, *case)
                .into_iter()
                .map(|range| Edit {
                    range,
                    replacement: replacement.to_owned(),
                })
                .collect();
        };
        let text = normalized(text);
        let mut positions = Positions::new(&text);
        regex
            .captures_iter(&text)
            .map(|captures| {
                let hit = captures.get(0).unwrap();
                let mut expanded = String::new();
                captures.expand(replacement, &mut expanded);
                Edit {
                    range: positions.range(hit.start(), hit.end()),
                    replacement: expanded,
                }
            })
            .collect()
    }
}

/// Lowercase with original character boundaries, preserving Unicode offsets.
fn literal_ranges(text: &str, query: &str, case: bool) -> Vec<CharRange> {
    let mut ranges = vec![];
    visit_literal(text, query, case, |range| {
        ranges.push(range);
        true
    });
    ranges
}

fn visit_literal(
    text: &str,
    query: &str,
    case: bool,
    mut visit: impl FnMut(CharRange) -> bool,
) -> usize {
    if query.is_empty() {
        return 0;
    }
    let text = normalized(text);
    let query = normalized(query);
    if case {
        let mut positions = Positions::new(&text);
        let mut count = 0;
        for (at, hit) in text.match_indices(&query) {
            count += 1;
            if !visit(positions.range(at, at + hit.len())) {
                break;
            }
        }
        return count;
    }
    let folded: String = text.chars().flat_map(char::to_lowercase).collect();
    let mut chars = text.chars();
    let mut byte = 0;
    let mut units = 0;
    let query: String = query.chars().flat_map(char::to_lowercase).collect();
    let mut count = 0;
    for (at, hit) in folded.match_indices(&query) {
        let mut bounds = [0; 2];
        let mut valid = true;
        for (index, target) in [at, at + hit.len()].into_iter().enumerate() {
            while byte < target {
                let ch = chars.next().expect("folded text retains source boundaries");
                byte += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
                units += ch.len_utf16() as i32;
            }
            valid &= byte == target;
            bounds[index] = units;
        }
        if valid {
            count += 1;
            if !visit(CharRange {
                start: bounds[0],
                end: bounds[1],
            }) {
                break;
            }
        }
    }
    count
}

/// Preserve source line endings outside replacement ranges, including closed memo files.
pub fn apply_edits(text: &str, edits: &[Edit]) -> String {
    let mut chars = text.char_indices().peekable();
    let mut units = 0;
    let mut byte = 0;
    let mut last = 0;
    let mut output = String::new();
    for edit in edits {
        let mut bounds = [0; 2];
        for (index, target) in [edit.range.start, edit.range.end].into_iter().enumerate() {
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
            bounds[index] = byte;
        }
        output.push_str(&text[last..bounds[0]]);
        output.push_str(&edit.replacement);
        last = bounds[1];
    }
    output.push_str(&text[last..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_unicode_boundaries_line_endings_and_early_stop() {
        let p = Pattern::compile("i\u{307}", false, false).unwrap();
        let mut ranges = vec![];
        p.visit_text("🙂İ\r\ni\u{307}", |range| {
            ranges.push(range);
            true
        });
        assert_eq!(
            ranges,
            [
                CharRange { start: 2, end: 3 },
                CharRange { start: 4, end: 6 }
            ]
        );
        assert_eq!(
            Pattern::compile("i", false, false)
                .unwrap()
                .visit_text("İI", |_| true),
            1
        );
        assert_eq!(
            Pattern::compile("a", false, false)
                .unwrap()
                .visit_text("aAaA", |_| false),
            1
        );
        assert_eq!(
            apply_edits(
                "한글\r\n한글",
                &Pattern::compile("한글", true, false)
                    .unwrap()
                    .edits_text("한글\r\n한글", "$1")
            ),
            "$1\r\n$1"
        );
    }
    #[test]
    fn regex_coordinates_captures_and_mixed_paragraphs() {
        let pattern = Pattern::compile(r"(?<word>한글)(\d+)", true, true).unwrap();
        let text = "🙂 한글12\r\n한글3\rkeep\n";
        let edits = pattern.edits_text(text, "${word}-$2-$$");
        assert_eq!(edits[0].range, CharRange { start: 3, end: 7 });
        assert_eq!(edits[1].range, CharRange { start: 8, end: 11 });
        assert_eq!(
            apply_edits(text, &edits),
            "🙂 한글-12-$\r\n한글-3-$\rkeep\n"
        );
        let mut ranges = Vec::new();
        pattern.visit_text(text, |range| {
            ranges.push(range);
            true
        });
        assert_eq!(
            ranges,
            edits.iter().map(|edit| edit.range).collect::<Vec<_>>()
        );
    }
    #[test]
    fn regex_anchors_empty_matches_and_errors() {
        let pattern = Pattern::compile("^", false, true).unwrap();
        let text = "🙂\r\n한글\r끝\n";
        let edits = pattern.edits_text(text, ">");
        assert_eq!(
            edits.iter().map(|e| e.range.start).collect::<Vec<_>>(),
            [0, 3, 6, 8]
        );
        assert_eq!(apply_edits(text, &edits), ">🙂\r\n>한글\r>끝\n>");
        assert!(Pattern::compile("[", true, true).is_err());
        assert!(Pattern::compile("[", true, false).is_ok());
        let mut count = 0;
        Pattern::compile("n[e]+dle", false, true)
            .unwrap()
            .visit_text("NEEDLE needle", |_| {
                count += 1;
                true
            });
        assert_eq!(count, 2);
        count = 0;
        Pattern::compile("needle", true, true)
            .unwrap()
            .visit_text("NEEDLE needle", |_| {
                count += 1;
                true
            });
        assert_eq!(count, 1);
    }
}
