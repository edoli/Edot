use crate::model::Notebook;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropZone {
    Before,
    Inside,
    After,
    Root,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropTarget {
    pub note: Option<u64>,
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
                    DropZone::Inside => (id, None),
                    DropZone::After => {
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
            parent,
            before,
            zone,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
