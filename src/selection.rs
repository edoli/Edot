use std::{collections::HashSet, rc::Rc};

/// TreeView keeps one keyboard caret; selected note IDs survive HWND rebuilds.
#[derive(Default)]
pub struct Selection {
    pub ids: Rc<HashSet<u64>>,
    anchor: Option<u64>,
}

impl Selection {
    pub fn single(&mut self, id: u64) {
        if self.ids.len() != 1 || !self.ids.contains(&id) {
            self.ids = Rc::new(HashSet::from([id]));
        }
        self.anchor = Some(id);
    }

    pub fn click(&mut self, id: u64, visible: &[u64], ctrl: bool, shift: bool) {
        if shift {
            let anchor = self
                .anchor
                .and_then(|a| visible.iter().position(|&v| v == a));
            if let (Some(a), Some(b)) = (anchor, visible.iter().position(|&v| v == id)) {
                let selected = Rc::make_mut(&mut self.ids);
                if !ctrl {
                    selected.clear();
                }
                selected.extend(&visible[a.min(b)..=a.max(b)]);
                return;
            }
        }
        if ctrl && !shift {
            let selected = Rc::make_mut(&mut self.ids);
            if !selected.insert(id) {
                selected.remove(&id);
            }
            self.anchor = Some(id);
        } else {
            self.single(id);
        }
    }

    pub fn all(&mut self, visible: &[u64]) {
        self.ids = Rc::new(visible.iter().copied().collect());
        self.anchor = visible.first().copied();
    }

    pub fn retain(&mut self, valid: impl Fn(u64) -> bool) {
        if self.ids.iter().any(|&id| !valid(id)) {
            Rc::make_mut(&mut self.ids).retain(|&id| valid(id));
        }
        if self.anchor.is_some_and(|id| !valid(id)) {
            self.anchor = None;
        }
    }
}
