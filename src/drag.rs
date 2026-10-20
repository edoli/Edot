use crate::model::NoteId;
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
    pub note: Option<NoteId>,
    /// Visible row where the insertion marker belongs (possibly a descendant).
    pub anchor: Option<NoteId>,
    pub parent: NoteId,
    pub before: Option<NoteId>,
    pub zone: DropZone,
}

impl DropTarget {
    pub fn resolve_many(
        book: &Notebook,
        sources: &[NoteId],
        note: Option<NoteId>,
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
                    DropZone::Inside | DropZone::InsideLast if target.is_folder() => (id, None),
                    DropZone::InsideFirst if target.is_folder() => (
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
                    _ => return None,
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
        sources: &[NoteId],
        row: NoteId,
        previous_visible: Option<NoteId>,
        next_visible: Option<NoteId>,
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
        if book.is_folder(row) {
            Self::resolve_many(book, sources, Some(row), DropZone::Inside)
        } else {
            Self::resolve_boundary(book, sources, Some(row), next_visible, false)
        }
    }

    fn resolve_boundary(
        book: &Notebook,
        sources: &[NoteId],
        previous: Option<NoteId>,
        next: Option<NoteId>,
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
