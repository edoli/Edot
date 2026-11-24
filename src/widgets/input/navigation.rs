//! Grapheme/visual navigation and mouse selection.
use super::*;
use std::borrow::Cow;

impl InputState {
    pub(super) fn stop_pointer_drag(&mut self) {
        self.drop_position = None;
        self.selecting = false;
        self.column_anchor = None;
        self.scroll_drag = None;
        self.selection_auto_scroll.stop();
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
        let at = self.table_caret(self.cursor(), self.boundary(at));
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
    fn navigation_destination(
        &mut self,
        key: &str,
        ctrl: bool,
        shift: bool,
    ) -> Option<(usize, bool)> {
        let cursor = self.cursor();
        if !ctrl && let Some(cell) = self.visual_cell(cursor) {
            match key {
                "home" => return Some((cell.start, false)),
                "end" => return Some((cell.end, false)),
                _ => {}
            }
        }
        let at = match key {
            "left" | "right" => {
                let backwards = key == "left";
                if !shift && !self.selection.is_empty() {
                    if backwards {
                        self.selection.start
                    } else {
                        self.selection.end
                    }
                } else if ctrl {
                    let target = self.table_caret(cursor, self.word(cursor, backwards));
                    if self.visual_document().is_some()
                        && self.position(target) == self.position(cursor)
                    {
                        self.horizontal_visual_caret(cursor, backwards)
                            .unwrap_or(target)
                    } else {
                        target
                    }
                } else {
                    self.horizontal_visual_caret(cursor, backwards)
                        .unwrap_or_else(|| {
                            if backwards {
                                self.prev(cursor)
                            } else {
                                self.next(cursor)
                            }
                        })
                }
            }
            "home" | "end" if ctrl => {
                if key == "home" {
                    0
                } else {
                    self.content.len()
                }
            }
            "home" | "end" => {
                let p = self.caret_position();
                return Some(self.navigation_target(point(
                    if key == "end" { px(1e8) } else { px(0.) },
                    p.y + px(0.5),
                )));
            }
            "up" | "down" | "pageup" | "pagedown" if self.multiline => {
                let p = self.caret_position();
                let x = self.preferred_x.unwrap_or(p.x);
                self.preferred_x = Some(x);
                if matches!(key, "up" | "down") && self.visual_document().is_some() {
                    let upwards = key == "up";
                    if let Some(target) = self.vertical_visual_cell(cursor, upwards, x) {
                        return Some(target);
                    }
                    let distance = if upwards {
                        -px(0.5)
                    } else {
                        self.visual_caret_height(cursor) + px(0.5)
                    };
                    let target = self.navigation_target(point(x, p.y + distance));
                    let target_y = self.position_with_wrap_side(target.0, target.1).y;
                    if (upwards && target_y < p.y) || (!upwards && target_y > p.y) {
                        return Some(target);
                    }
                    return Some(
                        self.neighboring_visual_caret(cursor, upwards, x)
                            .unwrap_or((cursor, self.caret_after_wrap)),
                    );
                }
                let step = if key.starts_with("page") {
                    self.bounds.size.height.max(self.line_height)
                } else if key == "up" {
                    // A point just above the current visual row also works
                    // when the preceding heading or block is taller.
                    px(0.5)
                } else {
                    self.row_at_y(p.y)
                        .map_or(self.line_height, |row| row.line_height)
                };
                return Some(self.navigation_target(point(
                    x,
                    p.y + if key == "up" || key == "pageup" {
                        -step
                    } else {
                        step
                    },
                )));
            }
            _ => return None,
        };
        let after_wrap = key == "left" && self.wrap_side_at_y(at, self.caret_position().y);
        Some((at, after_wrap))
    }

    fn navigation_target(&self, point: Point<Pixels>) -> (usize, bool) {
        let at = self.table_caret(self.cursor(), self.index_at(point));
        (at, self.wrap_side_at_y(at, point.y))
    }

    fn wrap_side_at_y(&self, at: usize, y: Pixels) -> bool {
        let before = self.position(at).y;
        let after = self.position_with_wrap_side(at, true).y;
        after > before && y >= after
    }

    pub(super) fn neighboring_visual_caret(
        &self,
        at: usize,
        backwards: bool,
        x: Pixels,
    ) -> Option<(usize, bool)> {
        let index = self
            .lines
            .partition_point(|line| line.start <= at)
            .checked_sub(1)?;
        if backwards {
            self.lines[..index]
                .iter()
                .rev()
                .find_map(|row| self.caret_on_neighbor(row, backwards, x))
        } else {
            self.lines[index + 1..]
                .iter()
                .find_map(|row| self.caret_on_neighbor(row, backwards, x))
        }
    }

    fn caret_on_neighbor(
        &self,
        neighbor: &Paragraph,
        backwards: bool,
        x: Pixels,
    ) -> Option<(usize, bool)> {
        if neighbor.compact {
            return None;
        }
        let y = if backwards {
            neighbor.top + neighbor.height() - px(0.5)
        } else {
            neighbor.top + px(0.5)
        };
        if let Some(rich) = neighbor.rich {
            let document = self.visual_document()?;
            let visual = self.visual.as_ref()?;
            if let Some(byte) = visual.geometry.hit(neighbor.start, point(x, y)) {
                return Some((byte, self.wrap_side_at_y(byte, y)));
            }
            let blocks = document.rich_blocks(rich.index);
            let mut text = blocks.iter().flat_map(|block| {
                block
                    .content
                    .pieces
                    .iter()
                    .chain(block.cells.iter().flat_map(|cell| cell.pieces.iter()))
            });
            let byte = if backwards {
                text.next_back().map(|piece| piece.source.end)
            } else {
                text.next().map(|piece| piece.source.start)
            }?;
            return Some((byte, false));
        }
        Some(self.navigation_target(point(x, y)))
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
        let target = self
            .visual_text_bounds(cursor)
            .map_or(target, |cell| target.clamp(cell.start, cell.end));
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
            if below {
                *range = end..end;
            }
            let Some(target) = self.visual_table_input_range(range.clone()) else {
                return;
            };
            *range = target;
            let newline = format!("\n{}", &prefix[..indent_len]);
            let newline = match self.visual_cell_input(range, &newline) {
                Cow::Borrowed(_) => newline,
                Cow::Owned(text) => text,
            };
            texts.push(newline);
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
        use crate::shortcuts::{ActiveShortcuts, Command};
        let command = cx
            .global::<ActiveShortcuts>()
            .command(&event.keystroke, macos);
        self.ensure_visual(cx);
        match command {
            Some(Command::Undo) => self.undo(&Undo, window, cx),
            Some(Command::Redo) => self.redo(&Redo, window, cx),
            Some(Command::Cut) => self.cut(&Cut, window, cx),
            Some(Command::Copy) => self.copy(&Copy, window, cx),
            Some(Command::Paste) => self.paste(&Paste, window, cx),
            Some(Command::SelectAll) => self.select_all(window, cx),
            Some(Command::NextOccurrence) if self.multiline && !self.is_read_only() => {
                self.select_next_occurrence(cx)
            }
            Some(Command::Bold | Command::Italic | Command::Strike | Command::Code)
                if self.multiline && !self.is_read_only() =>
            {
                if !event.is_held {
                    let marker = match command.unwrap() {
                        Command::Bold => "**",
                        Command::Italic => "*",
                        Command::Strike => "~~",
                        _ => "`",
                    };
                    self.toggle_inline_format(marker, cx);
                }
            }
            _ => {
                self.navigation_key(event, macos, window, cx);
                return;
            }
        }
        cx.stop_propagation();
    }

    fn navigation_key(
        &mut self,
        event: &KeyDownEvent,
        macos: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let mut dest = None;
        match key {
            "escape" if self.close_visual_block_edit(cx) => {
                cx.stop_propagation();
                return;
            }
            "enter"
                if primary
                    && !shift
                    && self.extra.is_empty()
                    && self.insert_after_visual_table(cx) =>
            {
                cx.stop_propagation();
                return;
            }
            "tab" | "enter"
                if !primary
                    && self.extra.is_empty()
                    && self.visual_cell(self.cursor()).is_some() =>
            {
                if let Some(next) = self.step_visual_cell(self.cursor(), shift) {
                    self.move_to(next, false, cx);
                }
                cx.stop_propagation();
                return;
            }
            "left" | "right" | "home" | "end" => {
                dest = self.navigation_destination(key, ctrl, shift);
            }
            "up" | "down" | "pageup" | "pagedown" if self.multiline => {
                dest = self.navigation_destination(key, ctrl, shift);
            }
            "backspace" | "delete" => {
                let line = macos && primary;
                self.finish_composition();
                let ranges = self
                    .selected_ranges()
                    .map(|range| self.deletion_range(range, key == "backspace", ctrl, line))
                    .map(|range| self.visual_table_input_range(range))
                    .collect::<Option<Vec<_>>>();
                if let Some(ranges) = ranges {
                    self.commit_deletion(ranges, event, cx);
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
        if let Some((at, after_wrap)) = dest {
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
                if let Some((at, after_wrap)) = self.navigation_destination(key, ctrl, shift) {
                    self.move_to(at, shift, cx);
                    self.caret_after_wrap = after_wrap;
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
            self.caret_after_wrap = after_wrap;
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
        self.position_with_wrap_side(self.cursor(), self.caret_after_wrap)
    }
    pub(super) fn position_with_wrap_side(&self, at: usize, after_wrap: bool) -> Point<Pixels> {
        let i = self
            .lines
            .partition_point(|l| l.start <= at)
            .saturating_sub(1);
        self.lines.get(i).map_or(
            point(self.content_padding.left, self.content_padding.top),
            |p| {
                if p.rich.is_some()
                    && let Some((position, _)) = self.rich_caret(p.start, at, after_wrap)
                {
                    return position;
                }
                let byte = at.min(p.end) - p.start;
                let pos = if after_wrap {
                    p.position_after_wrap(byte)
                } else {
                    p.position(byte)
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
            if self.lines.last().is_some_and(|l| p.y >= l.top + l.height()) {
                return self.content.len();
            }
        }
        let p = point(p.x - self.content_padding.left, p.y);
        self.row_at_y(p.y).map_or(0, |l| {
            if l.rich.is_some() {
                if let Some(at) = self.visual.as_ref().and_then(|visual| {
                    visual
                        .geometry
                        .hit(l.start, p + point(self.content_padding.left, px(0.)))
                }) {
                    return self.boundary(at);
                }
                let fraction = ((p.y - l.top) / l.height()).clamp(0., 1.);
                let target =
                    self.boundary(l.start + ((l.end - l.start) as f32 * fraction) as usize);
                return self.content[l.start..target]
                    .rfind(['\r', '\n'])
                    .map_or(l.start, |offset| l.start + offset + 1)
                    .min(l.end);
            }
            let x = p.x;
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
                            .index_for_position(point(x, y), l.line_height)
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
            let end = l
                .display
                .substitution_end(start - l.start)
                .map_or_else(|| self.next(start), |end| l.start + end)
                .min(l.end);
            let left = l.position(start - l.start);
            let right = l.position(end - l.start);
            // A wrap boundary's caret is represented at the preceding row's end.
            let row_top = (y / l.line_height).floor() * l.line_height;
            let left_x = if left.y < row_top { px(0.) } else { left.x };
            if right.y == row_top && x > (left_x + right.x) / 2. {
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
        let local = e.position - self.bounds.origin - self.scroll;
        if !e.modifiers.shift
            && let Some(row) = self.row_at_y(local.y)
            && let Some(rich) = row.rich
            && local.y >= row.top
            && local.y < row.top + row.height()
            && self
                .visual
                .as_ref()
                .is_some_and(|visual| visual.geometry.diagram_hit(row.start, local).is_none())
            && self.visual_document().is_some_and(|document| {
                matches!(
                    document.rich_blocks(rich.index)[0].kind,
                    super::reading_document::Kind::Mermaid(Ok(_))
                )
            })
        {
            self.set_selected_range(row.start..row.end, cx);
            self.reveal = false;
            cx.stop_propagation();
            return;
        }
        let (at, after_wrap) = self.navigation_target(local);
        if e.click_count >= 3 {
            let range = self.visual_cell(at).unwrap_or_else(|| {
                let start = self.content[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1);
                let end = self.content[at..]
                    .find(['\n', '\r'])
                    .map_or(self.content.len(), |i| at + i);
                start..end
            });
            self.set_selected_range(range, cx);
        } else if e.click_count == 2 {
            let mut range = self.word_at(at).unwrap_or(at..self.next(at));
            if let Some(cell) = self.visual_cell(at) {
                range.start = range.start.max(cell.start);
                range.end = range.end.min(cell.end);
            }
            self.set_selected_range(range, cx);
        } else {
            self.move_to(at, e.modifiers.shift, cx);
            self.caret_after_wrap = after_wrap;
        }
        if self.visual_document().is_some() {
            self.reveal = false;
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
            let anchor = if self.reversed {
                self.selection.end
            } else {
                self.selection.start
            };
            let local = position - self.bounds.origin - self.scroll;
            let diagram = self
                .visual
                .as_ref()
                .and_then(|visual| visual.geometry.diagram_drag(anchor, local));
            let (at, after_wrap) = diagram.map_or_else(
                || self.navigation_target(local),
                |at| (at, self.wrap_side_at_y(at, local.y)),
            );
            self.move_to(at, true, cx);
            self.caret_after_wrap = after_wrap;
        }
        // Timer-driven scrolling uses the pointer, rather than revealing a caret
        // that may lie several rows beyond the viewport.
        self.reveal = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{InputState, Undo};
    use gpui::{AppContext, EntityInputHandler, KeyDownEvent, Keystroke};

    fn event(key: &str) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: Keystroke::parse_with_os(key, crate::shortcuts::KEYBOARD_OS).unwrap(),
            is_held: false,
            prefer_character_input: false,
        }
    }

    #[gpui::test]
    fn held_deletion_restores_graphemes_and_selection_in_one_transaction(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let window = cx.add_window(InputState::new);
        let editor = window.root(cx).unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                let original = "A👨‍👩‍👧‍👦e\u{301}\r\n한글Z";
                for key in ["backspace", "delete"] {
                    s.set_value(original, w, cx);
                    let at = if key == "backspace" {
                        original.len()
                    } else {
                        0
                    };
                    s.set_selected_range(at..at, cx);
                    for repeat in 0..9 {
                        let mut deletion = event(key);
                        deletion.is_held = repeat > 0;
                        s.key_on(&deletion, false, w, cx);
                        // Holding a key must also bridge a long OS repeat delay.
                        s.last_edit =
                            Some(std::time::Instant::now() - std::time::Duration::from_secs(2));
                    }
                    assert_eq!(s.value(), "");
                    assert_eq!(s.undo.len(), 1);
                    assert!(s.undo[0].others.is_empty());
                    s.undo(&Undo, w, cx);
                    assert_eq!(s.value(), original);
                    assert_eq!(s.selected_range(), at..at);
                    s.redo(&super::Redo, w, cx);
                    assert_eq!(s.value(), "");
                    assert_eq!(s.selected_range(), 0..0);
                }
                for (key, expected) in [("backspace", "e"), ("delete", "a")] {
                    s.set_value("abcde", w, cx);
                    s.set_selected_range(1..4, cx);
                    s.key_on(&event(key), false, w, cx);
                    let mut deletion = event(key);
                    deletion.is_held = true;
                    s.key_on(&deletion, false, w, cx);
                    assert_eq!(s.value(), expected);
                    s.undo(&Undo, w, cx);
                    assert_eq!(s.value(), "abcde");
                    assert_eq!(s.selected_range(), 1..4);
                    s.redo(&super::Redo, w, cx);
                    assert_eq!(s.value(), expected);
                }
            });
        })
        .unwrap();
    }

    #[gpui::test]
    fn deletion_groups_end_on_new_press_direction_navigation_and_typing(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let window = cx.add_window(InputState::new);
        let editor = window.root(cx).unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_value("abcdef", w, cx);
                s.set_selected_range(3..3, cx);
                for (key, held) in [
                    ("backspace", false),
                    ("backspace", true),
                    ("delete", true),
                    ("delete", false),
                    ("delete", true),
                ] {
                    let mut deletion = event(key);
                    deletion.is_held = held;
                    s.key_on(&deletion, false, w, cx);
                }
                assert_eq!(s.value(), "a");
                for expected in ["aef", "adef", "abcdef"] {
                    s.undo(&Undo, w, cx);
                    assert_eq!(s.value(), expected);
                }

                s.set_value("abcdef", w, cx);
                s.set_selected_range(6..6, cx);
                s.key_on(&event("backspace"), false, w, cx);
                s.key_on(&event("left"), false, w, cx);
                s.key_on(&event("right"), false, w, cx);
                let mut held = event("backspace");
                held.is_held = true;
                s.key_on(&held, false, w, cx);
                s.undo(&Undo, w, cx);
                assert_eq!(s.value(), "abcde");
                s.redo(&super::Redo, w, cx);
                s.replace("X", w, cx);
                s.key_on(&held, false, w, cx);
                s.undo(&Undo, w, cx);
                assert_eq!(s.value(), "abcdX");
                s.undo(&Undo, w, cx);
                assert_eq!(s.value(), "abcd");
                s.undo(&Undo, w, cx);
                assert_eq!(s.value(), "abcde");

                s.set_value("aa bb cc dd", w, cx);
                s.set_selected_range(11..11, cx);
                s.key_on(&event("backspace"), false, w, cx);
                let mut word_deletion = event("ctrl-backspace");
                word_deletion.is_held = true;
                s.key_on(&word_deletion, false, w, cx);
                s.key_on(&word_deletion, false, w, cx);
                assert_eq!(s.value(), "aa bb ");
                s.undo(&Undo, w, cx);
                assert_eq!(s.value(), "aa bb cc d");
                s.undo(&Undo, w, cx);
                assert_eq!(s.value(), "aa bb cc dd");
            });
        })
        .unwrap();
    }

    #[gpui::test]
    fn held_multicaret_deletion_undo_survives_overlapping_and_collapsing_carets(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let window = cx.add_window(|w, cx| InputState::new(w, cx).soft_wrap(false));
        let editor = window.root(cx).unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                let original = "ab🙂 ab🙂";
                s.set_value(original, w, cx);
                s.set_selected_range(0.."ab🙂".len(), cx);
                s.select_next_occurrence(cx);
                s.key_on(&event("right"), false, w, cx);
                let selections: Vec<_> = s.selected_ranges().collect();
                assert_eq!(selections.len(), 2);
                for repeat in 0..5 {
                    let mut deletion = event("backspace");
                    deletion.is_held = repeat > 0;
                    s.key_on(&deletion, false, w, cx);
                }
                assert_eq!(s.value(), "");
                assert_eq!(s.undo.len(), 1);
                s.undo(&Undo, w, cx);
                assert_eq!(s.value(), original);
                assert_eq!(s.selected_ranges().collect::<Vec<_>>(), selections);
                s.redo(&super::Redo, w, cx);
                assert_eq!(s.value(), "");
                assert_eq!(s.selected_ranges().collect::<Vec<_>>(), vec![0..0]);
            });
        })
        .unwrap();
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
