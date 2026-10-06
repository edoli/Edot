//! Shared regular expression semantics and RichEdit UTF-16 paragraph coordinates.
use crate::editing::{self, CharRange};
use regex_lite::{Regex, RegexBuilder};
use std::rc::Rc;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

#[derive(Clone)]
pub enum Pattern {
    Literal { query: Rc<str>, case: bool },
    Regex(Rc<Regex>),
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

pub unsafe fn editor_text(edit: HWND) -> String {
    unsafe {
        let mut units = vec![0; GetWindowTextLengthW(edit).max(0) as usize + 1];
        let length = GetWindowTextW(edit, units.as_mut_ptr(), units.len() as i32);
        String::from_utf16_lossy(&units[..length.max(0) as usize])
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
            .map(|regex| Self::Regex(Rc::new(regex)))
            .map_err(|error| {
                format!(
                    "정규식 오류: {}",
                    error.to_string().lines().last().unwrap_or("잘못된 표현식")
                )
            })
    }

    /// Only used for Regex. UTF-8 matches are mapped in one sequential pass.
    pub fn visit_text(&self, text: &str, mut visit: impl FnMut(CharRange) -> bool) -> usize {
        let Self::Regex(regex) = self else {
            return 0;
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

    pub unsafe fn find_editor(
        &self,
        edit: HWND,
        start: i32,
        end: i32,
        backwards: bool,
        skip_empty: bool,
    ) -> Option<CharRange> {
        unsafe {
            if let Self::Literal { query, case } = self {
                return editing::find(edit, query, start, end, backwards, *case);
            }
            let mut found = None;
            self.visit_text(&editor_text(edit), |range| {
                let inside = if backwards {
                    range.start >= end && range.end <= start
                } else {
                    range.start >= start && (end < 0 || range.end <= end)
                };
                if inside && !(skip_empty && range.start == start && range.end == start) {
                    found = Some(range);
                    if !backwards {
                        return false;
                    }
                }
                !backwards || range.start <= start
            });
            found
        }
    }

    pub fn edits_text(&self, text: &str, replacement: &str) -> Vec<Edit> {
        let Self::Regex(regex) = self else {
            return Vec::new();
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

    pub unsafe fn replacement_editor(
        &self,
        edit: HWND,
        range: CharRange,
        replacement: &str,
    ) -> Option<String> {
        unsafe {
            if let Self::Literal { query, case } = self {
                return (range.end > range.start
                    && editing::find(edit, query, range.start, range.end, false, *case)
                        == Some(range))
                .then(|| replacement.to_owned());
            }
            let Self::Regex(regex) = self else {
                return None;
            };
            let text = normalized(&editor_text(edit));
            let mut positions = Positions::new(&text);
            for captures in regex.captures_iter(&text) {
                let hit = captures.get(0).unwrap();
                let found = positions.range(hit.start(), hit.end());
                if found == range {
                    let mut expanded = String::new();
                    captures.expand(replacement, &mut expanded);
                    return Some(expanded);
                }
                if found.start > range.start {
                    break;
                }
            }
            None
        }
    }
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
