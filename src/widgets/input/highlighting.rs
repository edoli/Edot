//! Cached source analysis and the GPUI styling adapter.
use super::{markdown::*, *};

const BACKGROUND_THRESHOLD: usize = 64 * 1024;

#[derive(Default)]
pub(super) struct Highlighting {
    pub(super) spans: Vec<Span>,
    revision: Option<u64>,
    task: Option<Task<()>>,
}

impl InputState {
    pub fn text_mode(&self) -> TextMode {
        self.text_mode
    }

    pub fn set_text_mode(&mut self, mode: TextMode, cx: &mut Context<Self>) {
        if self.text_mode == mode {
            return;
        }
        self.text_mode = mode;
        self.stop_preview_drag();
        if self.is_read_only() {
            self.finish_composition();
        }
        self.highlighting.spans = Vec::new();
        self.highlighting.revision = None;
        self.layout_key = None;
        self.ensure_highlighting(cx);
        self.ensure_preview(cx);
        cx.notify();
    }

    pub(super) fn ensure_highlighting(&mut self, cx: &mut Context<Self>) {
        if !self.multiline
            || self.text_mode != TextMode::Markdown
            || self.highlighting.revision == Some(self.revision)
        {
            return;
        }
        self.highlighting.spans = Vec::new();
        if self.content.len() <= BACKGROUND_THRESHOLD {
            self.highlighting.spans = markdown::parse(&self.content);
            self.highlighting.revision = Some(self.revision);
            return;
        }
        if self.highlighting.task.is_some() {
            return;
        }
        let mut snapshot = (self.revision, self.content.clone());
        // One worker per editor: coalesce edits made during analysis without
        // accumulating uncancellable parser jobs or displaying stale offsets.
        self.highlighting.task = Some(cx.spawn(async move |this, cx| {
            loop {
                let (revision, content) = snapshot;
                let spans = cx
                    .background_executor()
                    .spawn(async move { markdown::parse(&content) })
                    .await;
                let Ok(next) = this.update(cx, |s, cx| {
                    if s.text_mode != TextMode::Markdown {
                        s.highlighting.task = None;
                        return None;
                    }
                    if s.revision == revision {
                        s.highlighting.spans = spans;
                        s.highlighting.revision = Some(revision);
                        s.layout_key = None;
                        cx.notify();
                    } else if s.highlighting.revision != Some(s.revision) {
                        return Some((s.revision, s.content.clone()));
                    }
                    s.highlighting.task = None;
                    None
                }) else {
                    return;
                };
                let Some(next) = next else {
                    return;
                };
                snapshot = next;
            }
        }));
    }
}

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub link: Hsla,
    pub muted: Hsla,
    pub code: Hsla,
    pub marker: Hsla,
}

/// Intersect document spans and URL ranges with one physical source line.
/// Tab expansion changes run lengths only, never the editor's source offsets.
pub(super) fn text_runs(
    source: Range<usize>,
    tabs: &[(usize, usize, usize)],
    spans: &[Span],
    links: &[Range<usize>],
    base: &TextRun,
    palette: Palette,
) -> Vec<TextRun> {
    let mut syntax = spans.partition_point(|span| span.range.end <= source.start);
    if spans
        .get(syntax)
        .is_none_or(|s| s.range.start >= source.end)
    {
        return links::text_runs(base.len, tabs, links, base, palette.link);
    }
    let mut runs = Vec::new();
    let mut link_index = 0;
    let mut offset = source.start;
    while offset < source.end {
        while spans.get(syntax).is_some_and(|s| s.range.end <= offset) {
            syntax += 1;
        }
        while links
            .get(link_index)
            .is_some_and(|l| source.start + l.end <= offset)
        {
            link_index += 1;
        }
        let mut end = source.end;
        let mut flags = 0;
        if let Some(span) = spans.get(syntax) {
            if span.range.start <= offset {
                flags = span.style;
                end = end.min(span.range.end);
            } else {
                end = end.min(span.range.start);
            }
        }
        let mut url = false;
        if let Some(link) = links.get(link_index) {
            if source.start + link.start <= offset {
                url = true;
                end = end.min(source.start + link.end);
            } else {
                end = end.min(source.start + link.start);
            }
        }
        let mut run = base.clone();
        run.len = layout::display_index(end - source.start, tabs)
            - layout::display_index(offset - source.start, tabs);
        if flags & CODE != 0 {
            run.color = palette.code;
        } else {
            if flags & (QUOTE | RULE) != 0 {
                run.color = palette.muted;
            }
            if flags & MARKER != 0 {
                run.color = palette.marker;
            }
            if flags & HEADING != 0 {
                run.color = palette.link;
            }
            if flags & (HEADING | STRONG) != 0 {
                run.font.weight = FontWeight::BOLD;
            }
            if flags & EMPHASIS != 0 {
                run.font.style = FontStyle::Italic;
            }
            if flags & STRIKE != 0 {
                run.strikethrough = Some(StrikethroughStyle {
                    thickness: px(1.),
                    color: None,
                });
            }
            if flags & LINK != 0 {
                run.color = palette.link;
            }
        }
        // Keep existing URL decoration and Ctrl+click behavior, even in code.
        if url {
            run.color = palette.link;
            run.underline = Some(UnderlineStyle {
                thickness: px(1.),
                color: Some(palette.link),
                wavy: false,
            });
        }
        runs.push(run);
        offset = end;
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::{Palette, TextRun, layout, markdown, text_runs};

    #[test]
    fn styles_keep_tab_expansion_utf8_and_nested_emphasis_aligned() {
        let text = "앞\t**한글 *기울임*** ~~삭제~~ `코드` https://example.org";
        let (display, tabs) = layout::expand_tabs(text);
        let palette = Palette {
            link: gpui::rgb(0x2674f5).into(),
            muted: gpui::rgb(0x777777).into(),
            code: gpui::rgb(0x267f60).into(),
            marker: gpui::rgb(0x985500).into(),
        };
        let base = TextRun {
            len: display.len(),
            ..Default::default()
        };
        let runs = text_runs(
            0..text.len(),
            &tabs,
            &markdown::parse(text),
            &super::super::links::ranges(text),
            &base,
            palette,
        );
        assert_eq!(runs.iter().map(|r| r.len).sum::<usize>(), display.len());
        assert!(
            runs.iter().any(|r| r.font.weight == gpui::FontWeight::BOLD
                && r.font.style == gpui::FontStyle::Italic)
        );
        assert!(runs.iter().any(|r| r.strikethrough.is_some()));
        assert!(runs.iter().any(|r| r.color == palette.code));
        assert_eq!(runs.iter().filter(|r| r.underline.is_some()).count(), 1);

        // A middle line of a fenced block uses document offsets, not line offsets.
        let document = "# 앞부분\r\n\r\n~~~\r\n\t🙂 **코드 원문**\r\n~~~\r\n";
        let start = document.find('\t').unwrap();
        let end = start + document[start..].find('\r').unwrap();
        let (display, tabs) = layout::expand_tabs(&document[start..end]);
        let base = TextRun {
            len: display.len(),
            ..base
        };
        let runs = text_runs(
            start..end,
            &tabs,
            &markdown::parse(document),
            &[],
            &base,
            palette,
        );
        assert_eq!(runs.iter().map(|r| r.len).sum::<usize>(), display.len());
        assert!(
            runs.iter()
                .all(|r| r.color == palette.code && r.font.weight == base.font.weight)
        );
    }

    #[gpui::test]
    fn background_results_follow_latest_revision_without_editing_text(
        cx: &mut gpui::TestAppContext,
    ) {
        use super::{HEADING, InputState, STRONG};
        use gpui::AppContext;

        cx.update(crate::widgets::init);
        let original = "# 이전 제목\r\n\r\n본문 🙂\t줄\r\n".repeat(4000);
        let latest = "**최신 내용**\r\n\r\n본문 🙂\t줄\r\n".repeat(4000);
        let window = cx.add_window(|w, cx| {
            InputState::new(w, cx)
                .soft_wrap(true)
                .default_value(original)
        });
        let editor = window.root(cx).unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.ensure_highlighting(cx); // Initial default_value has revision zero.
                assert!(s.highlighting.task.is_some());
                s.set_value(latest.clone(), w, cx);
                s.set_selected_range(2.."**최신 내용".len(), cx);
                assert!(s.highlighting.spans.is_empty());
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| {
            let s = editor.read(cx);
            assert_eq!(s.value(), latest);
            assert_eq!(s.revision, 1);
            assert_eq!(s.selected_range(), 2.."**최신 내용".len());
            assert_eq!(s.highlighting.revision, Some(s.revision));
            assert!(s.highlighting.task.is_none());
            assert_eq!(s.highlighting.spans[0].style, STRONG);
        });
        // A small edit supersedes a running large-document analysis as well.
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_value("*기울임*\n\n".repeat(9000), w, cx);
                assert!(s.highlighting.task.is_some());
                s.set_value("# 작은 제목", w, cx);
                assert_eq!(s.highlighting.revision, Some(s.revision));
            });
            let single = cx.new(|cx| InputState::new(w, cx));
            single.update(cx, |s, cx| {
                s.set_value("**제목 입력**", w, cx);
                assert!(s.highlighting.spans.is_empty());
                assert!(s.highlighting.task.is_none());
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| {
            let s = editor.read(cx);
            assert_eq!(s.value(), "# 작은 제목");
            assert_eq!(s.revision, 3);
            assert_eq!(s.highlighting.revision, Some(3));
            assert_eq!(s.highlighting.spans[0].style, HEADING);
            assert!(s.highlighting.task.is_none());
        });

        // Turning Markdown off must also discard a result already in flight.
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_value("**큰 본문**\r\n\r\n".repeat(9000), w, cx);
                assert!(s.highlighting.task.is_some());
                s.set_text_mode(super::TextMode::PlainText, cx);
                assert!(s.highlighting.spans.is_empty());
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| {
            let s = editor.read(cx);
            assert_eq!(s.revision, 4);
            assert!(s.highlighting.spans.is_empty());
            assert!(s.highlighting.task.is_none());
            assert_eq!(s.highlighting.revision, None);
        });
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_value("**일반 텍스트**\r\n\r\n".repeat(9000), w, cx);
                assert!(s.highlighting.task.is_none());
                s.set_text_mode(super::TextMode::Markdown, cx);
                assert!(s.highlighting.task.is_some());
                s.set_text_mode(super::TextMode::PlainText, cx);
                s.set_text_mode(super::TextMode::Markdown, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| {
            let s = editor.read(cx);
            assert_eq!(s.revision, 5);
            assert_eq!(s.highlighting.revision, Some(5));
            assert_eq!(s.highlighting.spans[0].style, STRONG);
            assert!(s.highlighting.task.is_none());
        });
    }
}
