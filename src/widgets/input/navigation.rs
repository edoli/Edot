//! Grapheme/visual navigation and mouse selection.
use super::*;

impl InputState {
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
        self.moved(cx);
    }
    pub(super) fn key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let ctrl = event.keystroke.modifiers.control;
        let shift = event.keystroke.modifiers.shift;
        if event.keystroke.modifiers.alt || event.keystroke.modifiers.platform {
            return;
        }
        let cursor = self.cursor();
        let mut dest = None;
        match key {
            "left" => {
                dest = Some(if !shift && !self.selection.is_empty() {
                    self.selection.start
                } else if ctrl {
                    self.word(cursor, true)
                } else {
                    self.prev(cursor)
                })
            }
            "right" => {
                dest = Some(if !shift && !self.selection.is_empty() {
                    self.selection.end
                } else if ctrl {
                    self.word(cursor, false)
                } else {
                    self.next(cursor)
                })
            }
            "home" => {
                dest = Some(if ctrl {
                    0
                } else {
                    self.visual_edge(cursor, false)
                })
            }
            "end" => {
                dest = Some(if ctrl {
                    self.content.len()
                } else {
                    self.visual_edge(cursor, true)
                })
            }
            "up" | "down" | "pageup" | "pagedown" if self.multiline => {
                let p = self.position(cursor);
                let x = self.preferred_x.unwrap_or(p.x);
                self.preferred_x = Some(x);
                let step = if key.starts_with("page") {
                    self.bounds.size.height.max(self.line_height)
                } else {
                    self.line_height
                };
                dest = Some(self.index_at(point(
                    x,
                    p.y + if key == "up" || key == "pageup" {
                        -step
                    } else {
                        step
                    },
                )));
            }
            "backspace" | "delete" => {
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
                    self.replace("\n", window, cx);
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
            self.move_to(at, shift, cx);
        }
        cx.stop_propagation();
    }
    pub(super) fn visual_edge(&self, at: usize, end: bool) -> usize {
        let p = self.position(at);
        self.index_at(point(if end { px(1e8) } else { px(0.) }, p.y))
    }
    pub(super) fn position(&self, at: usize) -> Point<Pixels> {
        let i = self
            .lines
            .partition_point(|l| l.start <= at)
            .saturating_sub(1);
        self.lines.get(i).map_or(
            point(self.content_padding.left, self.content_padding.top),
            |p| {
                p.position(at.min(p.end) - p.start, self.line_height)
                    + point(self.content_padding.left, p.top)
            },
        )
    }
    pub(super) fn index_at(&self, p: Point<Pixels>) -> usize {
        let p = point(p.x - self.content_padding.left, p.y);
        let i = self
            .lines
            .partition_point(|l| l.top <= p.y)
            .saturating_sub(1);
        self.lines.get(i).map_or(0, |l| {
            l.start
                + l.source_index(
                    l.line
                        .index_for_position(point(p.x, (p.y - l.top).max(px(0.))), self.line_height)
                        .unwrap_or_else(|i| i),
                )
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
        self.selecting = true;
        let at = self.mouse_index(e.position);
        if e.click_count >= 3 {
            let start = self.content[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1);
            let end = self.content[at..]
                .find(['\n', '\r'])
                .map_or(self.content.len(), |i| at + i);
            self.set_selected_range(start..end, cx);
        } else if e.click_count == 2 {
            let range = self
                .content
                .unicode_word_indices()
                .find(|(i, s)| *i <= at && at < *i + s.len())
                .map(|(i, s)| i..i + s.len())
                .unwrap_or(at..self.next(at));
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
            self.clamp_scroll();
            self.move_to(self.mouse_index(e.position), true, cx);
        }
    }
}
