//! Occurrence selections and simultaneous replacements. The newest selection is primary.
use super::*;

impl InputState {
    pub(super) fn multiple_clipboard_text(&self) -> String {
        let linewise = self.selected_ranges().all(|r| r.is_empty());
        let mut ranges: Vec<_> = self
            .selected_ranges()
            .map(|r| {
                if linewise {
                    self.clipboard_range_at(r)
                } else {
                    r
                }
            })
            .collect();
        ranges.sort_by_key(|r| (r.start, r.end));
        ranges.dedup();
        ranges
            .iter()
            .map(|r| &self.content[r.clone()])
            .collect::<Vec<_>>()
            .join(if linewise { "" } else { "\n" })
    }

    pub(super) fn input_ranges(&self, explicit: Option<Range<usize>>) -> Vec<Range<usize>> {
        let primary = self.marked.as_ref().unwrap_or(&self.selection);
        let range = explicit.as_ref().unwrap_or(primary);
        let mut ranges: Vec<_> = self
            .extra
            .iter()
            .enumerate()
            .map(|(i, selection)| {
                let base = self.extra_marked.get(i).unwrap_or(&selection.range);
                if explicit.is_some() {
                    let start = self.boundary(
                        base.start
                            .saturating_add_signed(range.start as isize - primary.start as isize),
                    );
                    let end = self.boundary(
                        base.end
                            .saturating_add_signed(range.end as isize - primary.end as isize),
                    );
                    start..end.max(start)
                } else {
                    base.clone()
                }
            })
            .collect();
        ranges.push(range.clone());
        ranges
    }

    pub(super) fn clear_extra_selections(&mut self) {
        self.column_anchor = None;
        self.caret_after_wrap = false;
        self.extra.clear();
        self.extra_marked.clear();
        self.occurrence_word = false;
    }

    pub fn cancel_extra_selections(&mut self, cx: &mut Context<Self>) -> bool {
        self.stop_pointer_drag();
        cx.notify();
        self.finish_composition();
        let Some(first) = self.extra.first().cloned() else {
            return false;
        };
        self.selection = first.range;
        self.reversed = first.reversed;
        self.preferred_x = first.preferred_x;
        self.clear_extra_selections();
        self.caret_after_wrap = first.after_wrap;
        self.last_edit = None;
        self.moved(cx);
        true
    }

    pub(super) fn select_next_occurrence(&mut self, cx: &mut Context<Self>) {
        self.finish_composition();
        self.last_edit = None;
        if self.selection.is_empty() {
            if !self.extra.is_empty() {
                let mut selections: Vec<_> = self
                    .selected_ranges()
                    .map(|range| {
                        if range.is_empty() {
                            self.word_at(range.start).unwrap_or(range)
                        } else {
                            range
                        }
                    })
                    .collect();
                self.selection = selections.pop().unwrap();
                self.reversed = false;
                self.extra = selections
                    .into_iter()
                    .map(|range| Selection {
                        range,
                        reversed: false,
                        preferred_x: None,
                        after_wrap: false,
                    })
                    .collect();
                self.deduplicate_selections();
                self.occurrence_word = true;
                self.moved(cx);
                return;
            }
            if let Some(range) = self.word_at_cursor() {
                self.set_selected_range(range, cx);
                self.occurrence_word = true;
            }
            return;
        }
        let query = &self.content[self.selection.clone()];
        let after = self.selection.end;
        let found = self.content[after..]
            .match_indices(query)
            .map(|(at, _)| after + at)
            .chain(self.content[..after].match_indices(query).map(|(at, _)| at))
            .map(|at| at..at + query.len())
            .find(|range| {
                !self
                    .selected_ranges()
                    .any(|s| range.start < s.end && s.start < range.end)
                    && (!self.occurrence_word || self.word_at(range.start) == Some(range.clone()))
                    && [range.start, range.end].into_iter().all(|at| {
                        unicode_segmentation::GraphemeCursor::new(at, self.content.len(), true)
                            .is_boundary(&self.content, 0)
                            .expect("complete text")
                    })
            });
        if let Some(range) = found {
            self.extra.push(Selection {
                range: self.selection.clone(),
                reversed: self.reversed,
                preferred_x: self.preferred_x,
                after_wrap: self.caret_after_wrap,
            });
            self.selection = range;
            self.reversed = false;
            self.caret_after_wrap = false;
            self.preferred_x = None;
            self.moved(cx);
        }
    }

    /// Rebuild the text once; record edits right to left for stable undo offsets.
    pub(super) fn apply_multiple(
        &mut self,
        ranges: Vec<Range<usize>>,
        texts: &[String],
    ) -> Vec<Replacement> {
        let mut edits: Vec<_> = ranges.into_iter().enumerate().collect();
        edits.sort_by_key(|(_, r)| (r.start, r.end));
        // Navigation may make carets coincide or deletion ranges overlap.
        let mut unique: Vec<(Vec<usize>, Range<usize>, String)> = Vec::new();
        for (index, range) in edits {
            if let Some((indices, previous, _)) = unique.last_mut()
                && (range.start < previous.end || *previous == range)
            {
                previous.end = previous.end.max(range.end);
                indices.push(index);
            } else {
                unique.push((vec![index], range, texts[index].clone()));
            }
        }
        let mut selections = vec![0..0; texts.len()];
        let mut shift: isize = 0;
        for (indices, range, text) in &unique {
            let end = range.start.checked_add_signed(shift).unwrap() + text.len();
            for &i in indices {
                selections[i] = end..end;
            }
            shift += text.len() as isize - range.len() as isize;
        }
        let changes: Vec<_> = unique
            .iter()
            .rev()
            .map(|(_, range, text)| Replacement {
                start: range.start,
                before: self.content[range.clone()].to_string(),
                after: text.clone(),
            })
            .collect();
        let mut value =
            String::with_capacity(self.content.len().checked_add_signed(shift).unwrap());
        let mut copied = 0;
        for change in changes.iter().rev() {
            value.push_str(&self.content[copied..change.start]);
            value.push_str(&change.after);
            copied = change.start + change.before.len();
        }
        value.push_str(&self.content[copied..]);
        self.content = value.into();
        self.selection = selections.pop().unwrap();
        self.reversed = false;
        self.caret_after_wrap = false;
        self.extra = selections
            .into_iter()
            .map(|range| Selection {
                range,
                reversed: false,
                preferred_x: None,
                after_wrap: false,
            })
            .collect();
        self.deduplicate_selections();
        changes
    }

    pub(super) fn deduplicate_selections(&mut self) {
        if self.extra.is_empty() {
            return;
        }
        let mut selections: Vec<_> = std::mem::take(&mut self.extra)
            .into_iter()
            .enumerate()
            .collect();
        selections.push((
            selections.len(),
            Selection {
                range: self.selection.clone(),
                reversed: self.reversed,
                preferred_x: self.preferred_x,
                after_wrap: self.caret_after_wrap,
            },
        ));
        selections.sort_by_key(|(_, s)| (s.range.start, s.range.end));
        let mut merged: Vec<(usize, Selection)> = Vec::new();
        for (index, selection) in selections {
            if let Some((previous_index, previous)) = merged.last_mut()
                && (selection.range.start < previous.range.end || selection.range == previous.range)
            {
                previous.range.end = previous.range.end.max(selection.range.end);
                if index > *previous_index {
                    *previous_index = index;
                    previous.reversed = selection.reversed;
                    previous.after_wrap = selection.after_wrap;
                }
                previous.preferred_x = None;
            } else {
                merged.push((index, selection));
            }
        }
        merged.sort_by_key(|(i, _)| *i);
        let (_, primary) = merged.pop().unwrap();
        self.selection = primary.range;
        self.reversed = primary.reversed;
        self.caret_after_wrap = primary.after_wrap;
        self.preferred_x = primary.preferred_x;
        self.extra = merged.into_iter().map(|(_, selection)| selection).collect();
    }

    pub(super) fn commit_multiple(
        &mut self,
        ranges: Vec<Range<usize>>,
        texts: Vec<String>,
        merge: bool,
        cx: &mut Context<Self>,
    ) {
        let selection = self.selection.clone();
        let reversed = self.reversed;
        let after_wrap = self.caret_after_wrap;
        let extra = self.extra.clone();
        let changes = self.apply_multiple(ranges, &texts);
        if changes.iter().all(|c| c.before == c.after) {
            self.last_edit = None;
            self.marked = None;
            self.extra_marked.clear();
            self.occurrence_word = false;
            self.moved(cx);
            return;
        }
        let coalesce = merge
            && self
                .last_edit
                .is_some_and(|t| t.elapsed() < history::TYPING_MERGE_INTERVAL)
            && changes
                .iter()
                .all(|c| c.before.is_empty() && !c.after.contains(['\r', '\n']))
            && self.undo.last().is_some_and(|edit| {
                let mut previous: Vec<_> = edit
                    .others
                    .iter()
                    .map(|c| (c.start, c.before.as_str(), c.after.len()))
                    .collect();
                previous.insert(0, (edit.start, &edit.before, edit.after.len()));
                let mut shift = 0;
                previous.len() == changes.len()
                    && previous.iter().rev().zip(changes.iter().rev()).all(
                        |(&(start, before, len), next)| {
                            let matches = before.is_empty() && start + shift + len == next.start;
                            shift += len;
                            matches
                        },
                    )
            });
        if coalesce {
            let edit = self.undo.last_mut().unwrap();
            edit.after.push_str(&changes[0].after);
            for (previous, next) in edit.others.iter_mut().zip(&changes[1..]) {
                previous.after.push_str(&next.after);
            }
        } else {
            let mut changes = changes.into_iter();
            if let Some(first) = changes.next() {
                self.undo.push(Edit {
                    start: first.start,
                    before: first.before,
                    after: first.after,
                    selection,
                    reversed,
                    after_wrap,
                    extra,
                    others: changes.collect(),
                });
            }
        }
        self.redo.clear();
        self.last_edit = merge.then(std::time::Instant::now);
        self.preferred_x = None;
        self.marked = None;
        self.extra_marked.clear();
        self.occurrence_word = false;
        self.changed(cx);
    }
}
