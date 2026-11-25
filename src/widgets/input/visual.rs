//! Source offsets and their projection into the editable Markdown view.
use super::{
    reading_document::{self, Document, Kind},
    *,
};
use pulldown_cmark::{Event, Tag};
use std::borrow::Cow;
use std::sync::Arc;

struct InlineScope {
    source: Range<usize>,
    markers: [Range<usize>; 2],
    max_end: usize,
}

#[derive(Clone, Default)]
pub(super) struct DisplayMap(Vec<(Range<usize>, Range<usize>)>);

impl DisplayMap {
    pub(super) fn from_tabs(tabs: &[(usize, usize, usize)]) -> Self {
        Self(
            tabs.iter()
                .map(|&(source, display, width)| (source..source + 1, display..display + width))
                .collect(),
        )
    }

    pub(super) fn display_index(&self, byte: usize) -> usize {
        let index = self.0.partition_point(|(source, _)| source.end <= byte);
        if let Some((source, display)) = self.0.get(index)
            && source.contains(&byte)
        {
            return display.start;
        }
        index.checked_sub(1).map_or(byte, |previous| {
            let (source, display) = &self.0[previous];
            display.end + (byte - source.end)
        })
    }

    pub(super) fn source_index(&self, byte: usize) -> usize {
        let index = self.0.partition_point(|(_, display)| display.end <= byte);
        if let Some((source, display)) = self.0.get(index)
            && display.contains(&byte)
        {
            return source.start;
        }
        index.checked_sub(1).map_or(byte, |previous| {
            let (source, display) = &self.0[previous];
            source.end + (byte - display.end)
        })
    }

    pub(super) fn substitution_end(&self, byte: usize) -> Option<usize> {
        let index = self.0.partition_point(|(source, _)| source.end <= byte);
        self.0
            .get(index)
            .filter(|(source, display)| !display.is_empty() && source.contains(&byte))
            .map(|(source, _)| source.end)
    }
}

struct RichBlock {
    source: Range<usize>,
    blocks: Range<usize>,
}

struct VisualTable {
    source: Range<usize>,
    separator: Option<Range<usize>>,
    rows: Vec<Range<usize>>,
}

impl VisualTable {
    fn protected_end(&self) -> usize {
        self.separator
            .as_ref()
            .map_or(self.source.end, |line| line.end)
    }
}

#[derive(Clone, Copy)]
pub(super) struct RichRow {
    pub(super) index: usize,
    pub(super) height: Pixels,
}

#[derive(Default)]
pub(super) struct VisualDocument {
    document: Document,
    pub(super) spans: Vec<super::markdown::Span>,
    scopes: Vec<InlineScope>,
    tasks: Vec<usize>,
    rich: Vec<RichBlock>,
    cells: Vec<reading_document::TableCell>,
    tables: Vec<VisualTable>,
}

#[derive(Default)]
pub(super) struct VisualState {
    revision: Option<u64>,
    document: Arc<VisualDocument>,
    task: Option<Task<()>>,
    pub(super) revealed: Vec<usize>,
    pub(super) raw_block: Option<usize>,
    pub(super) geometry: super::reading_geometry::BlockGeometry,
    pub(super) rich_heights: Vec<Option<Pixels>>,
    pub(super) rich_style: Option<(Pixels, Pixels, Font)>,
    pub(super) rich_width: Pixels,
    copied: Option<usize>,
    copy_feedback: Option<Task<()>>,
}

impl InputState {
    pub fn set_editor_mode(&mut self, mode: EditorMode, cx: &mut Context<Self>) {
        if !self.multiline || self.is_read_only() || self.editor_mode() == mode {
            return;
        }
        self.stop_pointer_drag();
        self.remember_projection_anchor();
        self.visual = match mode {
            EditorMode::Source => None,
            EditorMode::Visual => Some(Box::default()),
        };
        self.layout_key = None;
        // Keep the editor entity, including the active IME session and history.
        self.reveal |= self.lines.is_empty();
        cx.notify();
    }

    pub fn editor_mode(&self) -> EditorMode {
        if self.visual.is_some() {
            EditorMode::Visual
        } else {
            EditorMode::Source
        }
    }

    pub(super) fn visual_document(&self) -> Option<&VisualDocument> {
        self.visual
            .as_ref()
            .filter(|visual| visual.revision == Some(self.revision))
            .map(|visual| visual.document.as_ref())
    }

    pub(super) fn ensure_visual(&mut self, cx: &mut Context<Self>) {
        let Some(visual) = &mut self.visual else {
            return;
        };
        if visual.revision == Some(self.revision) {
            return;
        }
        if self.content.len() <= 64 * 1024 {
            visual.install(self.revision, analyze(&self.content));
            return;
        }
        if visual.task.is_some() {
            return;
        }
        let mut snapshot = (self.revision, self.content.clone());
        visual.task = Some(cx.spawn(async move |this, cx| {
            loop {
                let (revision, text) = snapshot;
                let document = cx
                    .background_executor()
                    .spawn(async move { analyze(&text) })
                    .await;
                let Some(next) = this
                    .update(cx, |state, cx| {
                        let installed = state.visual.as_ref()?.revision;
                        if revision == state.revision {
                            state.remember_projection_anchor();
                            state.visual.as_mut().unwrap().install(revision, document);
                            state.layout_key = None;
                            cx.notify();
                        } else if installed != Some(state.revision) {
                            return Some((state.revision, state.content.clone()));
                        }
                        state.visual.as_mut().unwrap().task = None;
                        None
                    })
                    .ok()
                    .flatten()
                else {
                    return;
                };
                snapshot = next;
            }
        }));
    }

    pub(super) fn visual_reveal_points(&self) -> Vec<usize> {
        if self.visual_document().is_none() {
            return Vec::new();
        }
        if self.selecting {
            // Keep glyph positions steady from mouse-down through mouse-up.
            return self.visual.as_ref().unwrap().revealed.clone();
        }
        let document = self.visual_document().unwrap();
        let revealed = &self.visual.as_ref().unwrap().revealed;
        let scope_through_end = |at| {
            // Finish crossing the closing marker before hiding its glyphs.
            revealed
                .iter()
                .copied()
                .find(|&index| {
                    document
                        .scopes
                        .get(index)
                        .is_some_and(|scope| scope.source.end == at)
                })
                .or_else(|| document.scope_at(at))
        };
        let mut points: Vec<_> = self
            .selected_ranges()
            .filter_map(|range| {
                if range.is_empty() {
                    return scope_through_end(range.start);
                }
                let scope = document.scope_at(range.start)?;
                (scope_through_end(range.end) == Some(scope)).then_some(scope)
            })
            .collect();
        points.sort_unstable();
        points.dedup();
        points
    }

    pub(super) fn rich_caret(
        &self,
        block: usize,
        at: usize,
        after_wrap: bool,
    ) -> Option<(Point<Pixels>, Pixels)> {
        self.visual.as_ref()?.geometry.caret(block, at, after_wrap)
    }

    pub(super) fn visual_caret_height(&self, at: usize) -> Pixels {
        let ix = self
            .lines
            .partition_point(|line| line.start <= at)
            .saturating_sub(1);
        self.lines
            .get(ix)
            .filter(|line| line.rich.is_some())
            .and_then(|line| self.rich_caret(line.start, at, false))
            .map_or(self.line_height, |(_, height)| height)
    }

    pub(super) fn visual_cell(&self, at: usize) -> Option<Range<usize>> {
        let cell = self.visual_source_cell(at)?;
        Some(cell.text.start.min(at)..cell.text.end.max(at))
    }

    fn visual_source_cell(&self, at: usize) -> Option<&reading_document::TableCell> {
        let document = self.visual_document()?;
        self.visual_table(at)?;
        document.cell_index(at).map(|index| &document.cells[index])
    }

    fn visual_table(&self, at: usize) -> Option<&VisualTable> {
        let document = self.visual_document()?;
        let table = document.table_at(at)?;
        (self.visual.as_ref()?.raw_block != Some(table.source.start)).then_some(table)
    }

    pub(super) fn is_table_separator(&self, start: usize) -> bool {
        self.visual_table(start)
            .and_then(|table| table.separator.as_ref())
            .is_some_and(|separator| separator.start == start)
    }

    pub(super) fn table_caret(&self, from: usize, proposed: usize) -> usize {
        let Some(table) = self.visual_table(proposed) else {
            return proposed;
        };
        let Some(cells) = self.table_cells(table) else {
            return proposed;
        };
        let index = cells.partition_point(|cell| cell.source.end < proposed);
        if let Some(cell) = cells.get(index)
            && cell.source.start <= proposed
        {
            return proposed;
        }
        if proposed < from {
            cells.get(index.saturating_sub(1)).map_or(proposed, |cell| {
                if index == 0 {
                    cell.text.start
                } else {
                    cell.text.end
                }
            })
        } else {
            cells
                .get(index)
                .map(|cell| cell.text.start)
                .or_else(|| cells.last().map(|cell| cell.text.end))
                .unwrap_or(proposed)
        }
    }

    fn table_cells(&self, table: &VisualTable) -> Option<&[reading_document::TableCell]> {
        let document = self.visual_document()?;
        let first = table.rows.first()?.start;
        let last = table.rows.last()?.end;
        document.cells.get(first..last)
    }

    pub(super) fn visual_text_bounds(&self, at: usize) -> Option<Range<usize>> {
        if let Some(cell) = self.visual_cell(at) {
            return Some(cell);
        }
        let document = self.visual_document()?;
        let index = document.rich.partition_point(|block| block.source.end < at);
        let group = document.rich.get(index)?;
        if self.visual.as_ref()?.raw_block == Some(group.source.start) {
            return None;
        }
        for block in document.rich_blocks(index) {
            if block.prefix.is_empty()
                && block.quote == 0
                && !matches!(
                    block.kind,
                    Kind::Heading(_) | Kind::Code | Kind::Mermaid(Err(_))
                )
            {
                continue;
            }
            if let (Some(first), Some(last)) =
                (block.content.pieces.first(), block.content.pieces.last())
            {
                let start = if matches!(block.kind, Kind::Heading(_))
                    && document.scope_at(at).is_some_and(|index| {
                        document.scopes[index].source.start == block.source.start
                    }) {
                    block.source.start
                } else {
                    first.source.start
                };
                let end = start
                    + self.content[start..last.source.end]
                        .trim_end_matches(['\r', '\n'])
                        .len();
                if (start..=end).contains(&at) {
                    return Some(start..end);
                }
            }
        }
        None
    }

    pub(super) fn step_visual_cell(&self, at: usize, backwards: bool) -> Option<usize> {
        let table = self.visual_table(at)?;
        let cells = self.table_cells(table)?;
        let index = cells.partition_point(|cell| cell.source.end < at);
        let next = if backwards {
            index.checked_sub(1)
        } else {
            Some(index + 1)
        };
        Some(next.and_then(|index| cells.get(index)).map_or_else(
            || {
                let x = if backwards { px(1e8) } else { px(0.) };
                self.table_exit(table, at, backwards, x)
                    .map_or(at, |target| target.0)
            },
            |cell| {
                if backwards {
                    cell.text.end
                } else {
                    cell.text.start
                }
            },
        ))
    }

    pub(super) fn horizontal_visual_caret(&self, at: usize, backwards: bool) -> Option<usize> {
        self.visual_document()?;
        if self
            .layout_key
            .as_ref()
            .is_none_or(|key| key.0 != self.revision)
        {
            return None;
        }
        let row = self.row_for_source(at)?;
        row.rich?;
        let in_cell = self.visual_cell(at).is_some();
        if let Some(target) = self
            .visual
            .as_ref()?
            .geometry
            .horizontal_caret(row.start, at, backwards, in_cell)
        {
            return Some(target);
        }
        if in_cell {
            return self.step_visual_cell(at, backwards);
        }
        let x = if backwards { px(1e8) } else { px(0.) };
        Some(self.neighboring_visual_caret(at, backwards, x).map_or_else(
            || if backwards { 0 } else { self.content.len() },
            |target| target.0,
        ))
    }

    fn table_exit(
        &self,
        table: &VisualTable,
        at: usize,
        backwards: bool,
        x: Pixels,
    ) -> Option<(usize, bool)> {
        let fallback = if backwards {
            (table.source.start != 0).then(|| self.prev(table.source.start))
        } else {
            let end = table.protected_end();
            (end < self.content.len()).then(|| self.next(end))
        }?;
        if self
            .layout_key
            .as_ref()
            .is_some_and(|key| key.0 == self.revision)
        {
            return self
                .neighboring_visual_caret(at, backwards, x)
                .or(Some((fallback, false)));
        }
        Some((fallback, false))
    }

    pub(super) fn vertical_visual_cell(
        &self,
        at: usize,
        upwards: bool,
        x: Pixels,
    ) -> Option<(usize, bool)> {
        let table = self.visual_table(at)?;
        let document = self.visual_document()?;
        let cell_index = document.cell_index(at)?;
        let current_cell = &document.cells[cell_index];
        if let Some(wrapped) = self.visual.as_ref()?.geometry.wrapped_cell_caret(
            &current_cell.source,
            at,
            upwards,
            self.caret_after_wrap,
            x,
        ) {
            return Some(wrapped);
        }
        let row = table.rows.partition_point(|row| row.end <= cell_index);
        let current_row = table.rows.get(row)?;
        let column = cell_index.checked_sub(current_row.start)?;
        let direction = if upwards {
            row.checked_sub(1)
        } else {
            Some(row + 1)
        };
        let target_row = direction
            .and_then(|row| table.rows.get(row))
            .filter(|row| !row.is_empty());
        let Some(target_row) = target_row else {
            return Some(
                self.table_exit(table, at, upwards, x)
                    .unwrap_or((at, self.caret_after_wrap)),
            );
        };
        let target_cell = &document.cells[(target_row.start + column).min(target_row.end - 1)].text;
        let offset = unicode_segmentation::UnicodeSegmentation::graphemes(
            &self.content[current_cell.text.start.min(at)..at],
            true,
        )
        .count();
        let relative = unicode_segmentation::UnicodeSegmentation::grapheme_indices(
            &self.content[target_cell.clone()],
            true,
        )
        .nth(offset)
        .map_or(target_cell.len(), |(relative, _)| relative);
        Some((target_cell.start + relative, false))
    }

    pub(super) fn visual_table_input_range(&self, range: Range<usize>) -> Option<Range<usize>> {
        if range.is_empty() {
            let at = self.table_caret(self.cursor(), range.start);
            return Some(at..at);
        }
        if [range.start, range.end]
            .into_iter()
            .filter_map(|at| self.visual_table(at))
            .any(|table| {
                !(range.start <= table.source.start && range.end >= table.source.end)
                    && !self
                        .visual_source_cell(range.start)
                        .is_some_and(|cell| range.end <= cell.source.end)
            })
        {
            // A partial selection of multiple cells can be copied as Markdown.
            // Writing such a selection would delete invisible separators.
            return None;
        }
        Some(range)
    }

    pub(super) fn insert_after_visual_table(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(table) = self.visual_table(self.cursor()) else {
            return false;
        };
        let end = table.source.end;
        let after = self.next(end);
        // Keep the table's blank separator before the new editable line.
        let (at, leading_breaks, break_at) = match &table.separator {
            Some(separator) if separator.end < self.content.len() => {
                (self.next(separator.end), 0, separator.end)
            }
            Some(separator) => (separator.end, 1, end),
            None => (after, 1 + usize::from(after == end), end),
        };
        let line_break = if break_at < self.content.len() {
            &self.content[break_at..self.next(break_at)]
        } else {
            self.content[table.source.clone()]
                .find(['\r', '\n'])
                .map_or("\n", |offset| {
                    let start = table.source.start + offset;
                    &self.content[start..self.next(start)]
                })
        };
        // A new last line needs no terminator unless it is pushing down an
        // existing editable line, which can itself be the empty line at EOF.
        let trailing_breaks = usize::from(leading_breaks == 0 || at < self.content.len());
        let insert = line_break.repeat(leading_breaks + trailing_breaks);
        let caret = at + leading_breaks * line_break.len();
        let anchor = self.top_source_anchor();
        self.commit(at..at, &insert, false, cx);
        self.projection_anchor = anchor.map(|(source, y)| {
            let source = if source >= at {
                source + insert.len()
            } else {
                source
            };
            (source, y)
        });
        self.move_to(caret, false, cx);
        true
    }

    pub(super) fn visual_cell_input<'a>(
        &self,
        range: &Range<usize>,
        value: &'a str,
    ) -> Cow<'a, str> {
        if !value.contains(['|', '\r', '\n'])
            || !self
                .visual_source_cell(range.start)
                .is_some_and(|cell| range.end <= cell.source.end)
        {
            return Cow::Borrowed(value);
        }
        let value = value.replace("\r\n", " ").replace(['\r', '\n'], " ");
        let mut output = String::with_capacity(value.len());
        let mut escapes = self.content.as_bytes()[..range.start]
            .iter()
            .rev()
            .take_while(|&&byte| byte == b'\\')
            .count();
        for character in value.chars() {
            if character == '|' && escapes % 2 == 0 {
                output.push('\\');
            }
            if character == '\\' {
                escapes += 1;
            } else {
                escapes = 0;
            }
            output.push(character);
        }
        Cow::Owned(output)
    }

    pub(super) fn toggle_visual_task(
        &mut self,
        marker: usize,
        revision: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.revision != revision
            || self.is_composing()
            || !self
                .visual_document()
                .is_some_and(|document| document.tasks.binary_search(&marker).is_ok())
        {
            return false;
        }
        let replacement = match self.content.get(marker - 1..marker + 2) {
            Some("[ ]") => "x",
            Some("[x]" | "[X]") => " ",
            _ => return false,
        };
        let selection = (
            self.selection.clone(),
            self.reversed,
            self.caret_after_wrap,
            self.extra.clone(),
            self.occurrence_word,
        );
        self.commit(marker..marker + 1, replacement, false, cx);
        (
            self.selection,
            self.reversed,
            self.caret_after_wrap,
            self.extra,
            self.occurrence_word,
        ) = selection;
        self.reveal = false;
        true
    }

    pub(super) fn copy_visual_code(&mut self, block: usize, revision: u64, cx: &mut Context<Self>) {
        if self.revision != revision {
            return;
        }
        let Some(code) = self
            .visual_document()
            .and_then(|document| {
                document
                    .rich
                    .get(block)
                    .map(|_| document.rich_blocks(block))
            })
            .and_then(|blocks| blocks.first())
            .filter(|block| matches!(block.kind, Kind::Code | Kind::Mermaid(_)))
        else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(code.content.text.clone()));
        let visual = self.visual.as_mut().unwrap();
        visual.copied = Some(block);
        visual.copy_feedback = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |state, cx| {
                if let Some(visual) = &mut state.visual {
                    visual.copied = None;
                    visual.copy_feedback = None;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    pub(super) fn start_visual_block_edit(
        &mut self,
        index: usize,
        revision: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_visual_block_edit_at(index, revision, None, window, cx);
    }

    pub(super) fn start_visual_block_edit_at(
        &mut self,
        index: usize,
        revision: u64,
        offset: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.revision != revision || self.is_composing() {
            return;
        }
        let Some(document) = self.visual_document() else {
            return;
        };
        let Some(block) = document.rich.get(index) else {
            return;
        };
        let start = block.source.start;
        let at = offset
            .filter(|offset| block.source.contains(offset))
            .unwrap_or_else(|| {
                document.rich_blocks(index)[0]
                    .content
                    .pieces
                    .first()
                    .map_or(start, |piece| piece.source.start)
            });
        self.remember_projection_anchor();
        self.visual.as_mut().unwrap().raw_block = Some(start);
        self.layout_key = None;
        self.set_selected_range(at..at, cx);
        self.reveal = false;
        self.focus(window, cx);
    }

    pub(super) fn close_visual_block_edit(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(visual) = &mut self.visual else {
            return false;
        };
        if visual.raw_block.take().is_none() {
            return false;
        }
        self.remember_projection_anchor();
        self.layout_key = None;
        cx.notify();
        true
    }

    pub(super) fn prepaint_visual_blocks(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self.visual_document().is_none() {
            return Vec::new();
        }
        let caret = self.cursor();
        if self.reveal
            && let Some(row) = self.row_for_source(caret)
        {
            if row.top + self.scroll.y >= self.bounds.size.height {
                self.scroll.y = self.bounds.size.height - row.top - self.line_height;
            } else if row.top + row.height() + self.scroll.y <= px(0.) {
                self.scroll.y = -row.top;
            }
        }
        let anchor = self.top_source_anchor();
        let document = self.visual_document().unwrap();
        let top = -self.scroll.y;
        let first = self
            .lines
            .partition_point(|row| row.top + row.height() <= top);
        let last = self
            .lines
            .partition_point(|row| row.top < top + self.bounds.size.height + px(240.));
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let visual = self.visual.as_ref().unwrap();
        let geometry = visual.geometry.clone();
        let selected: Vec<_> = self.selected_ranges().collect();
        let marked: Vec<_> = self
            .marked
            .iter()
            .chain(&self.extra_marked)
            .cloned()
            .collect();
        let mut elements = Vec::new();
        let width = self.bounds.size.width.max(px(1.));
        for (index, row) in self.lines.iter().enumerate().take(last).skip(first) {
            let Some(rich) = row.rich else {
                continue;
            };
            let mut element = super::reading::render_visual_blocks(
                document.rich_blocks(rich.index),
                super::reading::BlockView {
                    source: row.start,
                    index: rich.index,
                    font_size,
                    directory: &self.source_directory,
                    editor: cx.entity().downgrade(),
                    revision: self.revision,
                    copied: visual.copied == Some(rich.index),
                    raw: &self.content,
                    document,
                    revealed: &visual.revealed,
                    selected: &selected,
                    marked: &marked,
                    geometry: geometry.clone(),
                    wrap: self.wrap,
                },
                window,
                cx,
            );
            let height = element
                .layout_as_root(
                    size(AvailableSpace::Definite(width), AvailableSpace::MinContent),
                    window,
                    cx,
                )
                .height
                .max(self.line_height);
            elements.push((index, rich.index, height, element));
        }
        let mut resized = false;
        let visual = self.visual.as_mut().unwrap();
        for (index, block, height, _) in &elements {
            visual.rich_heights[*block] = Some(*height);
            let rich = self.lines[*index].rich.as_mut().unwrap();
            resized |= rich.height != *height;
            rich.height = *height;
        }
        if resized {
            let mut top = px(0.);
            for row in &mut self.lines {
                row.top = top;
                top += row.height();
            }
            if let Some(anchor) = anchor {
                self.restore_top_source_anchor(anchor);
            }
            self.content_size.height = top + self.content_padding.bottom;
            // Revisit the viewport when measured blocks pack more rows into it.
            cx.notify();
        }
        if self.reveal
            && self
                .row_for_source(caret)
                .is_some_and(|row| row.rich.is_none())
        {
            self.scroll = self.scroll_to_caret(self.caret_position(), self.line_height);
        }
        self.clamp_scroll();
        let origin = self.bounds.origin + self.scroll;
        geometry.begin(origin);
        let bounds = self.bounds;
        let elements = window.with_content_mask(Some(ContentMask { bounds }), |window| {
            elements
                .into_iter()
                .map(|(index, _, _, mut element)| {
                    element.prepaint_at(origin + point(px(0.), self.lines[index].top), window, cx);
                    element
                })
                .collect()
        });
        if !self.wrap {
            let width = geometry.required_width();
            let visual = self.visual.as_mut().unwrap();
            visual.rich_width = visual.rich_width.max(width);
            self.content_size.width = self.content_size.width.max(visual.rich_width);
        }
        if self.reveal {
            self.reveal = false;
            if let Some(row) = self.row_for_source(caret)
                && row.rich.is_some()
                && let Some((position, height)) =
                    self.rich_caret(row.start, caret, self.caret_after_wrap)
            {
                let old_scroll = self.scroll;
                let desired = self.scroll_to_caret(position, height);
                if desired != old_scroll {
                    let revision = self.revision;
                    let editor = cx.weak_entity();
                    cx.defer(move |cx| {
                        let _ = editor.update(cx, |state, cx| {
                            if state.revision == revision
                                && state.cursor() == caret
                                && state.scroll == old_scroll
                            {
                                state.scroll = desired;
                                state.clamp_scroll();
                                cx.notify();
                            }
                        });
                    });
                }
            }
        }
        elements
    }
}

impl VisualState {
    fn install(&mut self, revision: u64, document: VisualDocument) {
        self.revision = Some(revision);
        self.revealed.clear();
        self.copied = None;
        self.copy_feedback = None;
        self.rich_heights = vec![None; document.rich.len()];
        self.rich_width = px(0.);
        self.geometry.begin(Point::default());
        self.document = Arc::new(document);
    }
}

impl VisualDocument {
    fn table_at(&self, at: usize) -> Option<&VisualTable> {
        let index = self
            .tables
            .partition_point(|table| table.protected_end() < at);
        self.tables
            .get(index)
            .filter(|table| table.source.start <= at)
    }

    fn cell_index(&self, at: usize) -> Option<usize> {
        let index = self.cells.partition_point(|cell| cell.source.end < at);
        self.cells
            .get(index)
            .filter(|cell| cell.source.start <= at)
            .map(|_| index)
    }
    fn scope_at(&self, at: usize) -> Option<usize> {
        let end = self
            .scopes
            .partition_point(|scope| scope.source.start <= at);
        self.scopes[..end]
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, scope)| scope.max_end > at)
            .filter(|(_, scope)| scope.source.contains(&at))
            .min_by_key(|(_, scope)| scope.source.len())
            .map(|(index, _)| index)
    }

    pub(super) fn reveal_markers(
        &self,
        revealed: &[usize],
        source: &Range<usize>,
    ) -> Vec<Range<usize>> {
        let mut markers: Vec<_> = revealed
            .iter()
            .flat_map(|&index| {
                self.scopes
                    .get(index)
                    .into_iter()
                    .flat_map(|scope| scope.markers.iter())
            })
            .filter(|marker| {
                !marker.is_empty() && source.start <= marker.start && marker.end <= source.end
            })
            .cloned()
            .collect();
        markers.sort_unstable_by_key(|marker| marker.start);
        markers
    }

    pub(super) fn rich_at(&self, byte: usize) -> Option<(usize, Range<usize>)> {
        let index = self.rich.partition_point(|block| block.source.end <= byte);
        let block = self.rich.get(index)?;
        (block.source.start == byte).then(|| (index, block.source.clone()))
    }

    pub(super) fn rich_blocks(&self, index: usize) -> &[reading_document::Block] {
        &self.document.blocks[self.rich[index].blocks.clone()]
    }
}

fn label_range(source: &str) -> Option<Range<usize>> {
    if source.starts_with('<') && source.ends_with('>') {
        return Some(1..source.len() - 1);
    }
    let mut depth = 1;
    let mut escaped = false;
    for (index, ch) in source.strip_prefix('[')?.char_indices() {
        if std::mem::take(&mut escaped) {
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(1..index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn inline_markers(event: &Event<'_>, source: &str) -> Option<[Range<usize>; 2]> {
    let len = source.len();
    let inner = match event {
        Event::Start(Tag::Heading { level, .. }) => {
            let line = source.trim_end_matches(['\r', '\n']);
            if line.contains(['\r', '\n']) {
                return None;
            }
            let value = line.trim_start_matches(' ');
            let count = value.bytes().take_while(|&byte| byte == b'#').count();
            if count != *level as usize {
                return None;
            }
            let start = line.len() - value[count..].trim_start_matches([' ', '\t']).len();
            let trimmed = line.trim_end_matches([' ', '\t']);
            let title = trimmed.trim_end_matches('#');
            let end = if title.len() < trimmed.len() && title.ends_with([' ', '\t']) {
                title.trim_end_matches([' ', '\t']).len()
            } else {
                line.len()
            };
            return (start <= end).then_some([0..start, end..trimmed.len().max(end)]);
        }
        Event::Start(Tag::Emphasis | Tag::Strong | Tag::Strikethrough) => {
            let count = if matches!(event, Event::Start(Tag::Emphasis)) {
                1
            } else {
                2
            };
            let marker = *source.as_bytes().first()?;
            if len <= count * 2
                || !matches!(marker, b'*' | b'_' | b'~')
                || !source.as_bytes()[..count]
                    .iter()
                    .all(|&byte| byte == marker)
                || !source.as_bytes()[len - count..]
                    .iter()
                    .all(|&byte| byte == marker)
            {
                return None;
            }
            count..len - count
        }
        Event::Code(code) if !source.contains(['\r', '\n']) => {
            let count = source.bytes().take_while(|&byte| byte == b'`').count();
            if count == 0 || len <= count * 2 {
                return None;
            }
            let inner = &source[count..len - count];
            let padding = usize::from(
                inner.strip_prefix(' ').and_then(|s| s.strip_suffix(' ')) == Some(code.as_ref()),
            );
            count + padding..len - count - padding
        }
        Event::Start(Tag::Link { .. }) if !source.contains(['\r', '\n']) => label_range(source)?,
        _ => return None,
    };
    (!inner.is_empty()).then_some([0..inner.start, inner.end..len])
}

fn analyze(text: &str) -> VisualDocument {
    let mut visual = VisualDocument::default();
    let mut spans = super::markdown::SpanBuilder::default();
    let mut tables = Vec::new();
    let mut code_depth = 0;
    visual.document = reading_document::parse_with(text, |event, range| {
        spans.observe(text, event, range.clone());
        let source = &text[range.clone()];
        match event {
            Event::Start(Tag::Table(_)) => tables.push(range.clone()),
            Event::Start(Tag::CodeBlock(_)) => code_depth += 1,
            Event::End(pulldown_cmark::TagEnd::CodeBlock) => code_depth -= 1,
            Event::TaskListMarker(_) => visual.tasks.push(range.start + 1),
            _ if code_depth != 0 => {}
            _ => {
                if let Some(markers) = inline_markers(event, source) {
                    visual.scopes.push(InlineScope {
                        source: range.clone(),
                        markers: markers
                            .map(|part| range.start + part.start..range.start + part.end),
                        max_end: 0,
                    });
                }
            }
        }
    });
    visual.spans = spans.finish();
    visual.scopes.sort_by_key(|scope| scope.source.start);
    let mut furthest_end = 0;
    for scope in &mut visual.scopes {
        furthest_end = furthest_end.max(scope.source.end);
        scope.max_end = furthest_end;
    }
    let blocks = &visual.document.blocks;
    let mut tables = tables.into_iter().peekable();
    let mut representations = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if matches!(block.kind, Kind::TableRow { header: true, .. }) {
            while tables
                .peek()
                .is_some_and(|table| table.end <= block.source.start)
            {
                tables.next();
            }
            if let Some(source) = tables.next() {
                let count = blocks[index..]
                    .iter()
                    .take_while(|block| {
                        matches!(block.kind, Kind::TableRow { .. })
                            && block.source.start < source.end
                    })
                    .count();
                if count > 0 {
                    representations.push(RichBlock {
                        source,
                        blocks: index..index + count,
                    });
                }
            }
        } else if !matches!(block.kind, Kind::TableRow { .. }) {
            representations.push(RichBlock {
                source: block.source.clone(),
                blocks: index..index + 1,
            });
        }
    }
    let mut end = 0;
    for mut block in representations {
        let start = text[..block.source.start]
            .rfind(['\r', '\n'])
            .map_or(0, |byte| byte + 1);
        let trimmed_end = block.source.start
            + text[block.source.clone()]
                .trim_end_matches(['\r', '\n'])
                .len();
        let line_end = text[trimmed_end..]
            .find(['\r', '\n'])
            .map_or(text.len(), |byte| trimmed_end + byte);
        if start >= line_end {
            continue;
        }
        if start < end {
            if let Some(previous) = visual.rich.last_mut() {
                previous.source.end = previous.source.end.max(line_end);
                previous.blocks.end = block.blocks.end;
                end = previous.source.end;
            }
            continue;
        }
        end = line_end;
        block.source = start..line_end;
        if matches!(
            blocks[block.blocks.start].kind,
            Kind::TableRow { header: true, .. }
        ) {
            let mut rows = Vec::new();
            for row in &blocks[block.blocks.clone()] {
                let start = visual.cells.len();
                visual
                    .cells
                    .extend(row.cells.iter().filter_map(|cell| cell.cell.clone()));
                rows.push(start..visual.cells.len());
            }
            if rows.iter().any(|row| !row.is_empty()) {
                // An editable line after a table needs a separate blank line in
                // the source, or Markdown can consume it as another table row.
                let tail = &text[line_end..];
                let separator = tail
                    .strip_prefix("\r\n")
                    .or_else(|| tail.strip_prefix(['\r', '\n']))
                    .and_then(|rest| {
                        let start = text.len() - rest.len();
                        let line = rest.split(['\r', '\n']).next().unwrap_or_default();
                        line.trim_matches([' ', '\t'])
                            .is_empty()
                            .then_some(start..start + line.len())
                    });
                visual.tables.push(VisualTable {
                    source: block.source.clone(),
                    separator,
                    rows,
                });
            }
        }
        visual.rich.push(block);
    }
    visual
}

#[cfg(test)]
mod tests {
    use super::{EditorMode, InputState, analyze};
    use gpui::{
        KeyDownEvent, Keystroke, Modifiers, MouseButton, TestAppContext, VisualTestContext, point,
        px,
    };

    #[test]
    fn revealing_one_inline_scope_keeps_the_other_spans_rendered() {
        let text = "## 제목 **굵은 *한글🙂*** 그리고 [문서](https://example.org) &amp;";
        let document = analyze(text);
        let block = &document.document.blocks[0];
        let at = text.find("한글").unwrap();
        let scope = document.scope_at(at).unwrap();
        let markers = document.reveal_markers(&[scope], &block.content.source);
        let projection =
            super::super::reading_geometry::Projection::new(&block.content, text, &markers, false);
        assert_eq!(projection.text, "제목 굵은 *한글🙂* 그리고 문서 &");
        assert_eq!(
            projection.source_to_display(at),
            projection.text.find("한글").unwrap()
        );
        let heading = document.scope_at(0).unwrap();
        let markers = document.reveal_markers(&[heading], &block.content.source);
        let projection =
            super::super::reading_geometry::Projection::new(&block.content, text, &markers, false);
        assert_eq!(projection.text, "## 제목 굵은 한글🙂 그리고 문서 &");

        for (text, expected) in [
            ("#### Four ####\r\n", "#### Four ####"),
            ("##### Five\n", "##### Five"),
        ] {
            let document = analyze(text);
            let inline = &document.document.blocks[0].content;
            let active = document.scope_at(text.find('F').unwrap()).unwrap();
            let markers = document.reveal_markers(&[active], &inline.source);
            let projection =
                super::super::reading_geometry::Projection::new(inline, text, &markers, false);
            assert_eq!(projection.text, expected);
        }
    }

    #[gpui::test]
    fn code_and_table_remain_rendered_during_cell_editing(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let source = "앞\n\n~~~rust\nlet key = 1;\n~~~\n\n| 왼쪽 | 오른쪽 |\n| :--- | ---: |\n| A | [한글🙂](https://example.org) |\n\n끝";
        let code_start = source.find("~~~rust").unwrap();
        let table_start = source.find("| 왼쪽").unwrap();
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx)
                .soft_wrap(true)
                .default_value(source.into());
            input.set_editor_mode(EditorMode::Visual, cx);
            input.focus(window, cx);
            input
        });
        let editor = window.root(cx).unwrap();
        let mut view = VisualTestContext::from_window(window.into(), cx);
        let draw = |view: &mut VisualTestContext| {
            view.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        draw(&mut view);
        let code_copy = format!("visual-code-copy-{code_start}").leak();
        let code = view.debug_bounds(code_copy).unwrap().center();
        view.simulate_mouse_move(code, None, Modifiers::default());
        draw(&mut view);
        view.simulate_click(code, Modifiers::default());
        assert_eq!(
            cx.update(|cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "let key = 1;\n"
        );
        let code_name = format!("visual-block-{code_start}").leak();
        let (a, b) = editor.read_with(cx, |s, _| {
            let at = source.find("let key").unwrap();
            let origin = s.bounds.origin + s.scroll;
            let y = s.visual_caret_height(at) / 2.;
            (
                origin + s.position(at) + point(px(0.1), y),
                origin + s.position(at + "let".len()) + point(px(0.1), y),
            )
        });
        view.simulate_mouse_down(a, MouseButton::Left, Modifiers::default());
        view.simulate_mouse_move(b, MouseButton::Left, Modifiers::default());
        view.simulate_mouse_up(b, MouseButton::Left, Modifiers::default());
        draw(&mut view);
        assert_eq!(
            editor.read_with(cx, |s, _| s.selected_value().to_owned()),
            "let"
        );
        assert!(view.debug_bounds(code_name).is_some());
        let table_name = format!("visual-block-{table_start}").leak();
        let table = view.debug_bounds(table_name).unwrap();
        let cell = view.debug_bounds("reading-table-cell-1-1").unwrap();
        assert!(table.intersects(&cell));
        view.simulate_click(
            point(cell.right() - px(12.), cell.center().y),
            Modifiers {
                control: true,
                ..Default::default()
            },
        );
        assert_eq!(cx.opened_url().as_deref(), Some("https://example.org"));
        view.simulate_click(
            point(cell.right() - px(12.), cell.center().y),
            Modifiers::default(),
        );
        draw(&mut view);
        assert!(view.debug_bounds(table_name).is_some());
        view.update(|window, cx| {
            editor.update(cx, |s, cx| {
                let range = s.visual_cell(s.cursor()).expect("caret in the chosen cell");
                assert!(s.value()[range].contains("한글"));
                s.set_selected_range(
                    source.find("한글").unwrap()..source.find("한글").unwrap(),
                    cx,
                );
                s.replace("새|", window, cx);
            });
        });
        draw(&mut view);
        assert!(view.debug_bounds(table_name).is_some());
        assert!(editor.read_with(cx, |s, _| s.value().contains(r"새\|한글")));
    }

    #[gpui::test]
    fn table_movement_excludes_pipes_and_the_alignment_row(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let source = "| A | B |\r\n| --- | ---: |\r\n| 한글 | last |";
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx)
                .soft_wrap(true)
                .default_value(source.into());
            input.set_editor_mode(EditorMode::Visual, cx);
            input.focus(window, cx);
            input
        });
        let editor = window.root(cx).unwrap();
        let mut view = VisualTestContext::from_window(window.into(), cx);
        view.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        view.update(|window, cx| {
            editor.update(cx, |s, cx| {
                let key = |s: &mut InputState,
                           name,
                           window: &mut gpui::Window,
                           cx: &mut gpui::Context<InputState>| {
                    s.key(
                        &KeyDownEvent {
                            keystroke: Keystroke::parse_with_os(
                                name,
                                crate::shortcuts::KEYBOARD_OS,
                            )
                            .unwrap(),
                            is_held: false,
                            prefer_character_input: false,
                        },
                        window,
                        cx,
                    );
                };
                let cell = |s: &InputState| {
                    s.visual_cell(s.cursor())
                        .map(|range| s.content[range].to_owned())
                };
                assert_eq!(cell(s).as_deref(), Some("A"));
                for direction in ["left", "up"] {
                    key(s, direction, window, cx);
                    assert_eq!(cell(s).as_deref(), Some("A"));
                }
                key(s, "tab", window, cx);
                assert_eq!(cell(s).as_deref(), Some("B"));
                key(s, "down", window, cx);
                assert_eq!(cell(s).as_deref(), Some("last"));
                for direction in ["end", "right", "down", "ctrl-end", "tab"] {
                    key(s, direction, window, cx);
                    assert_eq!(s.cursor(), source.find("last").unwrap() + 4);
                }
                key(s, "delete", window, cx);
                assert_eq!(s.value(), source);
                s.replace("!", window, cx);
                assert_eq!(s.value(), source.replace("last", "last!"));
                key(s, "ctrl-enter", window, cx);
                s.replace("outside", window, cx);
                assert_eq!(
                    s.value(),
                    format!("{}\r\n\r\noutside", source.replace("last", "last!"))
                );
            });
        });
    }

    #[gpui::test]
    fn vertical_table_arrows_preserve_the_grapheme_offset_inside_a_cell(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let source =
            "현황\n\n| 가👩‍💻나다 | 라🇰🇷마바 |\n| --- | ---: |\n| A🇰🇷BC | D👩‍💻EF |\n| X | Y |\n\n끝";
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx)
                .soft_wrap(true)
                .default_value(source.into());
            input.set_editor_mode(EditorMode::Visual, cx);
            input.focus(window, cx);
            input
        });
        let editor = window.root(cx).unwrap();
        let mut view = VisualTestContext::from_window(window.into(), cx);
        let draw = |view: &mut VisualTestContext| {
            view.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        let within = |prefix: &str| source.find(prefix).unwrap() + prefix.len();
        for (in_cell, in_header, in_short_cell) in [("A🇰🇷", "가👩‍💻", "X"), ("D👩‍💻", "라🇰🇷", "Y")]
        {
            let at = within(in_cell);
            view.update(|_, cx| editor.update(cx, |s, cx| s.set_selected_range(at..at, cx)));
            draw(&mut view);
            for (key, expected) in [
                ("up", within(in_header)),
                ("down", at),
                ("down", within(in_short_cell)),
            ] {
                view.simulate_keystrokes(key);
                draw(&mut view);
                editor.read_with(cx, |s, _| {
                    assert_eq!(s.cursor(), expected);
                    assert_eq!(s.value(), source);
                });
            }
        }
    }

    #[gpui::test]
    fn table_edges_and_ctrl_enter_use_visible_editor_lines(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let source =
            "Before\r\n\r\n| Left | Right |\r\n| :--- | ---: |\r\n| west | east |\r\n\r\nAfter";
        let table_start = source.find("| Left").unwrap();
        let before_blank = "Before\r\n".len();
        let after = source.find("After").unwrap();
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx)
                .soft_wrap(true)
                .default_value(source.into());
            input.set_editor_mode(EditorMode::Visual, cx);
            input.focus(window, cx);
            input
        });
        let editor = window.root(cx).unwrap();
        let mut view = VisualTestContext::from_window(window.into(), cx);
        let draw = |view: &mut VisualTestContext| {
            view.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        draw(&mut view);
        let layout = |view: &mut VisualTestContext| {
            view.update(|_, cx| {
                let s = editor.read(cx);
                let separator = s.row_for_source(after - 2).unwrap();
                assert!(separator.compact);
                assert!(s.row_for_source(before_blank).unwrap().height() > px(0.));
                (s.row_for_source(after).unwrap().top, s.content_size.height)
            })
        };
        let original_layout = layout(&mut view);
        let press = |view: &mut VisualTestContext, name, at| {
            view.update(|window, cx| {
                editor.update(cx, |s, cx| {
                    s.set_selected_range(at..at, cx);
                    s.key(
                        &KeyDownEvent {
                            keystroke: Keystroke::parse_with_os(
                                name,
                                crate::shortcuts::KEYBOARD_OS,
                            )
                            .unwrap(),
                            is_held: false,
                            prefer_character_input: false,
                        },
                        window,
                        cx,
                    );
                    s.cursor()
                })
            })
        };
        let left = source.find("Left").unwrap();
        let east = source.find("east").unwrap();
        for (key, at, target) in [
            ("up", left + 2, before_blank),
            ("left", left, before_blank),
            ("down", 2, before_blank),
            ("right", east + 4, after),
            ("backspace", after, after),
        ] {
            assert_eq!(press(&mut view, key, at), target);
            draw(&mut view);
            assert_eq!(layout(&mut view), original_layout);
            assert!(editor.read_with(cx, |s, _| s.value() == source));
        }
        let entry = press(&mut view, "down", before_blank);
        assert!(source.find("Left").unwrap() <= entry && entry < source.find("Right").unwrap());
        assert!((after..=source.len()).contains(&press(&mut view, "down", east + 1)));
        let entry = press(&mut view, "up", after + 2);
        let west = source.find("west").unwrap();
        assert!((west..=west + "west".len()).contains(&entry));
        assert_eq!(press(&mut view, "ctrl-enter", east + 1), after);
        draw(&mut view);
        editor.read_with(cx, |s, _| {
            let caret = s.caret_position();
            let row = s.row_for_source(s.cursor()).unwrap();
            let table = s.row_for_source(table_start).unwrap();
            let next = s.row_for_source(s.value().find("After").unwrap()).unwrap();
            assert!(row.rich.is_none() && !row.compact);
            assert_eq!(row.start, row.end);
            assert_eq!(caret.y, row.top);
            assert!(caret.y >= table.top + table.height() && caret.y < next.top);
            assert!(caret.y + s.scroll.y >= px(0.));
            assert_eq!(next.top, original_layout.0 + s.line_height);
        });
        view.update(|window, cx| editor.update(cx, |s, cx| s.replace("가", window, cx)));
        draw(&mut view);
        editor.read_with(cx, |s, _| {
            assert_eq!(s.value(), format!("{}가\r\nAfter", &source[..after]));
            assert_eq!(s.cursor(), after + "가".len());
            assert_eq!(s.visual_document().unwrap().tables[0].source.end, after - 4);
        });
    }

    #[gpui::test]
    fn ctrl_enter_adds_one_editable_row_even_if_one_already_exists(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let table = "| Left | Right |\n| --- | --- |\n| west | east |";
        let cell = table.find("east").unwrap();
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx).soft_wrap(true);
            input.set_editor_mode(EditorMode::Visual, cx);
            input.focus(window, cx);
            input
        });
        let editor = window.root(cx).unwrap();
        let mut view = VisualTestContext::from_window(window.into(), cx);
        let draw = |view: &mut VisualTestContext| {
            view.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        for (suffix, expected, caret_in_suffix) in [
            ("\n\nAfter", "\n\n\nAfter", 2),
            ("\n\n\nAfter", "\n\n\n\nAfter", 2),
            ("\n# After", "\n\n\n# After", 2),
            ("", "\n\n", 2),
            ("\n", "\n\n", 2),
            ("\n\n", "\n\n\n", 2),
            ("\r\n \t\r\nAfter", "\r\n \t\r\n\r\nAfter", 6),
            ("\r\n\nAfter", "\r\n\n\nAfter", 3),
        ] {
            let source = format!("{table}{suffix}");
            let edited = format!("{table}{expected}");
            let caret = table.len() + caret_in_suffix;
            view.update(|window, cx| {
                editor.update(cx, |s, cx| {
                    s.set_value(&source, window, cx);
                    s.set_selected_range(cell..cell, cx);
                });
            });
            draw(&mut view);
            let height = editor.read_with(cx, |s, _| s.content_size.height);
            view.simulate_keystrokes("ctrl-enter");
            draw(&mut view);
            editor.read_with(cx, |s, _| {
                assert_eq!(s.value(), edited, "{suffix:?}");
                assert_eq!(s.cursor(), caret, "{suffix:?}");
                let row = s.row_for_source(caret).unwrap();
                let table_row = s.row_for_source(cell).unwrap();
                assert_eq!(row.start..row.end, caret..caret, "{suffix:?}");
                assert!(row.rich.is_none() && !row.compact, "{suffix:?}");
                assert_eq!(row.top, table_row.top + table_row.height(), "{suffix:?}");
                assert_eq!(s.content_size.height, height + s.line_height, "{suffix:?}");
            });
            view.simulate_keystrokes("ctrl-z");
            draw(&mut view);
            editor.read_with(cx, |s, _| {
                assert_eq!(s.value(), source);
                assert_eq!(s.cursor(), cell);
                assert_eq!(s.content_size.height, height);
            });
        }
    }

    #[gpui::test]
    fn consecutive_enter_keeps_every_blank_line_visible(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let before = "**앞**";
        let after = "뒤";
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx)
                .soft_wrap(true)
                .default_value(format!("{before}{after}"));
            input.set_editor_mode(EditorMode::Visual, cx);
            input.set_selected_range(before.len()..before.len(), cx);
            input.focus(window, cx);
            input
        });
        let editor = window.root(cx).unwrap();
        let handle = window.into();
        let mut view = VisualTestContext::from_window(handle, cx);
        let draw = |view: &mut VisualTestContext| {
            view.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        draw(&mut view);
        let mut y = editor.read_with(cx, |s, _| s.caret_position().y);
        for breaks in 1..=3 {
            view.simulate_keystrokes("enter");
            draw(&mut view);
            editor.read_with(cx, |s, _| {
                assert_eq!(s.value(), format!("{before}{}{after}", "\n".repeat(breaks)));
                assert_eq!(s.cursor(), before.len() + breaks);
                assert!(s.caret_position().y > y);
                y = s.caret_position().y;
            });
        }
        view.simulate_keystrokes("up");
        draw(&mut view);
        editor.read_with(cx, |s, _| {
            assert_eq!(s.cursor(), before.len() + 2);
            for at in [before.len() + 1, before.len() + 2] {
                let row = s.row_for_source(at).unwrap();
                assert_eq!(row.start..row.end, at..at);
                assert_eq!(row.height(), s.line_height);
            }
            assert_eq!(s.position(before.len() + 3).y, y);
        });
    }

    #[gpui::test]
    fn typed_spaces_have_caret_width_before_the_next_character(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx).soft_wrap(true);
            input.set_editor_mode(EditorMode::Visual, cx);
            input.focus(window, cx);
            input
        });
        let editor = window.root(cx).unwrap();
        let mut view = VisualTestContext::from_window(window.into(), cx);
        let draw = |view: &mut VisualTestContext| {
            view.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        let table = "| A | B |\n| --- | ---: |\n| 왼쪽 |오른쪽|\n\n끝";
        for (source, at) in [
            ("마침", "마침".len()),
            ("마침\r\n계속", "마침".len()),
            ("# 제목", "# 제목".len()),
            ("들여쓰기", 0),
            ("앞\n계속", "앞\n".len()),
            (table, table.find("왼쪽").unwrap() + "왼쪽".len()),
            (table, table.find("오른쪽").unwrap() + "오른쪽".len()),
        ] {
            view.update(|window, cx| {
                editor.update(cx, |s, cx| {
                    s.set_value(source, window, cx);
                    s.set_selected_range(at..at, cx);
                });
            });
            draw(&mut view);
            for count in 1..=3 {
                view.update(|window, cx| editor.update(cx, |s, cx| s.replace(" ", window, cx)));
                draw(&mut view);
                editor.read_with(cx, |s, _| {
                    let expected =
                        format!("{}{}{}", &source[..at], " ".repeat(count), &source[at..]);
                    assert_eq!(s.value(), expected, "{source:?}");
                    assert_eq!(s.cursor(), at + count, "{source:?}");
                    let before = s.position(at + count - 1);
                    let caret = s.caret_position();
                    assert_eq!(before.y, caret.y, "{source:?}");
                    assert!(
                        before.x < caret.x,
                        "space {count} in {source:?}: {before:?} -> {caret:?}"
                    );
                });
            }
            view.update(|window, cx| editor.update(cx, |s, cx| s.replace("X", window, cx)));
            draw(&mut view);
            editor.read_with(cx, |s, _| {
                assert_eq!(s.value(), format!("{}   X{}", &source[..at], &source[at..]));
                assert_eq!(s.cursor(), at + 4);
            });
        }
    }

    #[gpui::test]
    fn horizontal_arrows_pass_hidden_markdown_in_one_step(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let source = "앞\n- [ ] task &amp; &fjlig; tail\n\n~~~text\ncode\n~~~\n\n***bold*** next\n\n[link](https://example.org) end";
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx)
                .soft_wrap(true)
                .default_value(source.into());
            input.set_editor_mode(EditorMode::Visual, cx);
            input.focus(window, cx);
            input
        });
        let editor = window.root(cx).unwrap();
        let mut view = VisualTestContext::from_window(window.into(), cx);
        let draw = |view: &mut VisualTestContext| {
            view.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        let place = |view: &mut VisualTestContext, at| {
            view.update(|_, cx| editor.update(cx, |s, cx| s.set_selected_range(at..at, cx)));
            draw(view);
        };
        let step = |view: &mut VisualTestContext, key, expected| {
            let position = view.update(|_, cx| editor.read(cx).caret_position());
            view.simulate_keystrokes(key);
            draw(view);
            view.update(|_, cx| {
                let s = editor.read(cx);
                assert_eq!(s.cursor(), expected);
                assert_ne!(s.caret_position(), position, "{key} at {expected}");
                assert_eq!(s.value(), source);
            });
        };
        let task = source.find("task").unwrap();
        place(&mut view, task);
        step(&mut view, "left", "앞".len());
        let entity = source.find("&amp;").unwrap();
        place(&mut view, entity);
        step(&mut view, "right", entity + "&amp;".len());
        step(&mut view, "left", entity);
        let entity = source.find("&fjlig;").unwrap();
        place(&mut view, entity);
        step(&mut view, "right", entity + "&fjlig;".len());
        step(&mut view, "left", entity);
        let code = source.find("code").unwrap();
        place(&mut view, code + "code".len());
        step(&mut view, "right", source.find("\n\n***").unwrap() + 1);

        let link_end = source.find(") end").unwrap() + 1;
        place(&mut view, link_end - 1);
        step(&mut view, "right", link_end);
        step(&mut view, "right", link_end + 1);

        let bold_end = source.find("*** next").unwrap();
        place(&mut view, bold_end);
        for expected in [bold_end + 1, bold_end + 2] {
            step(&mut view, "right", expected);
        }
    }

    #[gpui::test]
    fn vertical_arrows_reach_the_start_of_wrapped_lines(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let prose = "wrap this through a few different visual lines ".repeat(6);
        let source =
            format!("{prose}\n\n| Heading | Value |\n| --- | --- |\n| {prose} | row |\n\nAfter");
        let table = source.find("| Heading").unwrap();
        let cell = source.find("| wrap this").unwrap() + 2;
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx)
                .soft_wrap(true)
                .default_value(source);
            input.set_editor_mode(EditorMode::Visual, cx);
            input.focus(window, cx);
            input
        });
        cx.simulate_window_resize(window.into(), gpui::size(px(340.), px(650.)));
        let editor = window.root(cx).unwrap();
        let mut view = VisualTestContext::from_window(window.into(), cx);
        let draw = |view: &mut VisualTestContext| {
            view.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        for (at, within) in [(0, 0..table), (cell, cell..cell + prose.trim_end().len())] {
            view.update(|_, cx| editor.update(cx, |s, cx| s.set_selected_range(at..at, cx)));
            draw(&mut view);
            let (start, line_height) =
                editor.read_with(cx, |s, _| (s.caret_position(), s.visual_caret_height(at)));
            view.simulate_keystrokes("down");
            draw(&mut view);
            editor.read_with(cx, |s, _| {
                assert!(within.contains(&s.cursor()));
                assert_eq!(s.caret_position().y, start.y + line_height);
                assert_eq!(s.caret_position().x, start.x);
                assert!(s.caret_after_wrap);
            });
            view.simulate_keystrokes("down");
            draw(&mut view);
            editor.read_with(cx, |s, _| {
                assert!(within.contains(&s.cursor()));
                assert_eq!(s.caret_position().y, start.y + 2 * line_height);
            });
            for _ in 0..2 {
                view.simulate_keystrokes("up");
                draw(&mut view);
            }
            editor.read_with(cx, |s, _| {
                assert_eq!(s.cursor(), at);
                assert_eq!(s.caret_position(), start);
            });
        }
    }

    #[gpui::test]
    fn revealing_and_hiding_markdown_keeps_the_viewport_anchor(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let leading = "앞쪽 문단\n\n".repeat(20);
        let source = format!(
            "{leading}기준 줄\n\n[link](https://example.org/{})\n\n```text\ncode\n```\n\n바깥 문단\n\n{}",
            "long-path/".repeat(24),
            "뒤쪽 문단\n\n".repeat(35),
        );
        let link = source.find("link").unwrap() + 2;
        let fence = source.find("```text").unwrap();
        let outside = source.find("바깥").unwrap();
        let top = leading.len();
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx)
                .soft_wrap(true)
                .default_value(source);
            input.set_editor_mode(EditorMode::Visual, cx);
            input.focus(window, cx);
            input
        });
        cx.simulate_window_resize(window.into(), gpui::size(px(460.), px(460.)));
        let editor = window.root(cx).unwrap();
        let mut view = VisualTestContext::from_window(window.into(), cx);
        let draw = |view: &mut VisualTestContext| {
            view.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        draw(&mut view);
        view.update(|_, cx| {
            editor.update(cx, |s, cx| {
                let y = s.row_for_source(top).unwrap().top;
                s.set_scroll_offset(point(px(0.), -y - px(3.)), cx);
            });
        });
        draw(&mut view);
        let anchor = editor.read_with(cx, |s, _| s.top_source_anchor().unwrap());
        assert_eq!(anchor.0, top);
        let click = |view: &mut VisualTestContext, at| {
            let position = view.update(|_, cx| {
                let s = editor.read(cx);
                s.bounds.origin
                    + s.scroll
                    + s.position(at)
                    + point(px(0.1), s.visual_caret_height(at) / 2.)
            });
            view.simulate_click(position, Modifiers::default());
            draw(view);
        };
        let assert_anchor = |cx: &mut TestAppContext| {
            let current = editor.read_with(cx, |s, _| s.top_source_anchor().unwrap());
            assert_eq!(current.0, anchor.0);
            assert!((current.1 - anchor.1).abs() < px(0.1));
        };
        let height = |cx: &mut TestAppContext| {
            editor.read_with(cx, |s, _| s.row_for_source(link).unwrap().height())
        };
        let rendered_height = height(cx);
        for _ in 0..3 {
            click(&mut view, link);
            assert_anchor(cx);
            assert!(height(cx) > rendered_height);
            click(&mut view, outside);
            assert_anchor(cx);
            assert_eq!(height(cx), rendered_height);
        }
        let selector = format!("visual-code-edit-{fence}").leak();
        let button = view.debug_bounds(selector).unwrap().center();
        view.simulate_mouse_move(button, None, Modifiers::default());
        draw(&mut view);
        view.simulate_click(button, Modifiers::default());
        draw(&mut view);
        assert_anchor(cx);
        assert!(editor.read_with(cx, |s, _| s.row_for_source(fence).unwrap().rich.is_none()));
        click(&mut view, outside);
        assert_anchor(cx);
        assert!(editor.read_with(cx, |s, _| s.row_for_source(fence).unwrap().rich.is_some()));
    }

    #[gpui::test]
    fn headings_and_checkboxes_keep_their_rendered_geometry_when_selected(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let source =
            "## 제목 **굵게**와 *기울임*\r\n\r\n### 하위 제목\r\n\r\n- [ ] 체크할 **한글**\r\n";
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx)
                .soft_wrap(true)
                .default_value(source.into());
            input.set_editor_mode(EditorMode::Visual, cx);
            input.focus(window, cx);
            input
        });
        let editor = window.root(cx).unwrap();
        let mut view = VisualTestContext::from_window(window.into(), cx);
        let draw = |view: &mut VisualTestContext| {
            view.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        let position = |cx: &mut TestAppContext, label| {
            let at = source.find(label).unwrap();
            editor.read_with(cx, |s, _| {
                let height = s.visual_caret_height(at);
                (
                    s.bounds.origin + s.scroll + s.position(at) + point(px(0.2), height / 2.),
                    height,
                )
            })
        };
        draw(&mut view);
        let (bold, h2) = position(cx, "굵게");
        let (_, h3) = position(cx, "하위");
        assert!(h2 > h3);
        view.simulate_click(bold, Modifiers::default());
        draw(&mut view);
        assert_eq!(position(cx, "굵게").1, h2);
        assert_eq!(position(cx, "하위").1, h3);
        assert_eq!(
            editor.read_with(cx, |s, _| s.visual.as_ref().unwrap().revealed.len()),
            1
        );

        let marker = source.find("[ ]").unwrap() + 1;
        let selector = format!("visual-task-{marker}").leak();
        let checkbox = view.debug_bounds(selector).unwrap();
        view.simulate_click(position(cx, "한글").0, Modifiers::default());
        draw(&mut view);
        assert_eq!(view.debug_bounds(selector), Some(checkbox));
        view.simulate_click(checkbox.center(), Modifiers::default());
        draw(&mut view);
        assert_eq!(view.debug_bounds(selector), Some(checkbox));
        assert_eq!(
            editor.read_with(cx, |s, _| s.value().to_string()),
            source.replace("[ ]", "[x]")
        );
    }

    #[gpui::test]
    fn background_projection_ignores_old_offsets(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let initial = "# 이전\n\n**본문**\n\n".repeat(6000);
        let latest = "# 최신\n\n**한글🙂**\n\n".repeat(6000);
        let window = cx.add_window(|window, cx| {
            let mut input = InputState::new(window, cx)
                .soft_wrap(true)
                .default_value(initial);
            input.set_editor_mode(EditorMode::Visual, cx);
            assert!(input.visual_document().is_none());
            input.ensure_visual(cx);
            assert!(input.visual.as_ref().unwrap().task.is_some());
            input.set_value(latest.clone(), window, cx);
            assert!(input.visual_document().is_none());
            input
        });
        let editor = window.root(cx).unwrap();
        cx.run_until_parked();
        cx.read(|cx| {
            let editor = editor.read(cx);
            let document = editor.visual_document().unwrap();
            assert_eq!(editor.value(), latest);
            assert_eq!(document.document.blocks[1].content.text, "한글🙂");
        });
    }

    #[gpui::test]
    fn switching_mode_keeps_the_scrolled_source_position(cx: &mut TestAppContext) {
        cx.update(crate::widgets::init);
        let text = "## 제목\n\n본문\n\n".repeat(70);
        let window = cx.add_window(|window, cx| {
            InputState::new(window, cx)
                .soft_wrap(true)
                .default_value(text)
        });
        let editor = window.root(cx).unwrap();
        let mut view = VisualTestContext::from_window(window.into(), cx);
        let draw = |view: &mut VisualTestContext| {
            view.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        };
        draw(&mut view);
        view.update(|_, cx| {
            editor.update(cx, |s, cx| {
                s.set_scroll_offset(point(px(0.), px(-915.)), cx)
            })
        });
        draw(&mut view);
        let anchor = |view: &mut VisualTestContext| {
            view.update(|_, cx| {
                let s = editor.read(cx);
                let y = s.scroll_offset().y;
                let line = s
                    .lines
                    .iter()
                    .find(|line| line.top + line.height() > -y)
                    .unwrap();
                (line.start, line.top + y)
            })
        };
        let (source, y) = anchor(&mut view);
        for mode in [EditorMode::Visual, EditorMode::Source] {
            view.update(|_, cx| editor.update(cx, |s, cx| s.set_editor_mode(mode, cx)));
            draw(&mut view);
            let (current_source, current_y) = anchor(&mut view);
            assert_eq!(current_source, source);
            assert!((current_y - y).abs() < px(0.1));
        }
    }
}
