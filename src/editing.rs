//! UTF-16 paragraph coordinates shared with the original Edot search format.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct CharRange {
    pub start: i32,
    pub end: i32,
}

/// Convert normalized UTF-16 paragraph positions to original UTF-8 bytes.
pub fn byte_range(text: &str, range: CharRange) -> std::ops::Range<usize> {
    let mut positions = [text.len(); 2];
    let mut chars = text.char_indices().peekable();
    let mut units = 0;
    let mut byte = 0;
    for (index, target) in [range.start, range.end].into_iter().enumerate() {
        while units < target {
            let Some((at, ch)) = chars.next() else {
                break;
            };
            units += ch.len_utf16() as i32;
            byte = at + ch.len_utf8();
            if ch == '\r' && chars.peek().is_some_and(|(_, c)| *c == '\n') {
                byte = chars.next().unwrap().0 + 1;
            }
        }
        positions[index] = byte;
    }
    positions[0]..positions[1]
}
pub fn paragraph_units(text: &str) -> i32 {
    paragraph_units_iter(text.chars())
}
pub fn paragraph_units_iter(chars: impl Iterator<Item = char>) -> i32 {
    let mut previous = '\0';
    chars
        .map(|ch| {
            let units = if ch == '\n' && previous == '\r' {
                0
            } else {
                ch.len_utf16() as i32
            };
            previous = ch;
            units
        })
        .sum()
}

#[derive(Default)]
pub struct TextMetrics {
    pub total: i32,
    pub lines: Vec<usize>,
}
impl TextMetrics {
    pub fn new(text: &str) -> Self {
        let mut lines = vec![0];
        let mut chars = text.char_indices().peekable();
        while let Some((offset, ch)) = chars.next() {
            if ch == '\r' && chars.peek().is_some_and(|(_, c)| *c == '\n') {
                lines.push(chars.next().unwrap().0 + 1);
            } else if matches!(ch, '\r' | '\n') {
                lines.push(offset + 1);
            }
        }
        Self {
            total: paragraph_units(text),
            lines,
        }
    }
    pub fn line(&self, byte: usize) -> (usize, usize) {
        let line = self
            .lines
            .partition_point(|offset| *offset <= byte)
            .saturating_sub(1);
        (line + 1, self.lines.get(line).copied().unwrap_or(0))
    }
}
pub fn paragraph_range(text: &str, range: std::ops::Range<usize>) -> CharRange {
    CharRange {
        start: paragraph_units(&text[..range.start]),
        end: paragraph_units(&text[..range.end]),
    }
}
