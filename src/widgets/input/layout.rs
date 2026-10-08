//! Text shaping, content scrolling, painting and scrollbar geometry.
use super::*;

impl Paragraph {
    fn display_index(&self, byte: usize) -> usize {
        byte + self
            .tabs
            .iter()
            .filter(|(source, _, _)| *source < byte)
            .map(|(_, _, spaces)| spaces - 1)
            .sum::<usize>()
    }
    pub(super) fn position(&self, byte: usize, height: Pixels) -> Point<Pixels> {
        self.line
            .position_for_index(self.display_index(byte), height)
            .unwrap_or_default()
    }
    pub(super) fn position_after_wrap(&self, byte: usize, height: Pixels) -> Point<Pixels> {
        let pos = self.position(byte, height);
        let row = (pos.y / height).round() as usize;
        if self.line.wrap_boundaries.get(row).is_some_and(|boundary| {
            self.line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index
                == self.display_index(byte)
        }) {
            // The same byte is both the preceding row's end and this row's start.
            point(px(0.), pos.y + height)
        } else {
            pos
        }
    }
    pub(super) fn source_index(&self, display: usize) -> usize {
        let mut removed = 0;
        for &(source, start, spaces) in &self.tabs {
            if display < start {
                break;
            }
            if display < start + spaces {
                return source;
            }
            removed += spaces - 1;
        }
        display.saturating_sub(removed).min(self.end - self.start)
    }
}
fn expand_tabs(text: &str) -> (SharedString, Vec<(usize, usize, usize)>) {
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
    pub(super) fn layout(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
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
        let key = (self.revision, width, font_size, style.font(), color);
        if self.layout_key.as_ref() != Some(&key) {
            self.lines.clear();
            let mut start = 0;
            let mut top = self.content_padding.top;
            let mut max_width = px(0.);
            loop {
                let end = display[start..]
                    .find(['\r', '\n'])
                    .map_or(display.len(), |i| start + i);
                let (value, tabs) = expand_tabs(&display[start..end]);
                let run = TextRun {
                    len: value.len(),
                    font: style.font(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                if let Ok(lines) = window.text_system().shape_text(
                    value,
                    font_size,
                    &[run],
                    self.wrap.then_some(width),
                    None,
                ) {
                    for line in lines {
                        max_width = max_width.max(line.width());
                        let height = line.size(self.line_height).height;
                        self.lines.push(Paragraph {
                            start,
                            end,
                            top,
                            line,
                            tabs: tabs.clone(),
                        });
                        top += height;
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
            self.content_size = size(
                if self.wrap {
                    bounds.size.width
                } else {
                    self.content_padding.left + max_width + self.content_padding.right.max(px(4.))
                },
                top.max(self.content_padding.top + self.line_height) + self.content_padding.bottom,
            );
            self.layout_key = Some(key);
        }
        if self.reveal {
            let caret = self.caret_position();
            if self.cursor() == 0 {
                self.scroll = Point::default();
            }
            if caret.y + self.scroll.y < px(0.) {
                self.scroll.y = -(caret.y - self.content_padding.top).max(px(0.));
            }
            if caret.y + self.line_height + self.scroll.y > bounds.size.height {
                self.scroll.y = bounds.size.height - caret.y - self.line_height;
            }
            if caret.x + self.scroll.x < px(0.) {
                self.scroll.x = -(caret.x - self.content_padding.left).max(px(0.));
            }
            if caret.x + px(3.) + self.scroll.x > bounds.size.width {
                self.scroll.x = bounds.size.width - caret.x - px(3.);
            }
            self.reveal = false;
        }
        self.clamp_scroll();
    }
    pub(super) fn paint(&mut self, window: &mut Window, cx: &mut App) {
        let origin = self.bounds.origin + self.scroll;
        let focused = self.focus.is_focused(window);
        let selection_color = cx.theme().selection;
        let first = self
            .lines
            .partition_point(|l| l.top + l.line.size(self.line_height).height < -self.scroll.y);
        for l in &self.lines[first..] {
            if l.top + self.scroll.y > self.bounds.size.height {
                break;
            }
            let height = l.line.size(self.line_height).height;
            if l.top + height + self.scroll.y < px(0.)
                || l.top + self.scroll.y > self.bounds.size.height
            {
                continue;
            }
            let pos = origin + point(self.content_padding.left, l.top);
            for selection in self.selected_ranges() {
                let start = selection.start.max(l.start);
                let end = selection.end.min(l.end);
                if !selection.is_empty()
                    && start <= end
                    && selection.end > l.start
                    && selection.start <= l.end
                {
                    let a = l.position_after_wrap(start - l.start, self.line_height);
                    let b = l.position(end - l.start, self.line_height);
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
                                size((right - x).max(px(2.)), self.line_height),
                            ),
                            selection_color,
                        ));
                        y += self.line_height;
                    }
                }
            }
            let _ = l
                .line
                .paint(pos, self.line_height, TextAlign::Left, None, window, cx);
            for marked in self.extra_marked.iter().chain(self.marked.iter()) {
                let start = marked.start.max(l.start);
                let end = marked.end.min(l.end);
                if start < end {
                    let a = l.position_after_wrap(start - l.start, self.line_height);
                    let b = l.position(end - l.start, self.line_height);
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
                                pos + point(x, y + self.line_height - px(2.)),
                                size((right - x).max(px(1.)), px(1.)),
                            ),
                            cx.theme().foreground,
                        ));
                        y += self.line_height;
                    }
                }
            }
        }
        if focused && self.blink {
            for selection in &self.extra {
                let at = if selection.reversed {
                    selection.range.start
                } else {
                    selection.range.end
                };
                window.paint_quad(fill(
                    Bounds::new(
                        origin
                            + self.position_with_wrap_side(
                                at,
                                selection.after_wrap
                                    || selection.reversed && !selection.range.is_empty(),
                            ),
                        size(px(1.5), self.line_height),
                    ),
                    cx.theme().foreground,
                ));
            }
            window.paint_quad(fill(
                Bounds::new(
                    origin + self.caret_position(),
                    size(px(1.5), self.line_height),
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
