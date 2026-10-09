//! Rendered Markdown data, independent of GPUI and the source editor.
use super::markdown::{CODE, EMPHASIS, LINK, STRIKE, STRONG, Span};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::{ops::Range, sync::Arc};

#[derive(Default)]
pub(super) struct Inline {
    pub offset: usize,
    pub text: String,
    pub spans: Vec<Span>,
    pub links: Vec<(Range<usize>, String)>,
    pub images: Vec<String>,
}
impl Inline {
    fn push(&mut self, text: &str, style: u16) {
        let start = self.text.len();
        self.text.push_str(text);
        if style != 0 && start < self.text.len() {
            if let Some(last) = self
                .spans
                .last_mut()
                .filter(|s| s.style == style && s.range.end == start)
            {
                last.range.end = self.text.len();
            } else {
                self.spans.push(Span {
                    range: start..self.text.len(),
                    style,
                });
            }
        }
    }
}
pub(super) enum Kind {
    Paragraph,
    Heading(u8),
    Code,
    Mermaid(Result<Arc<super::mermaid::Graph>, &'static str>),
    Rule,
    TableRow { header: bool, alignment: Vec<Align> },
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Align {
    Left,
    Center,
    Right,
}
pub(super) struct Block {
    pub offset: usize,
    pub kind: Kind,
    pub content: Inline,
    pub cells: Vec<Inline>,
    pub prefix: String,
    pub indent: usize,
    pub quote: usize,
    pub source: Range<usize>,
}
#[derive(Default)]
pub(super) struct Document {
    pub blocks: Vec<Block>,
    pub len: usize,
}
impl Document {
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.text_range(0..self.len)
    }
    pub fn text_range(&self, range: Range<usize>) -> String {
        if range.is_empty() {
            return String::new();
        }
        let mut text = String::with_capacity(range.end.saturating_sub(range.start));
        let mut append = |offset: usize, value: &str| {
            let start = range.start.max(offset);
            let end = range.end.min(offset + value.len());
            if start < end {
                text.push_str(&value[start - offset..end - offset]);
            }
        };
        let first = self
            .blocks
            .partition_point(|b| b.offset <= range.start)
            .saturating_sub(1);
        for block in &self.blocks[first..] {
            if block.offset.saturating_sub(2) >= range.end {
                break;
            }
            if block.offset > 0 {
                append(block.offset - 2, "\n\n");
            }
            append(block.offset, &block.prefix);
            if !block.prefix.is_empty() {
                append(block.offset + block.prefix.len(), " ");
            }
            if block.cells.is_empty() {
                append(block.content.offset, &block.content.text);
            } else {
                for (i, cell) in block.cells.iter().enumerate() {
                    if i > 0 {
                        append(cell.offset - 1, "\t");
                    }
                    append(cell.offset, &cell.text);
                }
            }
        }
        text
    }
    fn index_text(&mut self) {
        let mut offset = 0;
        for block in &mut self.blocks {
            if offset > 0 {
                offset += 2;
            }
            block.offset = offset;
            offset += block.prefix.len() + usize::from(!block.prefix.is_empty());
            block.content.offset = offset;
            if block.cells.is_empty() {
                offset += block.content.text.len();
            } else {
                for (i, cell) in block.cells.iter_mut().enumerate() {
                    offset += usize::from(i > 0);
                    cell.offset = offset;
                    offset += cell.text.len();
                }
            }
        }
        self.len = offset;
    }
}

struct Builder {
    document: Document,
    current: Option<Block>,
    lists: Vec<Option<u64>>,
    quote: usize,
    prefix: String,
    counts: [u32; 4],
    link: Option<(usize, String)>,
    image: Option<(usize, String)>,
    table: Vec<Align>,
    row: Option<Block>,
}
impl Builder {
    fn flush(&mut self) {
        if let Some(mut block) = self.current.take() {
            if matches!(block.kind, Kind::Mermaid(_)) {
                block.kind =
                    Kind::Mermaid(super::mermaid::parse(&block.content.text).map(Arc::new));
            }
            if let Some(row) = &mut self.row {
                row.cells.push(block.content);
            } else {
                self.document.blocks.push(block);
            }
        }
    }
    fn start(&mut self, kind: Kind, range: Range<usize>) {
        self.flush();
        self.current = Some(Block {
            offset: 0,
            kind,
            content: Inline::default(),
            cells: Vec::new(),
            prefix: std::mem::take(&mut self.prefix),
            indent: self.lists.len().saturating_sub(1),
            quote: self.quote,
            source: range,
        });
    }
    fn ensure(&mut self, range: Range<usize>) -> &mut Block {
        if self.current.is_none() {
            self.start(Kind::Paragraph, range.clone());
        }
        let block = self.current.as_mut().unwrap();
        block.source.end = block.source.end.max(range.end);
        block
    }
    fn push(&mut self, text: &str, range: Range<usize>, extra: u16) {
        let mut style = extra;
        for (count, bit) in self.counts.iter().zip([STRONG, EMPHASIS, STRIKE, LINK]) {
            if *count > 0 {
                style |= bit;
            }
        }
        self.ensure(range).content.push(text, style);
    }
    fn row(&mut self, header: bool, range: Range<usize>) {
        self.start(
            Kind::TableRow {
                header,
                alignment: self.table.clone(),
            },
            range,
        );
        self.row = self.current.take();
    }
    fn finish_row(&mut self) {
        self.flush();
        if let Some(row) = self.row.take() {
            self.document.blocks.push(row);
        }
    }
}

pub(super) fn parse(text: &str) -> Document {
    let mut b = Builder {
        document: Document::default(),
        current: None,
        lists: Vec::new(),
        quote: 0,
        prefix: String::new(),
        counts: [0; 4],
        link: None,
        image: None,
        table: Vec::new(),
        row: None,
    };
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    if b.current
                        .as_ref()
                        .is_none_or(|c| !c.content.text.is_empty())
                    {
                        b.start(Kind::Paragraph, range);
                    }
                }
                Tag::Heading { level, .. } => b.start(Kind::Heading(level as u8), range),
                Tag::CodeBlock(CodeBlockKind::Fenced(info))
                    if info
                        .split_whitespace()
                        .next()
                        .is_some_and(|s| s.eq_ignore_ascii_case("mermaid")) =>
                {
                    b.start(Kind::Mermaid(Err("")), range);
                }
                Tag::CodeBlock(_) | Tag::HtmlBlock => b.start(Kind::Code, range),
                Tag::BlockQuote(_) => {
                    b.flush();
                    b.quote += 1;
                }
                Tag::List(start) => {
                    b.flush();
                    b.lists.push(start);
                }
                Tag::Item => {
                    b.flush();
                    b.prefix = if let Some(Some(number)) = b.lists.last_mut() {
                        let prefix = format!("{number}.");
                        *number = number.saturating_add(1);
                        prefix
                    } else {
                        "•".into()
                    };
                    b.start(Kind::Paragraph, range.start..range.start);
                }
                Tag::Strong => b.counts[0] += 1,
                Tag::Emphasis => b.counts[1] += 1,
                Tag::Strikethrough => b.counts[2] += 1,
                Tag::Link { dest_url, .. } => {
                    let start = b.ensure(range).content.text.len();
                    b.link = Some((start, dest_url.to_string()));
                    b.counts[3] += 1;
                }
                Tag::Image { dest_url, .. } => {
                    let start = b.ensure(range).content.text.len();
                    b.image = Some((start, dest_url.to_string()));
                }
                Tag::Table(alignment) => {
                    b.flush();
                    b.table = alignment
                        .into_iter()
                        .map(|a| match a {
                            Alignment::Center => Align::Center,
                            Alignment::Right => Align::Right,
                            _ => Align::Left,
                        })
                        .collect();
                }
                Tag::TableHead => b.row(true, range),
                Tag::TableRow => b.row(false, range),
                Tag::TableCell => b.start(Kind::Paragraph, range),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::CodeBlock
                | TagEnd::HtmlBlock
                | TagEnd::Item
                | TagEnd::TableCell => b.flush(),
                TagEnd::BlockQuote(_) => {
                    b.flush();
                    b.quote = b.quote.saturating_sub(1);
                }
                TagEnd::List(_) => {
                    b.flush();
                    b.lists.pop();
                }
                TagEnd::Strong => b.counts[0] -= 1,
                TagEnd::Emphasis => b.counts[1] -= 1,
                TagEnd::Strikethrough => b.counts[2] -= 1,
                TagEnd::Link => {
                    if let Some((start, target)) = b.link.take() {
                        let inline = &mut b.ensure(range).content;
                        inline.links.push((start..inline.text.len(), target));
                    }
                    b.counts[3] -= 1;
                }
                TagEnd::Image => {
                    if let Some((start, target)) = b.image.take() {
                        let inline = &mut b.ensure(range).content;
                        if start == inline.text.len() {
                            inline.push("이미지", 0);
                        }
                        inline.images.push(target);
                    }
                }
                TagEnd::TableHead | TagEnd::TableRow => b.finish_row(),
                TagEnd::Table => {
                    b.finish_row();
                    b.table.clear();
                }
                _ => {}
            },
            Event::Text(value) | Event::Html(value) | Event::InlineHtml(value) => {
                b.push(&value, range, 0)
            }
            Event::Code(value) => b.push(&value, range, CODE),
            Event::SoftBreak => b.push(" ", range, 0),
            Event::HardBreak => b.push("\n", range, 0),
            Event::TaskListMarker(checked) => {
                b.ensure(range).prefix = if checked { "☑" } else { "☐" }.into();
            }
            Event::Rule => {
                b.start(Kind::Rule, range);
                b.flush();
            }
            _ => {}
        }
    }
    b.flush();
    b.document.index_text();
    b.document
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rendered_blocks_keep_nested_styles_and_destinations_without_source_markers() {
        let source = "# 제목\r\n\r\n**굵은 *한글🙂*** ~~삭제~~ `a < b` &amp; [링크](https://example.org)\r\n\r\n> 인용\r\n\r\n3. 항목\r\n   - 하위\r\n4. 다음\r\n\r\n- [x] 완료\r\n\r\n~~~rust\r\n**원문**\r\n~~~\r\n\r\n| 이름 | 값 |\r\n| :--- | ---: |\r\n| 가 | 1 |\r\n\r\n![그림](image.png)\r\n\r\n<script>alert(1)</script>\r\n";
        let doc = parse(source);
        assert!(matches!(doc.blocks[0].kind, Kind::Heading(1)));
        assert_eq!(doc.blocks[0].content.text, "제목");
        let p = &doc.blocks[1].content;
        assert_eq!(p.text, "굵은 한글🙂 삭제 a < b & 링크");
        let at = p.text.find("한글").unwrap();
        assert_eq!(
            p.spans
                .iter()
                .find(|s| s.range.contains(&at))
                .unwrap()
                .style,
            STRONG | EMPHASIS
        );
        assert_eq!(p.links[0].1, "https://example.org");
        assert_eq!(&p.text[p.links[0].0.clone()], "링크");
        assert!(
            doc.blocks
                .iter()
                .any(|b| b.quote == 1 && b.content.text == "인용")
        );
        assert!(doc.blocks.iter().any(|b| b.prefix == "3."));
        assert!(doc.blocks.iter().any(|b| b.prefix == "4."));
        assert!(doc.blocks.iter().any(|b| b.indent == 1 && b.prefix == "•"));
        assert!(doc.blocks.iter().any(|b| b.prefix == "☑"));
        assert!(
            doc.blocks
                .iter()
                .any(|b| matches!(b.kind, Kind::Code) && b.content.text == "**원문**\n")
        );
        let table: Vec<_> = doc
            .blocks
            .iter()
            .filter(|b| matches!(b.kind, Kind::TableRow { .. }))
            .collect();
        assert_eq!(table.len(), 2);
        assert_eq!(table[0].cells[0].text, "이름");
        assert_eq!(table[1].cells[1].text, "1");
        assert!(doc.blocks.iter().any(|b| b.content.images == ["image.png"]));
        assert!(doc.text().contains("<script>alert(1)</script>"));
        let diagrams = parse(
            "```Mermaid\nflowchart TB\n A[한글] --> B[끝]\n```\n\n```rust\nmermaid\n```\n\n    mermaid\n",
        );
        assert!(matches!(diagrams.blocks[0].kind, Kind::Mermaid(Ok(_))));
        assert_eq!(
            diagrams.blocks[0].content.text,
            "flowchart TB\n A[한글] --> B[끝]\n"
        );
        assert!(
            diagrams.blocks[1..]
                .iter()
                .all(|b| matches!(b.kind, Kind::Code))
        );
        assert!(diagrams.text().contains("A[한글] --> B[끝]"));
        for block in &doc.blocks {
            for inline in std::iter::once(&block.content).chain(&block.cells) {
                assert!(
                    inline
                        .spans
                        .iter()
                        .all(|s| inline.text.get(s.range.clone()).is_some())
                );
                assert!(
                    inline
                        .spans
                        .windows(2)
                        .all(|s| s[0].range.end <= s[1].range.start)
                );
            }
        }
    }
}
