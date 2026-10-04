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
    pub fn resolve(
        book: &Notebook,
        source: u64,
        note: Option<u64>,
        zone: DropZone,
    ) -> Option<Self> {
        book.get(source)?;
        let (parent, before) = match (note, zone) {
            (None, DropZone::Root) => (0, None),
            (Some(id), zone) if id != source => {
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
                            .find(|&s| s != source);
                        (target.parent, next)
                    }
                    DropZone::Root => return None,
                }
            }
            _ => return None,
        };
        if parent == source || book.is_descendant(source, parent) {
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
}
