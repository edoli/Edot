use crate::storage::replace_file;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: u64,
    pub parent: u64,
    pub order: u32,
    pub title: String,
}

#[derive(Clone, Default)]
pub struct Notebook {
    pub notes: Vec<Note>,
    pub next_id: u64,
}

impl Notebook {
    pub fn load(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir.join("notes"))?;
        let path = dir.join("tree.tsv");
        let text = if path.exists() {
            fs::read_to_string(path)?
        } else {
            String::from("EDOT1\n")
        };
        let mut lines = text.lines();
        if lines.next() != Some("EDOT1") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported tree index",
            ));
        }
        let mut notes = Vec::new();
        for line in lines {
            let mut parts = line.split('\t');
            let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid tree index");
            let id = parts
                .next()
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?;
            let parent = parts
                .next()
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?;
            let order = parts
                .next()
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?;
            let title = String::from_utf8(decode_hex(parts.next().ok_or_else(invalid)?)?)
                .map_err(|_| invalid())?;
            if parts.next().is_some() || id == 0 || notes.iter().any(|n: &Note| n.id == id) {
                return Err(invalid());
            }
            notes.push(Note {
                id,
                parent,
                order,
                title,
            });
        }
        for note in &notes {
            if note.parent != 0 && !notes.iter().any(|n| n.id == note.parent) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Missing parent note",
                ));
            }
        }
        // If a previous write stopped after creating a text file, keep that file visible.
        for entry in fs::read_dir(dir.join("notes"))? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let filename = entry.file_name();
            let name = filename.to_string_lossy();
            let Some(id) = name
                .strip_suffix(".txt")
                .and_then(|s| s.parse::<u64>().ok())
            else {
                continue;
            };
            if id == 0 || notes.iter().any(|n| n.id == id) {
                continue;
            }
            let order = notes.iter().filter(|n| n.parent == 0).count() as u32;
            notes.push(Note {
                id,
                parent: 0,
                order,
                title: format!("복구된 메모 {id}"),
            });
        }
        let next_id = notes
            .iter()
            .map(|n| n.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let book = Self { notes, next_id };
        if book.notes.iter().any(|n| book.is_descendant(n.id, n.id)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Tree contains a cycle",
            ));
        }
        Ok(book)
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        let mut text = String::from("EDOT1\n");
        for n in &self.notes {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                n.id,
                n.parent,
                n.order,
                encode_hex(n.title.as_bytes())
            ));
        }
        replace_file(&dir.join("tree.tsv"), text.as_bytes())
    }

    pub fn file(&self, dir: &Path, id: u64) -> PathBuf {
        dir.join("notes").join(format!("{id}.txt"))
    }

    pub fn add(&mut self, parent: u64, title: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let order = self.notes.iter().filter(|n| n.parent == parent).count() as u32;
        self.notes.push(Note {
            id,
            parent,
            order,
            title,
        });
        id
    }

    pub fn get(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    pub fn children(&self, parent: u64) -> Vec<u64> {
        let mut children: Vec<_> = self.notes.iter().filter(|n| n.parent == parent).collect();
        children.sort_by_key(|n| (n.order, n.id));
        children.into_iter().map(|n| n.id).collect()
    }

    pub fn is_descendant(&self, ancestor: u64, candidate: u64) -> bool {
        let mut current = self.get(candidate).map(|n| n.parent).unwrap_or(0);
        for _ in 0..self.notes.len() {
            if current == 0 {
                return false;
            }
            if current == ancestor {
                return true;
            }
            current = self.get(current).map(|n| n.parent).unwrap_or(0);
        }
        true
    }

    pub fn move_note(&mut self, id: u64, parent: u64, before: Option<u64>) -> bool {
        if id == parent
            || (parent != 0 && self.get(parent).is_none())
            || self.is_descendant(id, parent)
        {
            return false;
        }
        if let Some(before_id) = before
            && (before_id == id || self.get(before_id).is_none_or(|n| n.parent != parent))
        {
            return false;
        }
        let old_parent = match self.get(id) {
            Some(n) => n.parent,
            None => return false,
        };
        let mut old_siblings = self.children(old_parent);
        old_siblings.retain(|&x| x != id);
        let mut new_siblings = if old_parent == parent {
            old_siblings.clone()
        } else {
            self.children(parent)
        };
        let position = before
            .and_then(|b| new_siblings.iter().position(|&x| x == b))
            .unwrap_or(new_siblings.len());
        new_siblings.insert(position, id);
        if let Some(note) = self.get_mut(id) {
            note.parent = parent;
        }
        self.set_orders(old_parent, &old_siblings);
        self.set_orders(parent, &new_siblings);
        true
    }

    /// Keep tree order, dropping selected descendants whose ancestor also moves.
    pub fn selected_roots(&self, ids: &[u64]) -> Vec<u64> {
        if ids.is_empty() {
            return Vec::new();
        }
        let selected: std::collections::HashSet<_> = ids.iter().copied().collect();
        let mut children: std::collections::HashMap<u64, Vec<_>> = std::collections::HashMap::new();
        for note in &self.notes {
            children.entry(note.parent).or_default().push(note);
        }
        for siblings in children.values_mut() {
            siblings.sort_by_key(|n| (n.order, n.id));
        }
        let mut ordered = Vec::new();
        let mut pending: Vec<_> = children
            .get(&0)
            .into_iter()
            .flatten()
            .rev()
            .map(|n| n.id)
            .collect();
        while let Some(id) = pending.pop() {
            if selected.contains(&id) {
                ordered.push(id);
            } else {
                pending.extend(children.get(&id).into_iter().flatten().rev().map(|n| n.id));
            }
        }
        ordered
    }

    pub fn move_notes(&mut self, ids: &[u64], parent: u64, before: Option<u64>) -> bool {
        if ids.is_empty() || ids.iter().any(|&id| self.get(id).is_none()) {
            return false;
        }
        let roots = self.selected_roots(ids);
        if (parent != 0 && self.get(parent).is_none())
            || roots
                .iter()
                .any(|&id| id == parent || self.is_descendant(id, parent))
            || before.is_some_and(|id| {
                roots.contains(&id) || self.get(id).is_none_or(|n| n.parent != parent)
            })
        {
            return false;
        }
        // Plan every affected sibling list before touching any note.
        let root_set: std::collections::HashSet<_> = roots.iter().copied().collect();
        let mut affected: std::collections::HashMap<u64, Vec<_>> = roots
            .iter()
            .map(|&id| (self.get(id).unwrap().parent, Vec::new()))
            .collect();
        affected.entry(parent).or_default();
        for note in &self.notes {
            if !root_set.contains(&note.id)
                && let Some(siblings) = affected.get_mut(&note.parent)
            {
                siblings.push((note.order, note.id));
            }
        }
        for siblings in affected.values_mut() {
            siblings.sort_unstable();
        }
        let mut destination: Vec<_> = affected
            .remove(&parent)
            .unwrap()
            .into_iter()
            .map(|(_, id)| id)
            .collect();
        let position = before
            .and_then(|id| destination.iter().position(|&n| n == id))
            .unwrap_or(destination.len());
        destination.splice(position..position, roots);
        let mut updates: std::collections::HashMap<_, _> = destination
            .into_iter()
            .enumerate()
            .map(|(order, id)| (id, (parent, order as u32)))
            .collect();
        for (old_parent, siblings) in affected {
            updates.extend(
                siblings
                    .into_iter()
                    .enumerate()
                    .map(|(order, (_, id))| (id, (old_parent, order as u32))),
            );
        }
        for note in &mut self.notes {
            if let Some(&(parent, order)) = updates.get(&note.id) {
                note.parent = parent;
                note.order = order;
            }
        }
        true
    }

    pub fn group_notes(&mut self, ids: &[u64]) -> Option<u64> {
        if ids.is_empty() || ids.iter().any(|&id| self.get(id).is_none()) {
            return None;
        }
        let roots = self.selected_roots(ids);
        let first = *roots.first()?;
        let mut parent = self.get(first)?.parent;
        while parent != 0 && !roots.iter().all(|&id| self.is_descendant(parent, id)) {
            parent = self.get(parent)?.parent;
        }
        let before = self.children(parent).into_iter().find(|&id| {
            roots
                .iter()
                .any(|&root| root == id || self.is_descendant(id, root))
        });
        let folder = self.add(parent, "새 폴더".into());
        self.move_note(folder, parent, before);
        self.move_notes(&roots, folder, None);
        Some(folder)
    }

    pub fn selected_subtrees(&self, ids: &[u64]) -> Vec<u64> {
        self.selected_roots(ids)
            .into_iter()
            .flat_map(|id| self.subtree(id))
            .collect()
    }

    /// Archive a whole selection and commit its index once. Failed file/index
    /// writes restore the previous tree and every file already archived.
    pub fn trash_notes(&mut self, dir: &Path, selected: &[u64]) -> io::Result<Vec<u64>> {
        if selected.iter().any(|&id| self.get(id).is_none()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unknown selected note",
            ));
        }
        let ids = self.selected_subtrees(selected);
        if ids.is_empty() {
            return Ok(ids);
        }
        let trash = dir.join("trash");
        fs::create_dir_all(&trash)?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut moved = Vec::new();
        for &id in &ids {
            let source = self.file(dir, id);
            let target = trash.join(format!("{id}-{stamp}.txt"));
            if let Err(error) = fs::rename(&source, &target) {
                for (from, to) in moved.iter().rev() {
                    let _ = fs::rename(to, from);
                }
                return Err(error);
            }
            moved.push((source, target));
        }
        let previous = self.clone();
        let deleted: std::collections::HashSet<_> = ids.iter().copied().collect();
        self.notes.retain(|n| !deleted.contains(&n.id));
        if let Err(error) = self.save(dir) {
            *self = previous;
            for (from, to) in moved.iter().rev() {
                let _ = fs::rename(to, from);
            }
            return Err(error);
        }
        Ok(ids)
    }

    fn set_orders(&mut self, parent: u64, siblings: &[u64]) {
        for (order, &id) in siblings.iter().enumerate() {
            if let Some(n) = self.get_mut(id) {
                n.parent = parent;
                n.order = order as u32;
            }
        }
    }

    pub fn subtree(&self, id: u64) -> Vec<u64> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            out.extend(self.children(out[i]));
            i += 1;
        }
        out
    }

    pub fn remove(&mut self, id: u64) -> Vec<u64> {
        let ids = self.subtree(id);
        self.notes.retain(|n| !ids.contains(&n.id));
        ids
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 15) as usize] as char);
    }
    out
}

fn decode_hex(text: &str) -> io::Result<Vec<u8>> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "Invalid title encoding");
    if !text.len().is_multiple_of(2) {
        return Err(bad());
    }
    let mut out = Vec::new();
    for pair in text.as_bytes().chunks_exact(2) {
        let digit = |c: u8| (c as char).to_digit(16).map(|v| v as u8).ok_or_else(bad);
        out.push((digit(pair[0])? << 4) | digit(pair[1])?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_moves_preserve_tree_order_and_validate_before_mutating() {
        let mut book = Notebook {
            next_id: 1,
            ..Default::default()
        };
        let a = book.add(0, "A".into());
        let b = book.add(0, "B".into());
        let c = book.add(0, "C".into());
        let child = book.add(a, "A1".into());
        let folder = book.add(0, "Folder".into());
        let existing = book.add(folder, "existing".into());
        let selected = [c, child, a, c];
        assert_eq!(book.selected_roots(&selected), vec![a, c]);
        let before = book.notes.clone();
        assert!(!book.move_notes(&selected, child, None));
        assert!(!book.move_notes(&[a, 999], folder, None));
        assert!(!book.move_notes(&selected, folder, Some(b)));
        assert_eq!(book.notes, before);
        assert!(book.move_notes(&selected, folder, Some(existing)));
        assert_eq!(book.children(0), vec![b, folder]);
        assert_eq!(book.children(folder), vec![a, c, existing]);
        assert_eq!(book.get(child).unwrap().parent, a);
        assert!(book.move_notes(&[c, a], folder, None));
        assert_eq!(book.children(folder), vec![existing, a, c]);
    }

    #[test]
    fn grouping_uses_common_parent_and_keeps_selected_subtrees() {
        let mut book = Notebook {
            next_id: 1,
            ..Default::default()
        };
        let parent = book.add(0, "parent".into());
        let a = book.add(parent, "A".into());
        let untouched = book.add(parent, "untouched".into());
        let b = book.add(parent, "B".into());
        let child = book.add(a, "child".into());
        let folder = book.group_notes(&[b, child, a]).unwrap();
        assert_eq!(book.children(parent), vec![folder, untouched]);
        assert_eq!(book.children(folder), vec![a, b]);
        assert_eq!(book.get(child).unwrap().parent, a);
        let sibling = book.add(0, "elsewhere".into());
        let deep = book.add(sibling, "deep".into());
        let outer = book.group_notes(&[deep, a]).unwrap();
        assert_eq!(book.get(outer).unwrap().parent, 0);
        assert_eq!(book.children(outer), vec![a, deep]);
        assert!(book.get(untouched).is_some());
    }

    #[test]
    fn batch_trash_restores_files_on_failures_and_archives_each_note_once() {
        let dir = std::env::temp_dir().join(format!(
            "edot-batch-trash-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut book = Notebook::load(&dir).unwrap();
        let a = book.add(0, "A".into());
        let child = book.add(a, "child".into());
        let b = book.add(0, "B".into());
        for id in [a, child, b] {
            fs::write(book.file(&dir, id), format!("text {id}")).unwrap();
        }
        book.save(&dir).unwrap();
        let previous = book.notes.clone();
        fs::remove_file(book.file(&dir, b)).unwrap();
        assert!(book.trash_notes(&dir, &[a, child, b]).is_err());
        assert_eq!(book.notes, previous);
        for id in [a, child] {
            assert_eq!(
                fs::read_to_string(book.file(&dir, id)).unwrap(),
                format!("text {id}")
            );
        }
        fs::write(book.file(&dir, b), format!("text {b}")).unwrap();
        fs::create_dir(dir.join("tree.tmp")).unwrap(); // Force index write failure.
        assert!(book.trash_notes(&dir, &[a, child, b]).is_err());
        assert_eq!(book.notes, previous);
        for id in [a, child, b] {
            assert!(book.file(&dir, id).is_file());
        }
        fs::remove_dir(dir.join("tree.tmp")).unwrap();
        assert_eq!(
            book.trash_notes(&dir, &[b, child, a, a]).unwrap(),
            vec![a, child, b]
        );
        assert_eq!(fs::read_dir(dir.join("trash")).unwrap().count(), 3);
        assert!(Notebook::load(&dir).unwrap().notes.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn moving_and_persisting_nested_notes() {
        let dir = std::env::temp_dir().join(format!("edot-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut book = Notebook::load(&dir).unwrap();
        let a = book.add(0, "가나다".into());
        let b = book.add(0, "B".into());
        let c = book.add(a, "C".into());
        assert!(!book.move_note(a, c, None));
        assert!(book.move_note(b, a, Some(c)));
        assert_eq!(book.children(a), vec![b, c]);
        book.save(&dir).unwrap();
        let loaded = Notebook::load(&dir).unwrap();
        assert_eq!(loaded.get(a).unwrap().title, "가나다");
        assert_eq!(loaded.children(a), vec![b, c]);
        let mut removed = loaded.clone();
        assert_eq!(removed.remove(a), vec![a, b, c]);
        assert!(removed.notes.is_empty());
        let file = loaded.file(&dir, a);
        replace_file(&file, "첫 내용".as_bytes()).unwrap();
        replace_file(&file, "수정된 내용".as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(file).unwrap(), "수정된 내용");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recovers_an_unindexed_text_file_without_reusing_its_id() {
        let dir = std::env::temp_dir().join(format!("edot-orphan-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("notes")).unwrap();
        fs::write(dir.join("notes/7.txt"), "살아있는 내용").unwrap();
        let mut book = Notebook::load(&dir).unwrap();
        assert_eq!(book.get(7).unwrap().title, "복구된 메모 7");
        assert_eq!(book.add(0, "새 메모".into()), 8);
        let _ = fs::remove_dir_all(dir);
    }
}
