//! Grapheme/visual navigation and mouse selection.
use super::*;

impl InputState {
    pub(super) fn stop_pointer_drag(&mut self) {
        self.drop_position = None;
        self.selecting = false;
        self.column_anchor = None;
        self.scroll_drag = None;
        self.selection_auto_scroll.stop();
        self.stop_reading_drag();
    }
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
    fn deletion_range(
        &self,
        range: Range<usize>,
        backward: bool,
        word: bool,
        line: bool,
    ) -> Range<usize> {
        if !range.is_empty() {
            return range;
        }
        let cursor = range.start;
        let target = if line {
            self.visual_edge(cursor, !backward)
        } else if word {
            self.word(cursor, backward)
        } else if backward {
            self.prev(cursor)
        } else {
            self.next(cursor)
        };
        if backward {
            target..cursor
        } else {
            cursor..target
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
        self.key_on(event, cfg!(target_os = "macos"), window, cx);
    }

    fn key_on(
        &mut self,
        event: &KeyDownEvent,
        macos: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The platform IME owns composition and AltGr/dead-key character input.
        if event.prefer_character_input {
            return;
        }
        if self.is_composing() {
            cx.stop_propagation(); // Keep focus traversal and parent shortcuts out of preedit.
            return;
        }
        let modifiers = event.keystroke.modifiers;
        let primary = super::super::shortcuts::primary_on(modifiers, macos);
        let shift = modifiers.shift;
        let mut key = event.keystroke.key.as_str();
        let mut ctrl = if macos { modifiers.alt } else { primary };
        if macos {
            if modifiers.control && !modifiers.platform && !modifiers.alt {
                key = match key {
                    "a" => "home",
                    "e" => "end",
                    "b" => "left",
                    "f" => "right",
                    "p" => "up",
                    "n" => "down",
                    "h" => "backspace",
                    "d" => "delete",
                    _ => return,
                };
            } else if modifiers.control || modifiers.alt && modifiers.platform {
                return;
            } else if primary {
                (key, ctrl) = match key {
                    "left" => ("home", false),
                    "right" => ("end", false),
                    "up" => ("home", true),
                    "down" => ("end", true),
                    _ => (key, false),
                };
            } else if modifiers.alt && !matches!(key, "left" | "right" | "backspace" | "delete") {
                return; // Option+letters must remain available for text input.
            }
        } else if modifiers.alt || modifiers.platform {
            return;
        }
        if self.is_read_only()
            && !matches!(
                key,
                "left" | "right" | "up" | "down" | "home" | "end" | "pageup" | "pagedown"
            )
        {
            if matches!(key, "backspace" | "delete" | "enter") {
                cx.stop_propagation();
            }
            return;
        }
        if primary && self.multiline && self.text_mode == TextMode::Markdown {
            let marker = match (key, shift) {
                ("b", false) => Some("**"),
                ("i", false) => Some("*"),
                ("x", true) if macos => Some("~~"),
                ("e", false) if macos => Some("`"),
                ("`", true) | ("~", false) if !macos => Some("~~"),
                ("`", false) if !macos => Some("`"),
                _ => None,
            };
            if let Some(marker) = marker {
                if !event.is_held {
                    self.toggle_inline_format(marker, cx);
                }
                cx.stop_propagation();
                return;
            }
        }
        let mut dest = None;
        match key {
            "d" if primary && !shift && self.multiline => {
                self.select_next_occurrence(cx);
            }
            "left" | "right" | "home" | "end" => {
                dest = self.navigation_destination(key, ctrl, shift);
            }
            "up" | "down" | "pageup" | "pagedown" if self.multiline => {
                dest = self.navigation_destination(key, ctrl, shift);
            }
            "backspace" | "delete" => {
                let line = macos && primary;
                if !self.extra.is_empty() {
                    self.finish_composition();
                    let ranges: Vec<_> = self
                        .selected_ranges()
                        .map(|range| self.deletion_range(range, key == "backspace", ctrl, line))
                        .collect();
                    let texts = vec![String::new(); ranges.len()];
                    self.commit_multiple(ranges, texts, false, cx);
                    cx.stop_propagation();
                    return;
                }
                let range =
                    self.deletion_range(self.selection.clone(), key == "backspace", ctrl, line);
                if !range.is_empty() {
                    self.commit(range, "", false, cx);
                }
            }
            "enter" => {
                if self.multiline {
                    self.insert_newline(primary && !shift, cx);
                } else {
                    cx.emit(InputEvent::PressEnter { shift });
                }
            }
            "tab" if self.multiline && !primary => self.replace("\t", window, cx),
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
        if super::super::shortcuts::primary(e.modifiers)
            && !e.modifiers.shift
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
            self.selection_auto_scroll.last_drag_position = Some(e.position);
            self.begin_column_selection(e.position, cx);
            cx.stop_propagation();
            return;
        }
        self.selecting = true;
        self.selection_auto_scroll.last_drag_position = Some(e.position);
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
            self.selection_auto_scroll.last_drag_position = Some(e.position);
            self.drag_selection(e.position, cx);
            let scrolling = self.selection_scroll_delta(e.position) != point(px(0.), px(0.));
            self.selection_auto_scroll
                .set(scrolling.then_some(px(1.)), cx, |_, s, cx| {
                    if !s.selecting {
                        s.selection_auto_scroll.stop();
                        return;
                    }
                    if let Some(position) = s.selection_auto_scroll.last_drag_position {
                        s.scroll -= s.selection_scroll_delta(position);
                        s.clamp_scroll();
                        s.drag_selection(position, cx);
                    }
                });
        }
    }
    fn selection_scroll_delta(&self, position: Point<Pixels>) -> Point<Pixels> {
        let delta = |value: Pixels, min: Pixels, max: Pixels| {
            if value < min {
                -((min - value) * 0.25).clamp(px(1.), px(24.))
            } else if value > max {
                ((value - max) * 0.25).clamp(px(1.), px(24.))
            } else {
                px(0.)
            }
        };
        point(
            if self.wrap {
                px(0.)
            } else {
                delta(position.x, self.bounds.left(), self.bounds.right())
            },
            if self.multiline {
                delta(position.y, self.bounds.top(), self.bounds.bottom())
            } else {
                px(0.)
            },
        )
    }
    fn drag_selection(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.column_anchor.is_some() {
            self.update_column_selection(position, cx);
        } else {
            self.move_to(self.mouse_index(position), true, cx);
        }
        // Timer-driven scrolling uses the pointer, rather than revealing a caret
        // that may lie several rows beyond the viewport.
        self.reveal = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{InputState, TextMode, Undo};
    use gpui::{AppContext, EntityInputHandler, KeyDownEvent, Keystroke};

    fn event(key: &str) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: Keystroke::parse(key).unwrap(),
            is_held: false,
            prefer_character_input: false,
        }
    }

    #[gpui::test]
    fn macos_navigation_uses_option_for_words_and_cmd_for_line_and_document(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let original = "one two\n한글🙂";
        let window = cx.add_window(|w, cx| {
            InputState::new(w, cx)
                .soft_wrap(false)
                .default_value(original.into())
        });
        let editor = window.root(cx).unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            w.draw(cx).clear(cx);
            editor.update(cx, |s, cx| {
                s.set_selected_range(7..7, cx);
                s.key_on(&event("alt-left"), true, w, cx);
                assert_eq!(s.selected_range(), 4..4);
                s.key_on(&event("cmd-left"), true, w, cx);
                assert_eq!(s.selected_range(), 0..0);
                s.key_on(&event("cmd-shift-right"), true, w, cx);
                assert_eq!(s.selected_value(), "one two");
                s.key_on(&event("cmd-down"), true, w, cx);
                assert_eq!(s.selected_range(), original.len()..original.len());
                s.key_on(&event("cmd-shift-up"), true, w, cx);
                assert_eq!(s.selected_value(), original);
                s.set_selected_range(7..7, cx);
                s.key_on(&event("alt-backspace"), true, w, cx);
                assert_eq!(s.value(), "one \n한글🙂");
                s.undo(&Undo, w, cx);
                assert_eq!(s.value(), original);
                s.key_on(&event("cmd-backspace"), true, w, cx);
                assert_eq!(s.value(), "\n한글🙂");
                s.undo(&Undo, w, cx);
                s.key_on(&event("ctrl-a"), true, w, cx);
                assert_eq!(s.selected_range(), 0..0);
                s.key_on(&event("ctrl-e"), true, w, cx);
                assert_eq!(s.selected_range(), 7..7);
                s.set_text_mode(TextMode::Markdown, cx);
                s.set_selected_range(0..3, cx);
                s.key_on(&event("cmd-e"), true, w, cx);
                assert_eq!(s.value(), "`one` two\n한글🙂");
                s.undo(&Undo, w, cx);
                s.key_on(&event("cmd-shift-x"), true, w, cx);
                assert_eq!(s.value(), "~~one~~ two\n한글🙂");
            });
        })
        .unwrap();
    }

    #[gpui::test]
    fn composition_keeps_utf16_ranges_and_owns_editing_keys_until_commit(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let original = "A🙂한글Z";
        let window = cx.add_window(|w, cx| {
            InputState::new(w, cx)
                .soft_wrap(false)
                .default_value(original.into())
        });
        let editor = window.root(cx).unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_selected_range(1..5, cx);
                let mut actual = None;
                assert_eq!(
                    s.text_for_range(1..3, &mut actual, w, cx).as_deref(),
                    Some("🙂")
                );
                assert_eq!(actual, Some(1..3));
                s.replace_and_mark_text_in_range(Some(1..3), "🇰🇷가", Some(4..5), w, cx);
                assert_eq!(s.selected_text_range(false, w, cx).unwrap().range, 5..6);
                assert_eq!(s.marked_text_range(w, cx), Some(1..6));
                let composing = s.value();
                for macos in [false, true] {
                    for key in ["enter", "tab", "escape", "backspace", "left"] {
                        s.key_on(&event(key), macos, w, cx);
                        assert_eq!(s.value(), composing);
                        assert!(s.is_composing());
                    }
                }
                s.replace_text_in_range(None, "한", w, cx);
                assert_eq!(s.value(), "A한한글Z");
                assert!(!s.is_composing());
                s.undo(&Undo, w, cx);
                assert_eq!(s.value(), original);
                let mut text_input = event("ctrl-alt-backspace");
                text_input.prefer_character_input = true;
                s.key_on(&text_input, false, w, cx);
                assert_eq!(s.value(), original);
            });
        })
        .unwrap();
    }
}
