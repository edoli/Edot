//! Parser-independent source byte spans. The editor never renders parsed text.
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag};

pub(super) const HEADING: u16 = 1 << 0;
pub(super) const STRONG: u16 = 1 << 1;
pub(super) const EMPHASIS: u16 = 1 << 2;
pub(super) const STRIKE: u16 = 1 << 3;
pub(super) const CODE: u16 = 1 << 4;
pub(super) const QUOTE: u16 = 1 << 5;
pub(super) const MARKER: u16 = 1 << 6;
pub(super) const LINK: u16 = 1 << 7;
pub(super) const RULE: u16 = 1 << 8;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Span {
    pub range: Range<usize>,
    pub style: u16,
}

/// Sorted, non-overlapping spans, including source delimiters. Nested emphasis
/// combines styles; plain text is omitted. No parser types escape this adapter.
pub(super) fn parse(text: &str) -> Vec<Span> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut boundaries = Vec::new();
    for (event, mut range) in Parser::new_ext(text, options).into_offset_iter() {
        let style = match event {
            Event::Start(Tag::Heading { .. }) => HEADING,
            Event::Start(Tag::Strong | Tag::TableHead) => STRONG,
            Event::Start(Tag::Emphasis) => EMPHASIS,
            Event::Start(Tag::Strikethrough) => STRIKE,
            Event::Start(Tag::CodeBlock(_)) | Event::Code(_) => CODE,
            Event::Start(Tag::BlockQuote(_)) => QUOTE,
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => LINK,
            Event::Start(Tag::Item) => {
                let bytes = &text.as_bytes()[range.clone()];
                let mut end = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
                if bytes.get(end).is_some_and(|b| {
                    if end == 0 {
                        matches!(b, b'-' | b'+' | b'*')
                    } else {
                        matches!(b, b'.' | b')')
                    }
                }) {
                    end += 1;
                    range.end = range.start + end;
                    MARKER
                } else {
                    continue;
                }
            }
            Event::TaskListMarker(_) => MARKER,
            Event::Rule => RULE,
            _ => continue,
        };
        if !range.is_empty() {
            let bit = style.trailing_zeros() as u8;
            boundaries.push((range.start, bit, 1i8));
            boundaries.push((range.end, bit, -1i8));
        }
    }
    boundaries.sort_unstable_by_key(|&(offset, _, _)| offset);
    let mut spans: Vec<Span> = Vec::new();
    let mut counts = [0i32; 9];
    let mut style = 0;
    let mut previous = 0;
    for (offset, bit, delta) in boundaries {
        if previous < offset && style != 0 {
            if let Some(last) = spans
                .last_mut()
                .filter(|s| s.range.end == previous && s.style == style)
            {
                last.range.end = offset;
            } else {
                spans.push(Span {
                    range: previous..offset,
                    style,
                });
            }
        }
        let count = &mut counts[usize::from(bit)];
        *count += i32::from(delta);
        if *count > 0 {
            style |= 1 << bit;
        } else {
            style &= !(1 << bit);
        }
        previous = offset;
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_spans_preserve_unicode_delimiters_and_nested_styles() {
        let text = "# 제목\r\n\r\n> **굵고 *기울임*** ~~삭제~~ `**코드**`\r\n\r\n- [x] 항목\r\n1. 순서\r\n\r\n[링크](https://example.org)\r\n\r\n~~~rust\r\n# **코드**\r\n~~~\r\n\r\n---\r\n";
        let spans = parse(text);
        for span in &spans {
            assert!(text.get(span.range.clone()).is_some());
        }
        assert!(spans.windows(2).all(|s| s[0].range.end <= s[1].range.start));
        let style_at = |part: &str| {
            let offset = text.find(part).unwrap();
            spans
                .iter()
                .find(|s| s.range.contains(&offset))
                .unwrap()
                .style
        };
        assert_eq!(style_at("# 제목"), HEADING);
        assert_eq!(style_at("기울임"), QUOTE | STRONG | EMPHASIS);
        assert_eq!(style_at("~~삭제~~"), QUOTE | STRIKE);
        assert_eq!(style_at("`**코드**`"), QUOTE | CODE);
        assert_eq!(style_at("- [x]"), MARKER);
        assert_eq!(style_at("[x]"), MARKER);
        assert_eq!(style_at("1. 순서"), MARKER);
        assert_eq!(style_at("[링크]"), LINK);
        assert_eq!(style_at("~~~rust"), CODE);
        assert_eq!(style_at("# **코드**"), CODE);
        assert_eq!(style_at("---"), RULE);
        assert!(parse("일반 텍스트\t🙂\r\n닫히지 않은 **기호").is_empty());
    }
}
