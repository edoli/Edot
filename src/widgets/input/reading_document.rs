//! Rendered Markdown data, independent of GPUI and the source editor.
use super::markdown::{CODE, EMPHASIS, LINK, STRIKE, STRONG, Span};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::{ops::Range, sync::Arc};

#[derive(Default)]
pub(super) struct Inline {
    pub text: String,
    pub spans: Vec<Span>,
    pub links: Vec<(Range<usize>, String)>,
    pub images: Vec<ReadingImage>,
    pub source: Range<usize>,
    pub pieces: Vec<SourcePiece>,
    pub cell: Option<TableCell>,
}

#[derive(Clone)]
pub(super) struct TableCell {
    pub source: Range<usize>,
    pub text: Range<usize>,
}

#[derive(Clone)]
pub(super) struct SourcePiece {
    pub source: Range<usize>,
    pub display: Range<usize>,
    pub verbatim: bool,
}
pub(super) struct ReadingImage {
    pub source: String,
    pub alt: String,
    pub source_range: Range<usize>,
}
impl Inline {
    fn push(&mut self, text: &str, style: u16, mut source: Range<usize>, original: &str) {
        let start = self.text.len();
        self.text.push_str(text);
        if !text.is_empty() {
            if style & CODE != 0
                && let Some(at) = original[source.clone()].find(text)
            {
                source.start += at;
                source.end = source.start + text.len();
            }
            if original[source.clone()].contains("\r\n")
                && original[source.clone()].replace("\r\n", "\n") == text
            {
                let mut source_start = source.start;
                let mut display_start = start;
                for (index, _) in original[source.clone()].match_indices("\r\n") {
                    let end = source.start + index;
                    let display_end = display_start + end - source_start;
                    self.add_piece(source_start..end, display_start..display_end, true);
                    self.add_piece(end..end + 2, display_end..display_end + 1, false);
                    source_start = end + 2;
                    display_start = display_end + 1;
                }
                self.add_piece(
                    source_start..source.end,
                    display_start..self.text.len(),
                    true,
                );
            } else {
                let verbatim = original[source.clone()] == *text;
                self.add_piece(source, start..self.text.len(), verbatim);
            }
        }
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

    fn add_piece(&mut self, source: Range<usize>, display: Range<usize>, verbatim: bool) {
        if !display.is_empty() {
            self.pieces.push(SourcePiece {
                source,
                display,
                verbatim,
            });
        }
    }
}
pub(super) enum Kind {
    Paragraph,
    Heading(u8),
    Code,
    Mermaid(Result<Arc<super::mermaid::Graph>, &'static str>),
    Rule,
    TableRow {
        header: bool,
        last: bool,
        alignment: Vec<Align>,
    },
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Align {
    Left,
    Center,
    Right,
}
pub(super) struct Block {
    pub kind: Kind,
    pub content: Inline,
    pub cells: Vec<Inline>,
    pub prefix: String,
    pub task_marker: Option<usize>,
    pub indent: usize,
    pub quote: usize,
    pub source: Range<usize>,
}
#[derive(Default)]
pub(super) struct Document {
    pub blocks: Vec<Block>,
}
struct Builder<'a> {
    original: &'a str,
    document: Document,
    current: Option<Block>,
    lists: Vec<Option<u64>>,
    quote: usize,
    prefix: String,
    counts: [u32; 4],
    link: Option<(usize, String)>,
    image: Option<(usize, ReadingImage)>,
    table: Vec<Align>,
    row: Option<Block>,
}
impl Builder<'_> {
    fn flush(&mut self) {
        if let Some(mut block) = self.current.take() {
            if matches!(block.kind, Kind::Mermaid(_)) {
                block.kind =
                    Kind::Mermaid(super::mermaid::parse(&block.content.text).map(Arc::new));
            }
            if let Some(row) = &mut self.row {
                row.cells.push(block.content);
            } else {
                if matches!(block.kind, Kind::Paragraph | Kind::Heading(_))
                    && !block.content.pieces.is_empty()
                {
                    let source = &block.content.source;
                    let line = source.start
                        + self.original[source.clone()]
                            .trim_end_matches(['\r', '\n'])
                            .len();
                    let end = self.original[line..]
                        .find(['\r', '\n'])
                        .map_or(self.original.len(), |length| line + length);
                    let start = self.original[..end]
                        .trim_end_matches([' ', '\t'])
                        .len()
                        .max(block.content.pieces.last().unwrap().source.end);
                    if start < end {
                        block.content.push(
                            &self.original[start..end],
                            0,
                            start..end,
                            self.original,
                        );
                        block.content.source.end = block.content.source.end.max(end);
                        block.source.end = block.source.end.max(end);
                    }
                }
                self.document.blocks.push(block);
            }
        }
    }
    fn start(&mut self, kind: Kind, range: Range<usize>) {
        self.flush();
        self.current = Some(Block {
            kind,
            content: Inline {
                source: range.clone(),
                ..Default::default()
            },
            cells: Vec::new(),
            prefix: std::mem::take(&mut self.prefix),
            task_marker: None,
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
        block.content.source.end = block.source.end;
        block
    }
    fn push(&mut self, text: &str, range: Range<usize>, extra: u16, original: &str) {
        let mut style = extra;
        for (count, bit) in self.counts.iter().zip([STRONG, EMPHASIS, STRIKE, LINK]) {
            if *count > 0 {
                style |= bit;
            }
        }
        let preserve_indent = self.row.is_none() && self.quote == 0 && self.lists.is_empty();
        let block = self.ensure(range.clone());
        let inline = &mut block.content;
        if preserve_indent && matches!(block.kind, Kind::Paragraph) {
            let start = if inline.text.ends_with('\n') {
                inline.pieces.last().map(|piece| piece.source.end)
            } else if inline.pieces.is_empty() && inline.images.is_empty() {
                Some(
                    original[..range.start]
                        .rfind(['\r', '\n'])
                        .map_or(0, |byte| byte + 1),
                )
            } else {
                None
            };
            if let Some(start) = start
                && let Some(leading) = original.get(start..range.start)
            {
                let end = start + leading.len() - leading.trim_start_matches([' ', '\t']).len();
                if start < end {
                    inline.source.start = inline.source.start.min(start);
                    block.source.start = block.source.start.min(start);
                    inline.push(&original[start..end], 0, start..end, original);
                }
            }
        }
        inline.push(text, style, range, original);
    }

    fn line_break(&mut self, mut range: Range<usize>) {
        let end = range.start
            + self.original[range.clone()]
                .trim_end_matches(['\r', '\n'])
                .len();
        let covered = self
            .current
            .as_ref()
            .and_then(|block| block.content.pieces.last())
            .map_or(range.start, |piece| piece.source.end);
        let start = self.original[..end]
            .trim_end_matches([' ', '\t'])
            .len()
            .max(covered);
        if start < end {
            self.push(&self.original[start..end], start..end, 0, self.original);
            range.start = end;
        }
        self.push("\n", range, 0, self.original);
    }
    fn row(&mut self, header: bool, range: Range<usize>) {
        self.start(
            Kind::TableRow {
                header,
                last: false,
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

#[cfg(test)]
fn parse(text: &str) -> Document {
    parse_with(text, |_, _| {})
}

pub(super) fn parse_with(
    text: &str,
    mut observe: impl FnMut(&Event<'_>, &Range<usize>),
) -> Document {
    let mut b = Builder {
        original: text,
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
        observe(&event, &range);
        // Image descriptions belong to the image, not the visible paragraph.
        // Ignore their formatting so it cannot leak into surrounding text.
        if let Some((depth, image)) = &mut b.image {
            match event {
                Event::Start(Tag::Image { .. }) => *depth += 1,
                Event::End(TagEnd::Image) => *depth -= 1,
                Event::Text(value)
                | Event::Code(value)
                | Event::Html(value)
                | Event::InlineHtml(value) => image.alt.push_str(&value),
                Event::SoftBreak | Event::HardBreak => image.alt.push(' '),
                _ => {}
            }
            if *depth == 0 {
                let (_, image) = b.image.take().unwrap();
                b.ensure(range).content.images.push(image);
            }
            continue;
        }
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
                    b.ensure(range.clone());
                    b.image = Some((
                        1,
                        ReadingImage {
                            source: dest_url.to_string(),
                            alt: String::new(),
                            source_range: range,
                        },
                    ));
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
                Tag::TableCell => {
                    let cell = b.row.as_ref().and_then(|row| {
                        let line_end = text[row.source.start..]
                            .find(['\r', '\n'])
                            .map_or(text.len(), |end| row.source.start + end);
                        if range.start >= line_end {
                            return None;
                        }
                        let end = range.end.min(line_end);
                        let raw = &text[range.start..end];
                        let trimmed = raw.trim_matches([' ', '\t']);
                        let contents = if trimmed.is_empty() {
                            end..end
                        } else {
                            let start =
                                range.start + raw.len() - raw.trim_start_matches([' ', '\t']).len();
                            start..start + trimmed.len()
                        };
                        Some(TableCell {
                            source: range.start..end,
                            text: contents,
                        })
                    });
                    b.start(Kind::Paragraph, range);
                    b.current.as_mut().unwrap().content.cell = cell;
                }
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
                    if let Some((start, target)) = b.link.take()
                        && super::links::is_markdown_target(&target)
                    {
                        let inline = &mut b.ensure(range).content;
                        if start < inline.text.len() {
                            inline.links.push((start..inline.text.len(), target));
                        }
                    }
                    b.counts[3] -= 1;
                }
                TagEnd::TableHead | TagEnd::TableRow => b.finish_row(),
                TagEnd::Table => {
                    b.finish_row();
                    if let Some(Block {
                        kind: Kind::TableRow { last, .. },
                        ..
                    }) = b.document.blocks.last_mut()
                    {
                        *last = true;
                    }
                    b.table.clear();
                }
                _ => {}
            },
            Event::Text(value) | Event::Html(value) | Event::InlineHtml(value) => {
                b.push(&value, range, 0, text)
            }
            Event::Code(value) => b.push(&value, range, CODE, text),
            Event::SoftBreak | Event::HardBreak => b.line_break(range),
            Event::TaskListMarker(checked) => {
                let marker = range.start;
                let valid = matches!(text.get(marker..marker + 3), Some("[ ]" | "[x]" | "[X]"));
                let block = b.ensure(range);
                block.prefix = if checked { "☑" } else { "☐" }.into();
                block.task_marker = valid.then_some(marker + 1);
            }
            Event::Rule => {
                b.start(Kind::Rule, range);
                b.flush();
            }
            _ => {}
        }
    }
    b.flush();
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
        assert!(
            p.spans
                .iter()
                .any(|span| span.style & LINK != 0 && &p.text[span.range.clone()] == "링크")
        );
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
        let image = doc
            .blocks
            .iter()
            .find(|b| !b.content.images.is_empty())
            .unwrap();
        assert_eq!(image.content.images[0].source, "image.png");
        assert_eq!(image.content.images[0].alt, "그림");
        assert!(image.content.text.is_empty());
        let images = parse(
            "앞 **![한글 *설명* `코드` &amp; ![안쪽](inner.png)](<images/공백 이름.png>)뒤**\n\n![](empty.png)",
        );
        let paragraph = &images.blocks[0].content;
        assert_eq!(paragraph.text, "앞 뒤");
        assert_eq!(paragraph.images.len(), 1);
        assert_eq!(paragraph.images[0].source, "images/공백 이름.png");
        assert_eq!(paragraph.images[0].alt, "한글 설명 코드 & 안쪽");
        assert_eq!(paragraph.spans[0].range, "앞 ".len().."앞 뒤".len());
        assert_eq!(paragraph.spans[0].style, STRONG);
        assert!(images.blocks[1].content.text.is_empty());
        assert!(images.blocks[1].content.images[0].alt.is_empty());
        assert!(
            doc.blocks
                .iter()
                .any(|block| block.content.text.contains("<script>alert(1)</script>"))
        );
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
