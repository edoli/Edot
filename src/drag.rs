use crate::model::Notebook;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropZone {
    Before,
    Inside,
    InsideFirst,
    InsideLast,
    After,
    AfterSubtree,
    Root,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropTarget {
    pub note: Option<u64>,
    /// Visible row where the insertion marker belongs (possibly a descendant).
    pub anchor: Option<u64>,
    pub parent: u64,
    pub before: Option<u64>,
    pub zone: DropZone,
}

impl DropTarget {
    #[cfg(test)]
    pub fn resolve(
        book: &Notebook,
        source: u64,
        note: Option<u64>,
        zone: DropZone,
    ) -> Option<Self> {
        Self::resolve_many(book, &[source], note, zone)
    }

    pub fn resolve_many(
        book: &Notebook,
        sources: &[u64],
        note: Option<u64>,
        zone: DropZone,
    ) -> Option<Self> {
        if sources.is_empty() || sources.iter().any(|&id| book.get(id).is_none()) {
            return None;
        }
        let roots = sources;
        let (parent, before) = match (note, zone) {
            (None, DropZone::Root) => (0, None),
            (Some(id), zone)
                if !roots.contains(&id)
                    && !roots.iter().any(|&root| book.is_descendant(root, id)) =>
            {
                let target = book.get(id)?;
                match zone {
                    DropZone::Before => (target.parent, Some(id)),
                    DropZone::Inside | DropZone::InsideLast => (id, None),
                    DropZone::InsideFirst => (
                        id,
                        book.children(id).into_iter().find(|id| !roots.contains(id)),
                    ),
                    DropZone::After | DropZone::AfterSubtree => {
                        let siblings = book.children(target.parent);
                        let position = siblings.iter().position(|&s| s == id)?;
                        let next = siblings[position + 1..]
                            .iter()
                            .copied()
                            .find(|s| !roots.contains(s));
                        (target.parent, next)
                    }
                    DropZone::Root => return None,
                }
            }
            _ => return None,
        };
        if roots
            .iter()
            .any(|&source| parent == source || book.is_descendant(source, parent))
        {
            return None;
        }
        Some(Self {
            note,
            anchor: note,
            parent,
            before,
            zone,
        })
    }

    /// Both row edges resolve the shared gap to the same target. Only a gap
    /// that closes a folder distinguishes its inside and outside positions.
    pub fn resolve_row(
        book: &Notebook,
        sources: &[u64],
        row: u64,
        previous_visible: Option<u64>,
        next_visible: Option<u64>,
        y: i32,
        height: i32,
    ) -> Option<Self> {
        let height = height.max(1);
        book.get(row)?;
        if y < height / 4 {
            return Self::resolve_boundary(book, sources, previous_visible, Some(row), true);
        }
        if y >= height * 3 / 4 {
            return Self::resolve_boundary(book, sources, Some(row), next_visible, false);
        }
        Self::resolve_many(book, sources, Some(row), DropZone::Inside)
    }

    fn resolve_boundary(
        book: &Notebook,
        sources: &[u64],
        previous: Option<u64>,
        next: Option<u64>,
        outside: bool,
    ) -> Option<Self> {
        let Some(row) = previous else {
            return Self::resolve_many(book, sources, next, DropZone::Before);
        };
        let note = book.get(row)?;
        if next.is_some_and(|next| book.get(next).is_some_and(|next| next.parent == row)) {
            return Self::resolve_many(book, sources, Some(row), DropZone::InsideFirst);
        }
        let mut closing = None;
        let mut parent = note.parent;
        while parent != 0 && next.is_none_or(|next| !book.is_descendant(parent, next)) {
            closing = Some(parent);
            parent = book.get(parent)?.parent;
        }
        if let Some(folder) = closing {
            let zone = if outside {
                DropZone::AfterSubtree
            } else {
                DropZone::InsideLast
            };
            let mut target = Self::resolve_many(book, sources, Some(folder), zone)?;
            target.anchor = Some(row);
            return Some(target);
        }
        // Prefer the next row so selected items above the gap remain movable.
        // When the next row moves too, use the preceding unselected sibling.
        if let Some(next) = next.filter(|next| !sources.contains(next)) {
            Self::resolve_many(book, sources, Some(next), DropZone::Before)
        } else {
            Self::resolve_many(book, sources, Some(row), DropZone::After)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_row_edges_share_one_gap_except_at_a_folder_end() {
        let mut book = Notebook {
            next_id: 1,
            ..Default::default()
        };
        let folder = book.add(0, "folder".into());
        let first = book.add(folder, "first".into());
        let last = book.add(folder, "last".into());
        let next = book.add(0, "next".into());
        let source = book.add(0, "source".into());
        for height in [28, 43, 58] {
            let at = |row, previous, next, y| {
                DropTarget::resolve_row(&book, &[source], row, previous, next, y, height).unwrap()
            };
            let top = at(folder, None, Some(first), height - 1);
            let between = at(first, Some(folder), Some(last), height - 1);
            let bottom = at(last, Some(first), Some(next), height - 1);
            let after = at(next, Some(last), Some(source), 0);
            assert_eq!(
                (top.zone, top.parent, top.before, top.anchor),
                (DropZone::InsideFirst, folder, Some(first), Some(folder))
            );
            assert_eq!(
                (bottom.zone, bottom.parent, bottom.before, bottom.anchor),
                (DropZone::InsideLast, folder, None, Some(last))
            );
            assert_eq!(
                (
                    after.zone,
                    after.parent,
                    after.before,
                    after.note,
                    after.anchor
                ),
                (
                    DropZone::AfterSubtree,
                    0,
                    Some(next),
                    Some(folder),
                    Some(last)
                )
            );
            // Scan the whole gap from either adjacent row: one target, or two
            // distinct parents at the boundary where the folder ends.
            for y in height * 3 / 4..height {
                assert_eq!(at(folder, None, Some(first), y), top);
                assert_eq!(at(first, Some(folder), Some(last), y), between);
                assert_eq!(at(last, Some(first), Some(next), y), bottom);
            }
            for y in 0..height / 4 {
                assert_eq!(at(first, Some(folder), Some(last), y), top);
                assert_eq!(at(last, Some(first), Some(next), y), between);
                assert_eq!(at(next, Some(last), Some(source), y), after);
            }
            // No extra insertion slot in the middle of the final child.
            assert_eq!(
                at(last, Some(first), Some(next), height * 5 / 8).zone,
                DropZone::Inside
            );
            for (target, children, roots) in [
                (top, vec![source, first, last], vec![folder, next]),
                (bottom, vec![first, last, source], vec![folder, next]),
                (after, vec![first, last], vec![folder, source, next]),
            ] {
                let mut moved = book.clone();
                assert!(moved.move_notes(&[source], target.parent, target.before));
                assert_eq!(moved.children(folder), children);
                assert_eq!(moved.children(0), roots);
            }
            let closed = at(folder, None, Some(next), height - 1);
            assert_eq!(closed, at(next, Some(folder), Some(source), 0));
            assert_eq!(
                (closed.zone, closed.parent, closed.before),
                (DropZone::Before, 0, Some(next))
            );
            // Selected neighbours do not split or invalidate the shared gap.
            for moving in [first, last] {
                let above = DropTarget::resolve_row(
                    &book,
                    &[moving],
                    first,
                    Some(folder),
                    Some(last),
                    height - 1,
                    height,
                )
                .unwrap();
                let below = DropTarget::resolve_row(
                    &book,
                    &[moving],
                    last,
                    Some(first),
                    Some(next),
                    0,
                    height,
                )
                .unwrap();
                assert_eq!(above, below);
                let mut moved = book.clone();
                assert!(moved.move_notes(&[moving], above.parent, above.before));
                assert_eq!(moved.children(folder), vec![first, last]);
            }
            let same = DropTarget::resolve_row(
                &book,
                &[first],
                folder,
                None,
                Some(first),
                height - 1,
                height,
            )
            .unwrap();
            assert_eq!(same.before, Some(last));
            assert!(
                DropTarget::resolve_row(
                    &book,
                    &[folder],
                    folder,
                    None,
                    Some(first),
                    height - 1,
                    height
                )
                .is_none()
            );
            assert!(
                DropTarget::resolve_row(
                    &book,
                    &[folder],
                    last,
                    Some(first),
                    Some(next),
                    height - 1,
                    height
                )
                .is_none()
            );
        }
    }

    #[test]
    fn nested_subtree_end_and_batch_moves_preserve_every_child() {
        let mut book = Notebook {
            next_id: 1,
            ..Default::default()
        };
        let outer = book.add(0, "outer".into());
        let inner = book.add(outer, "inner".into());
        let leaf = book.add(inner, "leaf".into());
        let other = book.add(0, "other".into());
        let a = book.add(0, "a".into());
        let b = book.add(0, "b".into());
        let end =
            DropTarget::resolve_row(&book, &[a, b], other, Some(leaf), Some(a), 0, 28).unwrap();
        assert_eq!(
            (end.note, end.anchor, end.parent, end.before),
            (Some(outer), Some(leaf), 0, Some(other))
        );
        assert!(book.move_notes(&[a, b], end.parent, end.before));
        assert_eq!(book.children(0), vec![outer, a, b, other]);
        assert_eq!(book.children(outer), vec![inner]);
        assert_eq!(book.children(inner), vec![leaf]);
        let last = DropTarget::resolve_row(&book, &[a], leaf, Some(inner), None, 27, 28).unwrap();
        assert_eq!((last.zone, last.parent), (DropZone::InsideLast, outer));
        let sibling = book.add(outer, "sibling".into());
        let inner_end =
            DropTarget::resolve_row(&book, &[a], sibling, Some(leaf), Some(other), 0, 28).unwrap();
        assert_eq!(
            (inner_end.note, inner_end.parent, inner_end.before),
            (Some(inner), outer, Some(sibling))
        );
        let between =
            DropTarget::resolve_row(&book, &[a], inner, Some(outer), Some(sibling), 27, 28)
                .unwrap();
        assert_eq!(
            (between.zone, between.parent, between.before),
            (DropZone::Before, outer, Some(sibling))
        );
    }

    #[test]
    fn feedback_rejects_cycles_and_matches_the_saved_move() {
        let mut book = Notebook {
            next_id: 1,
            ..Default::default()
        };
        let a = book.add(0, "A".into());
        let b = book.add(a, "B".into());
        let c = book.add(0, "C".into());
        let d = book.add(0, "D".into());
        assert!(DropTarget::resolve(&book, a, Some(b), DropZone::Inside).is_none());
        assert!(DropTarget::resolve(&book, a, Some(b), DropZone::Before).is_none());
        assert!(DropTarget::resolve(&book, a, Some(a), DropZone::After).is_none());
        let drop = DropTarget::resolve(&book, a, Some(c), DropZone::After).unwrap();
        assert_eq!((drop.parent, drop.before), (0, Some(d)));
        assert!(book.move_note(a, drop.parent, drop.before));
        assert_eq!(book.children(0), vec![c, a, d]);
        assert_eq!(book.get(b).unwrap().parent, a);
    }

    #[test]
    fn dropping_after_the_previous_sibling_is_a_valid_no_op() {
        let mut book = Notebook {
            next_id: 1,
            ..Default::default()
        };
        let a = book.add(0, "A".into());
        let b = book.add(0, "B".into());
        let drop = DropTarget::resolve(&book, b, Some(a), DropZone::After).unwrap();
        assert_eq!(drop.before, None);
        assert!(book.move_note(b, drop.parent, drop.before));
        assert_eq!(book.children(0), vec![a, b]);
        assert!(DropTarget::resolve(&book, b, None, DropZone::Root).is_some());
    }

    #[test]
    fn batch_feedback_skips_selected_siblings_and_rejects_every_selected_subtree() {
        let mut book = Notebook {
            next_id: 1,
            ..Default::default()
        };
        let ids: Vec<_> = (0..5).map(|i| book.add(0, format!("{i}"))).collect();
        let child = book.add(ids[3], "child".into());
        let selected = [ids[1], ids[3]];
        assert!(
            DropTarget::resolve_many(&book, &selected, Some(child), DropZone::Inside).is_none()
        );
        assert!(
            DropTarget::resolve_many(&book, &selected, Some(ids[1]), DropZone::Before).is_none()
        );
        let target =
            DropTarget::resolve_many(&book, &selected, Some(ids[2]), DropZone::After).unwrap();
        assert_eq!(target.before, Some(ids[4]));
        assert!(book.move_notes(&selected, target.parent, target.before));
        assert_eq!(
            book.children(0),
            vec![ids[0], ids[2], ids[1], ids[3], ids[4]]
        );
        assert_eq!(book.get(child).unwrap().parent, ids[3]);
    }
}
