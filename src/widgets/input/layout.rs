//! Text shaping, content scrolling, painting and scrollbar geometry.
use super::*;

pub(super) fn display_index(byte: usize, tabs: &[(usize, usize, usize)]) -> usize {
    byte + tabs
        .iter()
        .filter(|(source, _, _)| *source < byte)
        .map(|(_, _, spaces)| spaces - 1)
        .sum::<usize>()
}

impl Paragraph {
    fn display_index(&self, byte: usize) -> usize {
        self.display.display_index(byte)
    }
    pub(super) fn height(&self) -> Pixels {
        if self.compact {
            return px(0.);
        }
        self.rich.map_or_else(
            || self.line.size(self.line_height).height,
            |rich| rich.height,
        )
    }
    pub(super) fn position(&self, byte: usize) -> Point<Pixels> {
        if let Some(rich) = self.rich {
            let fraction = byte as f32 / (self.end - self.start).max(1) as f32;
            return point(
                px(0.),
                (rich.height - self.line_height).max(px(0.)) * fraction,
            );
        }
        self.line
            .position_for_index(self.display_index(byte), self.line_height)
            .unwrap_or_default()
    }
    pub(super) fn position_after_wrap(&self, byte: usize) -> Point<Pixels> {
        let pos = self.position(byte);
        let row = (pos.y / self.line_height).round() as usize;
        if self.line.wrap_boundaries.get(row).is_some_and(|boundary| {
            self.line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index
                == self.display_index(byte)
        }) {
            // The same byte is both the preceding row's end and this row's start.
            point(px(0.), pos.y + self.line_height)
        } else {
            pos
        }
    }
    pub(super) fn source_index(&self, display: usize) -> usize {
        self.display
            .source_index(display)
            .min(self.end - self.start)
    }
}
pub(super) fn expand_tabs(text: &str) -> (SharedString, Vec<(usize, usize, usize)>) {
    if !text.contains('\t') {
        return (text.to_owned().into(), vec![]);
    }
    let mut display = String::new();
    let mut tabs = vec![];
    let mut column = 0;
    for (byte, ch) in text.char_indices() {
        if ch == '\t' {
            let spaces = 4 - column % 4;
            tabs.push((byte, display.len(), spaces));
            display.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            display.push(ch);
            column += 1;
        }
    }
    (display.into(), tabs)
}

impl InputState {
    pub(super) fn top_source_anchor(&self) -> Option<(usize, Pixels)> {
        let first = self
            .lines
            .partition_point(|row| row.top + row.height() <= -self.scroll.y);
        self.lines
            .get(first)
            .map(|row| (row.start, row.top + self.scroll.y))
    }

    pub(super) fn restore_top_source_anchor(&mut self, anchor: (usize, Pixels)) {
        let (source, y) = anchor;
        let index = self
            .lines
            .partition_point(|row| row.start <= source)
            .saturating_sub(1);
        if let Some(row) = self.lines.get(index) {
            self.scroll.y = y - row.top;
        }
    }

    pub(super) fn remember_projection_anchor(&mut self) {
        if self
            .layout_key
            .as_ref()
            .is_some_and(|key| key.0 == self.revision)
        {
            self.projection_anchor = self.top_source_anchor();
        }
    }

    pub(super) fn row_at_y(&self, y: Pixels) -> Option<&Paragraph> {
        self.lines.get(
            self.lines
                .partition_point(|row| row.top <= y)
                .saturating_sub(1),
        )
    }

    pub(super) fn clamp_scroll(&mut self) {
        self.scroll.x = self
            .scroll
            .x
            .min(px(0.))
            .max(-(self.content_size.width - self.bounds.size.width).max(px(0.)));
        self.scroll.y = self
            .scroll
            .y
            .min(px(0.))
            .max(-(self.content_size.height - self.bounds.size.height).max(px(0.)));
    }
    pub(super) fn layout(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_highlighting(cx);
        self.ensure_visual(cx);
        if self.selection.is_empty() && !self.is_composing() {
            let at = self.table_caret(self.cursor(), self.cursor());
            self.selection = at..at;
        }
        self.bounds = bounds;
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        self.line_height = (font_size * 1.5).max(px(18.));
        if !self.multiline {
            let top = ((bounds.size.height - self.line_height) / 2.).max(px(0.));
            if self.content_padding.top != top {
                self.content_padding.top = top;
                self.layout_key = None;
            }
        }
        let width = if self.wrap {
            (bounds.size.width - self.content_padding.left - self.content_padding.right).max(px(1.))
        } else {
            px(1e8)
        };
        let display = if self.content.is_empty() {
            self.placeholder.clone()
        } else {
            self.content.clone()
        };
        let color = if self.content.is_empty() {
            style.color.opacity(0.5)
        } else {
            style.color
        };
        let link_color = cx.theme().primary;
        let palette = highlighting::Palette {
            link: link_color,
            muted: cx.theme().muted_foreground,
            code: if cx.theme().background.l < 0.5 {
                rgb(0x98c379).into()
            } else {
                rgb(0x267f60).into()
            },
            marker: cx.theme().warning,
        };
        let key = (
            self.revision,
            width,
            font_size,
            style.font(),
            color,
            link_color,
        );
        let revealed = self.visual_reveal_points();
        let metrics_match = self.layout_key.as_ref() == Some(&key);
        let mut rich_style_changed = false;
        if let Some(visual) = &mut self.visual {
            let rich_style = (bounds.size.width, font_size, style.font());
            if visual.rich_style.as_ref() != Some(&rich_style) {
                visual.rich_style = Some(rich_style);
                visual.rich_heights.fill(None);
                visual.rich_width = px(0.);
                rich_style_changed = true;
            }
        }
        if let Some(start) = self.visual.as_ref().and_then(|visual| visual.raw_block)
            && !self.selecting
            && self.visual_document().is_some_and(|document| {
                document.rich_at(start).is_some_and(|(_, source)| {
                    self.cursor() < source.start || self.cursor() > source.end
                }) || document.rich_at(start).is_none() && self.cursor() < start
            })
        {
            self.remember_projection_anchor();
            self.visual.as_mut().unwrap().raw_block = None;
            self.layout_key = None;
        }
        let metrics_match = metrics_match && self.layout_key.is_some();
        let visual_changed = self
            .visual
            .as_ref()
            .is_some_and(|visual| visual.revealed != revealed);
        if !metrics_match || visual_changed || rich_style_changed {
            let scroll_anchor = self.projection_anchor.take().or_else(|| {
                if (visual_changed || rich_style_changed)
                    && self
                        .layout_key
                        .as_ref()
                        .is_some_and(|key| key.0 == self.revision)
                {
                    self.top_source_anchor()
                } else {
                    None
                }
            });
            let mut output_lines = Vec::with_capacity(self.lines.len());
            let mut previous = std::mem::take(&mut self.lines).into_iter().peekable();
            let visual = self.visual_document();
            let mut start = 0;
            let mut top = if visual.is_some() {
                px(0.)
            } else {
                self.content_padding.top
            };
            let mut max_width = px(0.);
            loop {
                let rich = visual.and_then(|visual| visual.rich_at(start)).filter(|_| {
                    self.visual
                        .as_ref()
                        .is_none_or(|visual| visual.raw_block != Some(start))
                });
                let end = rich.as_ref().map_or_else(
                    || {
                        display[start..]
                            .find(['\r', '\n'])
                            .map_or(display.len(), |i| start + i)
                    },
                    |(_, source)| source.end,
                );
                let compact = rich.is_none() && self.is_table_separator(start);
                let rich = rich.map(|(index, _)| visual::RichRow {
                    index,
                    height: self.visual.as_ref().unwrap().rich_heights[index]
                        .unwrap_or(self.line_height * 3.),
                });
                while previous.peek().is_some_and(|row| row.start < start) {
                    previous.next();
                }
                if metrics_match
                    && !rich_style_changed
                    && previous.peek().is_some_and(|row| {
                        row.start == start
                            && row.end == end
                            && row.compact == compact
                            && row.rich.map(|row| row.index) == rich.map(|row| row.index)
                    })
                {
                    let mut row = previous.next().unwrap();
                    row.top = top;
                    top += row.height();
                    if row.rich.is_none() {
                        max_width = max_width.max(row.line.width());
                    }
                    output_lines.push(row);
                } else {
                    let (value, tabs) = if rich.is_some() {
                        ("".into(), Vec::new())
                    } else {
                        expand_tabs(&display[start..end])
                    };
                    let map = visual::DisplayMap::from_tabs(&tabs);
                    let links = if self.multiline && !self.content.is_empty() && rich.is_none() {
                        links::ranges(&display[start..end])
                    } else {
                        Vec::new()
                    };
                    let run = TextRun {
                        len: value.len(),
                        font: style.font(),
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let runs = if rich.is_some() {
                        Vec::new()
                    } else {
                        highlighting::text_runs(
                            start..end,
                            &tabs,
                            visual.map_or(self.highlighting.spans_for(self.revision), |document| {
                                &document.spans
                            }),
                            &links,
                            &run,
                            palette,
                        )
                    };
                    if let Ok(lines) = window.text_system().shape_text(
                        value,
                        font_size,
                        if runs.is_empty() {
                            std::slice::from_ref(&run)
                        } else {
                            &runs
                        },
                        self.wrap.then_some(width),
                        None,
                    ) {
                        for line in lines {
                            if rich.is_none() {
                                max_width = max_width.max(line.width());
                            }
                            let row = Paragraph {
                                start,
                                end,
                                top,
                                line,
                                display: map.clone(),
                                links: links.clone(),
                                compact,
                                line_height: self.line_height,
                                rich,
                            };
                            top += row.height();
                            output_lines.push(row);
                        }
                    }
                }
                if end == display.len() {
                    break;
                }
                start = end
                    + if display[end..].starts_with("\r\n") {
                        2
                    } else {
                        1
                    };
            }
            self.lines = output_lines;
            if let Some(visual) = &mut self.visual {
                visual.revealed = revealed;
            }
            if let Some(anchor) = scroll_anchor {
                self.restore_top_source_anchor(anchor);
            }
            self.content_size = size(
                if self.wrap {
                    bounds.size.width
                } else {
                    let source_width = self.content_padding.left
                        + max_width
                        + self.content_padding.right.max(px(4.));
                    self.visual
                        .as_ref()
                        .map_or(source_width, |visual| source_width.max(visual.rich_width))
                },
                top.max(self.content_padding.top + self.line_height) + self.content_padding.bottom,
            );
            self.layout_key = Some(key);
        }
        if self.visual_document().is_none() {
            if self.reveal {
                if self.cursor() == 0 {
                    self.scroll = Point::default();
                }
                self.scroll = self.scroll_to_caret(self.caret_position(), self.line_height);
                self.reveal = false;
            }
            self.clamp_scroll();
        }
    }

    pub(super) fn scroll_to_caret(&self, caret: Point<Pixels>, height: Pixels) -> Point<Pixels> {
        let mut scroll = self.scroll;
        if caret.y + scroll.y < px(0.) {
            scroll.y = -(caret.y - self.content_padding.top).max(px(0.));
        }
        if caret.y + height + scroll.y > self.bounds.size.height {
            scroll.y = self.bounds.size.height - caret.y - height;
        }
        if caret.x + scroll.x < px(0.) {
            scroll.x = -(caret.x - self.content_padding.left).max(px(0.));
        }
        if caret.x + px(3.) + scroll.x > self.bounds.size.width {
            scroll.x = self.bounds.size.width - caret.x - px(3.);
        }
        scroll
    }

    pub(super) fn row_for_source(&self, at: usize) -> Option<&Paragraph> {
        let index = self
            .lines
            .partition_point(|row| row.start <= at)
            .saturating_sub(1);
        self.lines.get(index)
    }

    pub(super) fn paint(&mut self, window: &mut Window, cx: &mut App) {
        let origin = self.bounds.origin + self.scroll;
        let focused = self.focus.is_focused(window);
        let selection_color = cx.theme().selection;
        let drop_caret = self
            .drop_caret()
            .filter(|_| !self.is_read_only())
            .map(|at| origin + at);
        if let Some(at) = drop_caret {
            // A quiet row guide sits behind the text and existing selections.
            window.paint_quad(
                fill(
                    Bounds::new(
                        point(self.bounds.left() + px(4.), at.y + px(1.)),
                        size(
                            (self.bounds.size.width - px(8.)).max(px(0.)),
                            (self.line_height - px(2.)).max(px(0.)),
                        ),
                    ),
                    cx.theme().primary.opacity(0.06),
                )
                .corner_radii(px(3.)),
            );
        }
        let first = self
            .lines
            .partition_point(|l| l.top + l.height() < -self.scroll.y);
        for l in &self.lines[first..] {
            if l.top + self.scroll.y > self.bounds.size.height {
                break;
            }
            let height = l.height();
            if l.top + height + self.scroll.y < px(0.)
                || l.top + self.scroll.y > self.bounds.size.height
            {
                continue;
            }
            if l.rich.is_some() {
                if self
                    .visual
                    .as_ref()
                    .is_some_and(|visual| !visual.geometry.has_text(l.start))
                    && self.selected_ranges().any(|range| {
                        if self
                            .visual
                            .as_ref()
                            .is_some_and(|visual| visual.geometry.has_diagram_labels(l.start))
                        {
                            range.start <= l.start && l.end <= range.end
                        } else {
                            range.start < l.end && range.end > l.start
                        }
                    })
                {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(self.bounds.left(), origin.y + l.top),
                            size(self.bounds.size.width, height),
                        ),
                        selection_color.opacity(0.35),
                    ));
                }
                continue;
            }
            if l.compact {
                continue;
            }
            let pos = origin + point(self.content_padding.left, l.top);
            let line_height = l.line_height;
            for selection in self.selected_ranges() {
                let start = selection.start.max(l.start);
                let end = selection.end.min(l.end);
                if !selection.is_empty()
                    && start <= end
                    && selection.end > l.start
                    && selection.start <= l.end
                {
                    let a = l.position_after_wrap(start - l.start);
                    let b = l.position(end - l.start);
                    let mut y = a.y;
                    while y <= b.y {
                        let x = if y == a.y { a.x } else { px(0.) };
                        let right = if y == b.y && selection.end <= l.end {
                            b.x
                        } else {
                            self.bounds.size.width - self.scroll.x - self.content_padding.left
                        };
                        window.paint_quad(fill(
                            Bounds::new(
                                pos + point(x, y),
                                size((right - x).max(px(2.)), line_height),
                            ),
                            selection_color,
                        ));
                        y += line_height;
                    }
                }
            }
            let _ = l
                .line
                .paint(pos, line_height, TextAlign::Left, None, window, cx);
            for marked in self.extra_marked.iter().chain(self.marked.iter()) {
                let start = marked.start.max(l.start);
                let end = marked.end.min(l.end);
                if start < end {
                    let a = l.position_after_wrap(start - l.start);
                    let b = l.position(end - l.start);
                    let mut y = a.y;
                    while y <= b.y {
                        let x = if y == a.y { a.x } else { px(0.) };
                        let right = if y == b.y {
                            b.x
                        } else {
                            self.bounds.size.width - self.scroll.x - self.content_padding.left
                        };
                        window.paint_quad(fill(
                            Bounds::new(
                                pos + point(x, y + line_height - px(2.)),
                                size((right - x).max(px(1.)), px(1.)),
                            ),
                            cx.theme().foreground,
                        ));
                        y += line_height;
                    }
                }
            }
        }
        if let Some(at) = drop_caret {
            let at = at + point(px(0.), px(2.));
            let height = (self.line_height - px(4.)).max(px(2.));
            let color = cx.theme().primary;
            window.paint_quad(
                fill(
                    Bounds::new(at - point(px(3.), px(0.)), size(px(6.), height)),
                    color.opacity(0.12),
                )
                .corner_radii(px(3.)),
            );
            window.paint_quad(
                fill(
                    Bounds::new(at - point(px(1.), px(0.)), size(px(2.), height)),
                    color,
                )
                .corner_radii(px(1.)),
            );
        }
        if self.drop_position.is_none() && focused && self.blink && !self.is_read_only() {
            for selection in &self.extra {
                let at = if selection.reversed {
                    selection.range.start
                } else {
                    selection.range.end
                };
                window.paint_quad(fill(
                    Bounds::new(
                        origin + self.position_with_wrap_side(at, selection.after_wrap),
                        size(px(1.5), self.visual_caret_height(at)),
                    ),
                    cx.theme().foreground,
                ));
            }
            window.paint_quad(fill(
                Bounds::new(
                    origin + self.caret_position(),
                    size(px(1.5), self.visual_caret_height(self.cursor())),
                ),
                cx.theme().foreground,
            ));
        }
        for axis in [false, true] {
            if let Some((track, thumb)) = self.scrollbar_geometry(axis) {
                let active = self
                    .scroll_drag
                    .is_some_and(|(drag_axis, _, _)| drag_axis == axis);
                let hovered = self.scroll_hover == Some(axis);
                if active || hovered {
                    window.paint_quad(fill(track, cx.theme().primary.opacity(0.1)));
                }
                window.paint_quad(fill(
                    thumb,
                    if active || hovered {
                        cx.theme().primary
                    } else {
                        cx.theme().muted_foreground.opacity(0.5)
                    },
                ));
            }
        }
    }
    pub(super) fn scrollbar_geometry(
        &self,
        horizontal: bool,
    ) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
        if !self.multiline || (horizontal && self.wrap) {
            return None;
        }
        let viewport = if horizontal {
            self.bounds.size.width
        } else {
            self.bounds.size.height
        };
        let content = if horizontal {
            self.content_size.width
        } else {
            self.content_size.height
        };
        if content <= viewport || viewport <= px(0.) {
            return None;
        }
        let length = (viewport * (viewport / content)).max(px(24.)).min(viewport);
        let offset = if horizontal {
            self.scroll.x
        } else {
            self.scroll.y
        };
        let pos = (viewport - length) * (-offset / (content - viewport));
        Some(if horizontal {
            (
                Bounds::new(
                    point(self.bounds.left(), self.bounds.bottom() - px(10.)),
                    size(viewport, px(10.)),
                ),
                Bounds::new(
                    point(self.bounds.left() + pos, self.bounds.bottom() - px(8.)),
                    size(length, px(6.)),
                ),
            )
        } else {
            (
                Bounds::new(
                    point(self.bounds.right() - px(10.), self.bounds.top()),
                    size(px(10.), viewport),
                ),
                Bounds::new(
                    point(self.bounds.right() - px(8.), self.bounds.top() + pos),
                    size(px(6.), length),
                ),
            )
        })
    }
}
