//! Positions shared by the rendered Markdown blocks and the source editor.
use super::{
    reading_document::{Inline, SourcePiece},
    *,
};
use std::{cell::RefCell, rc::Rc};

pub(super) struct Projection {
    pub(super) text: String,
    pub(super) pieces: Vec<SourcePiece>,
    pub(super) source: Range<usize>,
    insertions: Vec<(usize, usize)>,
}

impl Projection {
    pub(super) fn new(inline: &Inline, raw: &str, markers: &[Range<usize>], code: bool) -> Self {
        let text = if code {
            inline.text.trim_end_matches('\n')
        } else {
            &inline.text
        };
        let mut positions: Vec<_> = markers
            .iter()
            .filter_map(|range| {
                raw.get(range.clone()).map(|_| {
                    (
                        source_to_display(&inline.pieces, range.start).min(text.len()),
                        range,
                    )
                })
            })
            .collect();
        positions.sort_by_key(|&(display, range)| (display, range.start));
        let mut projection = Self {
            text: String::with_capacity(text.len()),
            pieces: Vec::with_capacity(inline.pieces.len() + positions.len()),
            source: inline.cell.as_ref().unwrap_or(&inline.source).clone(),
            insertions: Vec::with_capacity(positions.len()),
        };
        let mut markers = positions.into_iter().peekable();
        for piece in &inline.pieces {
            while let Some((display, range)) =
                markers.next_if(|(display, _)| *display <= piece.display.start)
            {
                projection.push_marker(raw, display, range);
            }
            if piece.display.start >= text.len() {
                break;
            }
            let end = piece.display.end.min(text.len());
            let content = &text[piece.display.start..end];
            if code && piece.verbatim && content.contains('\t') {
                let mut start = 0;
                for (index, _) in content.match_indices('\t') {
                    projection.push(
                        &content[start..index],
                        piece.source.start + start..piece.source.start + index,
                        true,
                    );
                    projection.push(
                        "    ",
                        piece.source.start + index..piece.source.start + index + 1,
                        false,
                    );
                    start = index + 1;
                }
                projection.push(
                    &content[start..],
                    piece.source.start + start..piece.source.start + content.len(),
                    true,
                );
            } else {
                let source_end = if piece.verbatim {
                    piece.source.start + content.len()
                } else {
                    piece.source.end
                };
                projection.push(content, piece.source.start..source_end, piece.verbatim);
            }
        }
        for (display, range) in markers {
            projection.push_marker(raw, display, range);
        }
        projection
    }

    fn push_marker(&mut self, raw: &str, display: usize, range: &Range<usize>) {
        self.push(&raw[range.clone()], range.clone(), true);
        self.insertions.push((display, range.len()));
    }

    fn push(&mut self, text: &str, source: Range<usize>, verbatim: bool) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        self.pieces.push(SourcePiece {
            source,
            display: start..self.text.len(),
            verbatim,
        });
    }

    pub(super) fn styled_range(&self, range: &Range<usize>) -> Range<usize> {
        let start = self
            .insertions
            .iter()
            .filter(|(at, _)| *at <= range.start)
            .map(|(_, len)| len)
            .sum::<usize>();
        let end = self
            .insertions
            .iter()
            .filter(|(at, _)| *at < range.end)
            .map(|(_, len)| len)
            .sum::<usize>();
        (range.start + start).min(self.text.len())..(range.end + end).min(self.text.len())
    }

    pub(super) fn source_to_display(&self, at: usize) -> usize {
        source_to_display(&self.pieces, at)
    }

    fn step_source(&self, at: usize, backwards: bool) -> Option<usize> {
        let display = self.source_to_display(at);
        let target = if backwards {
            self.text
                .get(..display)?
                .grapheme_indices(true)
                .next_back()?
                .0
        } else {
            display + self.text.get(display..)?.graphemes(true).next()?.len()
        };
        let index = self
            .pieces
            .partition_point(|piece| piece.display.end <= target);
        if let Some(piece) = self.pieces.get(index)
            && !piece.verbatim
            && piece.display.start < target
        {
            // A single source token can render more than one glyph (for example &fjlig;).
            return Some(if backwards {
                piece.source.start
            } else {
                piece.source.end
            });
        }
        Some(self.display_to_source(target, !backwards))
    }

    fn display_to_source(&self, at: usize, trailing: bool) -> usize {
        let index = self.pieces.partition_point(|piece| {
            piece.display.end < at || !trailing && piece.display.end == at
        });
        let Some(piece) = self.pieces.get(index) else {
            return self
                .pieces
                .last()
                .map_or(self.source.start, |piece| piece.source.end);
        };
        if piece.verbatim {
            piece.source.start
                + at.saturating_sub(piece.display.start)
                    .min(piece.source.len())
        } else if trailing && at >= piece.display.end {
            piece.source.end
        } else {
            piece.source.start
        }
    }

    pub(super) fn diagram_label(&self, range: Range<usize>, label: &str) -> Option<Self> {
        let raw = self.text.get(range.clone())?;
        let source =
            self.display_to_source(range.start, false)..self.display_to_source(range.end, true);
        let mut projection = Self {
            text: String::with_capacity(label.len()),
            pieces: Vec::new(),
            source: source.clone(),
            insertions: Vec::new(),
        };
        if raw == label && source.len() == label.len() {
            projection.push(label, source, true);
        } else {
            let mut characters = raw.char_indices().peekable();
            while let Some((start, character)) = characters.next() {
                let value = if character == '\\' {
                    characters.next().map_or(character, |(_, value)| value)
                } else {
                    character
                };
                let end = characters.peek().map_or(raw.len(), |(end, _)| *end);
                let source = self.display_to_source(range.start + start, false)
                    ..self.display_to_source(range.start + end, true);
                projection.push(
                    value.encode_utf8(&mut [0; 4]),
                    source.clone(),
                    source.len() == value.len_utf8(),
                );
            }
        }
        (projection.text == label).then_some(projection)
    }
}

fn source_to_display(pieces: &[SourcePiece], at: usize) -> usize {
    let index = pieces.partition_point(|piece| piece.source.end <= at);
    if let Some(piece) = pieces.get(index) {
        if piece.verbatim {
            piece.display.start
                + at.saturating_sub(piece.source.start)
                    .min(piece.display.len())
        } else {
            piece.display.start
        }
    } else {
        pieces.last().map_or(0, |piece| piece.display.end)
    }
}

fn source_distance(source: &Range<usize>, at: usize) -> usize {
    source.start.saturating_sub(at) + at.saturating_sub(source.end)
}

fn at_wrap_boundary(line: &WrappedLineLayout, byte: usize) -> bool {
    line.wrap_boundaries
        .iter()
        .any(|boundary| line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index == byte)
}

pub(super) struct Fragment {
    pub(super) block: usize,
    pub(super) projection: Rc<Projection>,
    pub(super) layout: TextLayout,
    pub(super) align: TextAlign,
    pub(super) nowrap: bool,
    pub(super) links: Vec<(Range<usize>, SharedString)>,
}

pub(super) struct DiagramLabel {
    pub(super) block: usize,
    pub(super) projection: Projection,
    pub(super) line: Rc<WrappedLine>,
    pub(super) rect: Bounds<Pixels>,
    pub(super) line_height: Pixels,
}

impl DiagramLabel {
    fn alignment(&self, row: usize) -> Pixels {
        let wrap_x = |index: usize| {
            self.line
                .wrap_boundaries
                .get(index)
                .map_or(self.line.unwrapped_layout.width, |wrap| {
                    self.line.unwrapped_layout.runs[wrap.run_ix].glyphs[wrap.glyph_ix]
                        .position
                        .x
                })
        };
        let start = if row == 0 { px(0.) } else { wrap_x(row - 1) };
        (self.rect.size.width - (wrap_x(row) - start)) / 2.
    }

    fn position(&self, at: usize) -> Point<Pixels> {
        self.position_with_wrap_side(at, false)
    }

    fn position_with_wrap_side(&self, at: usize, after_wrap: bool) -> Point<Pixels> {
        let mut p = self
            .line
            .position_for_index(at, self.line_height)
            .unwrap_or_default();
        if after_wrap && at_wrap_boundary(&self.line, at) {
            p = point(px(0.), p.y + self.line_height);
        }
        let row = (p.y / self.line_height).floor() as usize;
        self.rect.origin + p + point(self.alignment(row), px(0.))
    }

    fn hit(&self, point: Point<Pixels>) -> Option<usize> {
        if !self.rect.contains(&point) {
            return None;
        }
        self.source_index(point, true)
    }

    fn source_index(&self, point: Point<Pixels>, exact: bool) -> Option<usize> {
        let mut local = point - self.rect.origin;
        if !exact {
            if local.y < px(0.) {
                return Some(self.projection.source.start);
            }
            if local.y >= self.rect.size.height {
                return Some(self.projection.source.end);
            }
        }
        let row = (local.y / self.line_height).floor() as usize;
        local.x -= self.alignment(row);
        let byte = match self.line.index_for_position(local, self.line_height) {
            Ok(byte) => byte,
            Err(byte) if !exact => {
                return Some(self.projection.display_to_source(byte, local.x >= px(0.)));
            }
            Err(_) => return None,
        };
        let next = self
            .projection
            .text
            .get(byte..)?
            .graphemes(true)
            .next()
            .map_or(byte, |value| byte + value.len());
        let after_wrap = self.position_with_wrap_side(byte, true);
        let a = if point.y >= after_wrap.y {
            after_wrap
        } else {
            self.position(byte)
        };
        let b = self.position(next);
        let trailing = a.y == b.y && point.x >= (a.x + b.x) / 2.;
        Some(
            self.projection
                .display_to_source(if trailing { next } else { byte }, trailing),
        )
    }

    pub(super) fn paint_selection(
        &self,
        selected: &[Range<usize>],
        color: Hsla,
        window: &mut Window,
    ) {
        for range in selected {
            let start = range.start.max(self.projection.source.start);
            let end = range.end.min(self.projection.source.end);
            if start >= end {
                continue;
            }
            let a = self.position(self.projection.source_to_display(start));
            let b = self.position(self.projection.source_to_display(end));
            let mut y = a.y;
            while y <= b.y {
                let x = if y == a.y { a.x } else { self.rect.left() };
                let right = if y == b.y { b.x } else { self.rect.right() };
                window.paint_quad(fill(
                    Bounds::new(point(x, y), size((right - x).max(px(1.)), self.line_height)),
                    color,
                ));
                y += self.line_height;
            }
        }
    }
}

impl Fragment {
    fn position(&self, at: usize) -> Point<Pixels> {
        self.position_with_wrap_side(at, false)
    }

    fn position_with_wrap_side(&self, at: usize, after_wrap: bool) -> Point<Pixels> {
        let bounds = self.layout.bounds();
        let mut p = self.layout.position_for_index(at).unwrap_or(bounds.origin);
        if after_wrap
            && self.layout.line_layout_for_index(at).is_some_and(|line| {
                if line.wrap_boundaries.is_empty() {
                    return false;
                }
                let start = self
                    .projection
                    .text
                    .get(..at)
                    .and_then(|text| text.rfind('\n'))
                    .map_or(0, |newline| newline + 1);
                at_wrap_boundary(&line, at - start)
            })
        {
            p = point(bounds.left(), p.y + self.layout.line_height());
        }
        p.x += self.alignment(p.y);
        p
    }

    pub(super) fn index(&self, mut point: Point<Pixels>) -> Result<usize, usize> {
        point.x -= self.alignment(point.y);
        self.layout.index_for_position(point)
    }

    fn closest_source(&self, point: Point<Pixels>) -> usize {
        let hit = self.index(point);
        let at = *hit.as_ref().unwrap_or_else(|byte| byte);
        let text = self.projection.text.as_str();
        if !text.is_char_boundary(at) {
            return self.projection.display_to_source(at.min(text.len()), false);
        }
        let mut grapheme = unicode_segmentation::GraphemeCursor::new(at, text.len(), true);
        let start = if grapheme.is_boundary(text, 0).unwrap_or(true) {
            at
        } else {
            grapheme.prev_boundary(text, 0).ok().flatten().unwrap_or(at)
        };
        let end = grapheme
            .next_boundary(text, 0)
            .ok()
            .flatten()
            .unwrap_or(text.len());
        let after_wrap = self.position_with_wrap_side(start, true);
        let left = if point.y >= after_wrap.y {
            after_wrap
        } else {
            self.position(start)
        };
        let right = self.position(end);
        let trailing = left.y == right.y && point.x >= (left.x + right.x) / 2.;
        self.projection
            .display_to_source(if trailing { end } else { start }, trailing)
    }

    fn alignment(&self, y: Pixels) -> Pixels {
        if self.align == TextAlign::Left {
            return px(0.);
        }
        let bounds = self.layout.bounds();
        let mut row = ((y - bounds.top()).max(px(0.)) / self.layout.line_height()).floor() as usize;
        for line in self.layout.line_layouts() {
            if row > line.wrap_boundaries.len() {
                row -= line.wrap_boundaries.len() + 1;
                continue;
            }
            let wrap_x = |index: usize| {
                line.wrap_boundaries
                    .get(index)
                    .map_or(line.unwrapped_layout.width, |wrap| {
                        line.unwrapped_layout.runs[wrap.run_ix].glyphs[wrap.glyph_ix]
                            .position
                            .x
                    })
            };
            let start = if row == 0 { px(0.) } else { wrap_x(row - 1) };
            let end = wrap_x(row);
            return (bounds.size.width - (end - start))
                * if self.align == TextAlign::Right {
                    1.
                } else {
                    0.5
                };
        }
        px(0.)
    }
}

#[derive(Clone, Default)]
pub(super) struct BlockGeometry(Rc<RefCell<Geometry>>);

#[derive(Default)]
struct Geometry {
    origin: Point<Pixels>,
    fragments: Vec<Rc<Fragment>>,
    diagram_labels: Vec<Rc<DiagramLabel>>,
}

impl BlockGeometry {
    pub(super) fn horizontal_caret(
        &self,
        block: usize,
        at: usize,
        backwards: bool,
        in_cell: bool,
    ) -> Option<usize> {
        let geometry = self.0.borrow();
        let projections = geometry
            .fragments
            .iter()
            .filter(|fragment| fragment.block == block)
            .map(|fragment| fragment.projection.as_ref())
            .chain(
                geometry
                    .diagram_labels
                    .iter()
                    .filter(|label| label.block == block)
                    .map(|label| &label.projection),
            );
        let current = projections
            .clone()
            .min_by_key(|projection| source_distance(&projection.source, at))?;
        current.step_source(at, backwards).or_else(|| {
            if in_cell {
                return None;
            }
            if backwards {
                let previous = projections
                    .filter(|p| !p.text.is_empty() && p.source.end <= current.source.start)
                    .max_by_key(|p| p.source.end)?;
                Some(previous.display_to_source(previous.text.len(), true))
            } else {
                let next = projections
                    .filter(|p| !p.text.is_empty() && p.source.start >= current.source.end)
                    .min_by_key(|p| p.source.start)?;
                Some(next.display_to_source(0, false))
            }
        })
    }

    pub(super) fn wrapped_cell_caret(
        &self,
        cell: &Range<usize>,
        at: usize,
        upwards: bool,
        after_wrap: bool,
        x: Pixels,
    ) -> Option<(usize, bool)> {
        let geometry = self.0.borrow();
        let fragment = geometry
            .fragments
            .iter()
            .find(|fragment| &fragment.projection.source == cell)?;
        let current =
            fragment.position_with_wrap_side(fragment.projection.source_to_display(at), after_wrap);
        let step = if upwards {
            -px(0.5)
        } else {
            fragment.layout.line_height() + px(0.5)
        };
        let point = point(x + geometry.origin.x, current.y + step);
        let bounds = fragment.layout.bounds();
        if point.y < bounds.top() || point.y >= bounds.bottom() {
            return None;
        }
        let next = fragment.closest_source(point).clamp(cell.start, cell.end);
        let display = fragment.projection.source_to_display(next);
        let before = fragment.position(display);
        let after = fragment.position_with_wrap_side(display, true);
        let after_wrap = after.y > before.y && point.y >= after.y;
        let y = if after_wrap { after.y } else { before.y };
        if (upwards && y < current.y) || (!upwards && y > current.y) {
            Some((next, after_wrap))
        } else {
            None
        }
    }

    pub(super) fn begin(&self, origin: Point<Pixels>) {
        let mut geometry = self.0.borrow_mut();
        geometry.origin = origin;
        geometry.fragments.clear();
        geometry.diagram_labels.clear();
    }

    pub(super) fn register(&self, fragment: Rc<Fragment>) {
        self.0.borrow_mut().fragments.push(fragment);
    }

    pub(super) fn register_diagram_label(&self, fragment: Rc<DiagramLabel>) {
        self.0.borrow_mut().diagram_labels.push(fragment);
    }

    pub(super) fn diagram_hit(&self, block: usize, point: Point<Pixels>) -> Option<usize> {
        let geometry = self.0.borrow();
        let point = point + geometry.origin;
        geometry
            .diagram_labels
            .iter()
            .filter(|label| label.block == block)
            .find_map(|label| label.hit(point))
    }

    pub(super) fn diagram_drag(&self, anchor: usize, point: Point<Pixels>) -> Option<usize> {
        let geometry = self.0.borrow();
        geometry
            .diagram_labels
            .iter()
            .find(|label| {
                (label.projection.source.start..=label.projection.source.end).contains(&anchor)
            })
            .and_then(|label| label.source_index(point + geometry.origin, false))
    }

    pub(super) fn has_diagram_labels(&self, block: usize) -> bool {
        self.0
            .borrow()
            .diagram_labels
            .iter()
            .any(|label| label.block == block)
    }

    pub(super) fn link_at(&self, block: usize, point: Point<Pixels>) -> Option<SharedString> {
        let g = self.0.borrow();
        let point = point + g.origin;
        g.fragments
            .iter()
            .filter(|fragment| {
                fragment.block == block
                    && !fragment.links.is_empty()
                    && fragment.layout.bounds().contains(&point)
            })
            .find_map(|fragment| {
                let at = fragment.index(point).ok()?;
                fragment
                    .links
                    .iter()
                    .find(|(range, _)| range.contains(&at))
                    .map(|(_, target)| target.clone())
            })
    }

    pub(super) fn hit(&self, block: usize, p: Point<Pixels>) -> Option<usize> {
        if let Some(at) = self.diagram_hit(block, p) {
            return Some(at);
        }
        let g = self.0.borrow();
        let p = p + g.origin;
        g.fragments
            .iter()
            .filter(|fragment| fragment.block == block)
            .min_by(|a, b| {
                let distance = |fragment: &Fragment| {
                    let bounds = fragment.layout.bounds();
                    let dx = (bounds.left() - p.x).max(p.x - bounds.right()).max(px(0.));
                    let dy = (bounds.top() - p.y).max(p.y - bounds.bottom()).max(px(0.));
                    (dy, dx)
                };
                let (ay, ax) = distance(a);
                let (by, bx) = distance(b);
                f32::from(ay)
                    .total_cmp(&f32::from(by))
                    .then_with(|| f32::from(ax).total_cmp(&f32::from(bx)))
            })
            .map(|fragment| fragment.closest_source(p))
    }

    pub(super) fn caret(
        &self,
        block: usize,
        at: usize,
        after_wrap: bool,
    ) -> Option<(Point<Pixels>, Pixels)> {
        let g = self.0.borrow();
        if let Some(label) = g.diagram_labels.iter().find(|label| {
            label.block == block
                && (label.projection.source.start..=label.projection.source.end).contains(&at)
        }) {
            return Some((
                label.position_with_wrap_side(label.projection.source_to_display(at), after_wrap)
                    - g.origin,
                label.line_height,
            ));
        }
        let fragment = g
            .fragments
            .iter()
            .filter(|fragment| fragment.block == block)
            .min_by_key(|fragment| source_distance(&fragment.projection.source, at))?;
        let byte = fragment.projection.source_to_display(at);
        Some((
            fragment.position_with_wrap_side(byte, after_wrap) - g.origin,
            fragment.layout.line_height(),
        ))
    }

    pub(super) fn has_text(&self, block: usize) -> bool {
        self.0
            .borrow()
            .fragments
            .iter()
            .any(|fragment| fragment.block == block)
    }

    pub(super) fn required_width(&self) -> Pixels {
        let g = self.0.borrow();
        g.fragments
            .iter()
            .filter(|fragment| fragment.nowrap)
            .map(|fragment| {
                let left = fragment.layout.bounds().left() - g.origin.x;
                let text = fragment
                    .layout
                    .line_layouts()
                    .iter()
                    .fold(px(0.), |width, line| width.max(line.unwrapped_layout.width));
                left + text + px(16.)
            })
            .fold(px(0.), Pixels::max)
    }
}
