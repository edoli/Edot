//! Grapheme/visual navigation and mouse selection.
use super::*;

impl InputState {
    /// Plain-text word boundaries, including VS Code's decimal-number rule.
    pub(super) fn word_at_cursor(&self) -> Option<Range<usize>> {
        self.word_at(self.cursor())
    }
    pub(super) fn word_at(&self, cursor: usize) -> Option<Range<usize>> {
        static WORD: std::sync::LazyLock<regex_lite::Regex> = std::sync::LazyLock::new(|| {
            regex_lite::Regex::new(
                r#"-?[0-9]*\.[0-9][A-Za-z0-9_]*|[^`~!@#$%^&*()=+\[\]{}\\|;:'",.<>/?\-\s\x{0085}\x{00a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}]+"#,
            )
            .expect("valid plain-text word pattern")
        });
        let start = self.content[..cursor]
            .rfind(['\r', '\n'])
            .map_or(0, |i| i + 1);
        let end = self.content[cursor..]
            .find(['\r', '\n'])
            .map_or(self.content.len(), |i| cursor + i);
        WORD.find_iter(&self.content[start..end])
            .find(|m| m.start() <= cursor - start && cursor - start <= m.end())
            .map(|m| start + m.start()..start + m.end())
    }
    pub(super) fn prev(&self, at: usize) -> usize {
        self.content[..at]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }
    pub(super) fn next(&self, at: usize) -> usize {
        self.content[at..]
            .graphemes(true)
            .next()
            .map_or(at, |s| at + s.len())
    }
    pub(super) fn word(&self, at: usize, backward: bool) -> usize {
        if backward {
            let prefix = &self.content[..at];
            let trim = prefix.trim_end_matches(char::is_whitespace);
            trim.unicode_word_indices()
                .next_back()
                .map_or(0, |(i, _)| i)
        } else {
            self.content[at..]
                .unicode_word_indices()
                .next()
                .map_or(self.content.len(), |(i, s)| at + i + s.len())
        }
    }
    pub(super) fn move_to(&mut self, at: usize, shift: bool, cx: &mut Context<Self>) {
        self.last_edit = None;
        let at = self.boundary(at);
        let anchor = if !shift {
            at
        } else if self.reversed {
            self.selection.end
        } else {
            self.selection.start
        };
        self.selection = at.min(anchor)..at.max(anchor);
        self.reversed = at < anchor;
        self.caret_after_wrap = false;
        self.moved(cx);
    }
    fn navigation_destination(&mut self, key: &str, ctrl: bool, shift: bool) -> Option<usize> {
        let cursor = self.cursor();
        match key {
            "left" => Some(if !shift && !self.selection.is_empty() {
                self.selection.start
            } else if ctrl {
                self.word(cursor, true)
            } else {
                self.prev(cursor)
            }),
            "right" => Some(if !shift && !self.selection.is_empty() {
                self.selection.end
            } else if ctrl {
                self.word(cursor, false)
            } else {
                self.next(cursor)
            }),
            "home" => Some(if ctrl {
                0
            } else {
                self.visual_edge(cursor, false)
            }),
            "end" => Some(if ctrl {
                self.content.len()
            } else {
                self.visual_edge(cursor, true)
            }),
            "up" | "down" | "pageup" | "pagedown" if self.multiline => {
                let p = self.caret_position();
                let x = self.preferred_x.unwrap_or(p.x);
                self.preferred_x = Some(x);
                let step = if key.starts_with("page") {
                    self.bounds.size.height.max(self.line_height)
                } else {
                    self.line_height
                };
                Some(self.index_at(point(
                    x,
                    p.y + if key == "up" || key == "pageup" {
                        -step
                    } else {
                        step
                    },
                )))
            }
            _ => None,
        }
    }
    fn insert_newline(&mut self, below: bool, cx: &mut Context<Self>) {
        self.finish_composition();
        let mut ranges: Vec<_> = self.selected_ranges().collect();
        let mut texts = Vec::with_capacity(ranges.len());
        for (index, range) in ranges.iter_mut().enumerate() {
            let at = if below {
                self.extra.get(index).map_or_else(
                    || self.cursor(),
                    |selection| {
                        if selection.reversed {
                            selection.range.start
                        } else {
                            selection.range.end
                        }
                    },
                )
            } else {
                range.start
            };
            let start = self.content[..at].rfind(['\r', '\n']).map_or(0, |i| i + 1);
            let end = if below {
                self.content[at..]
                    .find(['\r', '\n'])
                    .map_or(self.content.len(), |i| at + i)
            } else {
                at
            };
            let prefix = &self.content[start..end];
            let indent_len = prefix.len() - prefix.trim_start_matches([' ', '\t']).len();
            texts.push(format!("\n{}", &prefix[..indent_len]));
            if below {
                *range = end..end;
            }
        }
        if self.extra.is_empty() {
            self.commit(ranges.pop().unwrap(), &texts[0], !below, cx);
        } else {
            self.commit_multiple(ranges, texts, !below, cx);
        }
    }
    pub(super) fn key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_read_only() {
            return;
        }
        let key = event.keystroke.key.as_str();
        let ctrl = event.keystroke.modifiers.control;
        let shift = event.keystroke.modifiers.shift;
        if event.keystroke.modifiers.alt || event.keystroke.modifiers.platform {
            return;
        }
        let cursor = self.cursor();
        let mut dest = None;
        match key {
            "d" if ctrl && !shift && self.multiline => {
                self.select_next_occurrence(cx);
            }
            "left" | "right" | "home" | "end" => {
                dest = self.navigation_destination(key, ctrl, shift);
            }
            "up" | "down" | "pageup" | "pagedown" if self.multiline => {
                dest = self.navigation_destination(key, ctrl, shift);
            }
            "backspace" | "delete" => {
                if !self.extra.is_empty() {
                    self.finish_composition();
                    let ranges: Vec<_> = self
                        .selected_ranges()
                        .map(|range| {
                            if !range.is_empty() {
                                range
                            } else if key == "backspace" {
                                (if ctrl {
                                    self.word(range.start, true)
                                } else {
                                    self.prev(range.start)
                                })..range.start
                            } else {
                                range.start..if ctrl {
                                    self.word(range.start, false)
                                } else {
                                    self.next(range.start)
                                }
                            }
                        })
                        .collect();
                    let texts = vec![String::new(); ranges.len()];
                    self.commit_multiple(ranges, texts, false, cx);
                    cx.stop_propagation();
                    return;
                }
                let range = if self.selection.is_empty() {
                    if key == "backspace" {
                        (if ctrl {
                            self.word(cursor, true)
                        } else {
                            self.prev(cursor)
                        })..cursor
                    } else {
                        cursor..if ctrl {
                            self.word(cursor, false)
                        } else {
                            self.next(cursor)
                        }
                    }
                } else {
                    self.selection.clone()
                };
                if !range.is_empty() {
                    self.commit(range, "", false, cx);
                }
            }
            "enter" => {
                if self.multiline {
                    self.insert_newline(ctrl && !shift, cx);
                } else {
                    cx.emit(InputEvent::PressEnter { shift });
                }
            }
            "tab" if self.multiline && !ctrl => self.replace("\t", window, cx),
            _ => return,
        }
        if !matches!(key, "up" | "down" | "pageup" | "pagedown") {
            self.preferred_x = None;
        }
        if let Some(at) = dest {
            // Reuse the same navigation rules for every additional caret.
            let extra = std::mem::take(&mut self.extra);
            let primary = self.selection.clone();
            let reversed = self.reversed;
            let preferred = self.preferred_x;
            let mut moved = Vec::with_capacity(extra.len());
            for selection in extra {
                self.selection = selection.range;
                self.reversed = selection.reversed;
                self.caret_after_wrap = selection.after_wrap;
                self.preferred_x = if matches!(key, "up" | "down" | "pageup" | "pagedown") {
                    selection.preferred_x
                } else {
                    None
                };
                if let Some(at) = self.navigation_destination(key, ctrl, shift) {
                    self.move_to(at, shift, cx);
                }
                moved.push(Selection {
                    range: self.selection.clone(),
                    reversed: self.reversed,
                    preferred_x: self.preferred_x,
                    after_wrap: self.caret_after_wrap,
                });
            }
            self.selection = primary;
            self.reversed = reversed;
            self.preferred_x = preferred;
            self.move_to(at, shift, cx);
            self.extra = moved;
            self.deduplicate_selections();
            self.occurrence_word = false;
        }
        cx.stop_propagation();
    }
    pub(super) fn visual_edge(&self, at: usize, end: bool) -> usize {
        let p = self.position(at);
        self.index_at(point(if end { px(1e8) } else { px(0.) }, p.y))
    }
    pub(super) fn position(&self, at: usize) -> Point<Pixels> {
        self.position_with_wrap_side(at, false)
    }
    pub(super) fn caret_position(&self) -> Point<Pixels> {
        self.position_with_wrap_side(
            self.cursor(),
            self.caret_after_wrap || self.reversed && !self.selection.is_empty(),
        )
    }
    pub(super) fn position_with_wrap_side(&self, at: usize, after_wrap: bool) -> Point<Pixels> {
        let i = self
            .lines
            .partition_point(|l| l.start <= at)
            .saturating_sub(1);
        self.lines.get(i).map_or(
            point(self.content_padding.left, self.content_padding.top),
            |p| {
                let byte = at.min(p.end) - p.start;
                let pos = if after_wrap {
                    p.position_after_wrap(byte, self.line_height)
                } else {
                    p.position(byte, self.line_height)
                };
                pos + point(self.content_padding.left, p.top)
            },
        )
    }
    pub(super) fn index_at(&self, p: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            // Placeholder glyphs are not editable text (including multi-clicks).
            return 0;
        }
        if self.multiline {
            if self.lines.first().is_some_and(|l| p.y < l.top) {
                return 0;
            }
            if self
                .lines
                .last()
                .is_some_and(|l| p.y >= l.top + l.line.size(self.line_height).height)
            {
                return self.content.len();
            }
        }
        let p = point(p.x - self.content_padding.left, p.y);
        let i = self
            .lines
            .partition_point(|l| l.top <= p.y)
            .saturating_sub(1);
        self.lines.get(i).map_or(0, |l| {
            // A single-line field's vertical padding must still use its text row.
            let y = if self.multiline {
                (p.y - l.top).max(px(0.))
            } else {
                px(0.)
            };
            let at = self.boundary(
                l.start
                    + l.source_index(
                        l.line
                            .index_for_position(point(p.x, y), self.line_height)
                            .unwrap_or_else(|i| i),
                    ),
            );
            // Shaping returns the glyph under the pointer. Choose the nearest
            // caret instead, keeping combining sequences and emoji indivisible.
            let mut cursor =
                unicode_segmentation::GraphemeCursor::new(at, self.content.len(), true);
            let start = if cursor.is_boundary(&self.content, 0).expect("complete text") {
                at
            } else {
                cursor
                    .prev_boundary(&self.content, 0)
                    .expect("complete text")
                    .unwrap_or(0)
            };
            let end = self.next(start).min(l.end);
            let left = l.position(start - l.start, self.line_height);
            let right = l.position(end - l.start, self.line_height);
            // A wrap boundary's caret is represented at the preceding row's end.
            let row_top = (y / self.line_height).floor() * self.line_height;
            let left_x = if left.y < row_top { px(0.) } else { left.x };
            if right.y == row_top && p.x > (left_x + right.x) / 2. {
                end
            } else {
                start
            }
        })
    }
    pub(super) fn mouse_index(&self, position: Point<Pixels>) -> usize {
        self.index_at(position - self.bounds.origin - self.scroll)
    }
    pub(super) fn mouse_down(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus(window, cx);
        if let Some((axis, track, thumb)) = [false, true]
            .into_iter()
            .filter_map(|axis| {
                self.scrollbar_geometry(axis)
                    .map(|(track, thumb)| (axis, track, thumb))
            })
            .find(|(_, track, _)| track.contains(&e.position))
        {
            let at = if axis { e.position.x } else { e.position.y };
            let viewport = if axis {
                track.size.width
            } else {
                track.size.height
            };
            let extent = if axis {
                self.content_size.width
            } else {
                self.content_size.height
            };
            let origin = if axis { track.left() } else { track.top() };
            let thumb_size = if axis {
                thumb.size.width
            } else {
                thumb.size.height
            };
            let thumb_start = if axis { thumb.left() } else { thumb.top() };
            let value = if at >= thumb_start && at <= thumb_start + thumb_size {
                if axis { self.scroll.x } else { self.scroll.y }
            } else {
                -((extent - viewport)
                    * ((at - origin - thumb_size / 2.) / (viewport - thumb_size).max(px(1.))))
                .clamp(px(0.), extent - viewport)
            };
            if axis {
                self.scroll.x = value;
            } else {
                self.scroll.y = value;
            }
            self.scroll_drag = Some((axis, at, value));
            self.reveal = false;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if e.modifiers.control
            && !e.modifiers.alt
            && !e.modifiers.shift
            && !e.modifiers.platform
            && self.open_link_at(e.position, cx)
        {
            self.selecting = false;
            cx.stop_propagation();
            return;
        }
        if self.multiline
            && e.modifiers.alt
            && e.modifiers.shift
            && !e.modifiers.control
            && !e.modifiers.platform
        {
            self.begin_column_selection(e.position, cx);
            cx.stop_propagation();
            return;
        }
        self.selecting = true;
        self.clear_extra_selections();
        self.preferred_x = None;
        let at = self.mouse_index(e.position);
        if e.click_count >= 3 {
            let start = self.content[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1);
            let end = self.content[at..]
                .find(['\n', '\r'])
                .map_or(self.content.len(), |i| at + i);
            self.set_selected_range(start..end, cx);
        } else if e.click_count == 2 {
            let range = self.word_at(at).unwrap_or(at..self.next(at));
            self.set_selected_range(range, cx);
        } else {
            self.move_to(at, e.modifiers.shift, cx);
        }
        cx.stop_propagation();
    }
    pub(super) fn mouse_move(
        &mut self,
        e: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((axis, start, offset)) = self.scroll_drag {
            let viewport = if axis {
                self.bounds.size.width
            } else {
                self.bounds.size.height
            };
            let extent = if axis {
                self.content_size.width
            } else {
                self.content_size.height
            };
            let at = if axis { e.position.x } else { e.position.y };
            let thumb = (viewport * (viewport / extent)).max(px(24.)).min(viewport);
            let value =
                offset - (extent - viewport) * ((at - start) / (viewport - thumb).max(px(1.)));
            if axis {
                self.scroll.x = value
            } else {
                self.scroll.y = value
            };
            self.clamp_scroll();
            self.reveal = false;
            cx.notify();
            return;
        }
        if self.selecting {
            if e.position.y < self.bounds.top() {
                self.scroll.y += self.line_height;
            }
            if e.position.y > self.bounds.bottom() {
                self.scroll.y -= self.line_height;
            }
            if self.column_anchor.is_some() && !self.wrap {
                if e.position.x < self.bounds.left() {
                    self.scroll.x += self.line_height;
                }
                if e.position.x > self.bounds.right() {
                    self.scroll.x -= self.line_height;
                }
            }
            self.clamp_scroll();
            if self.column_anchor.is_some() {
                self.update_column_selection(e.position, cx);
            } else {
                self.move_to(self.mouse_index(e.position), true, cx);
            }
        }
    }
}
