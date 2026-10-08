use crate::storage::replace_file;
use std::collections::HashMap;
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
        Self::read_index(dir, true)
    }

    /// Live reload reads only a complete index. Files may arrive separately;
    /// recovering orphans here would resurrect notes deleted in another app.
    pub fn reload(dir: &Path) -> io::Result<Self> {
        Self::read_index(dir, false)
    }

    fn read_index(dir: &Path, recover: bool) -> io::Result<Self> {
        let path = dir.join("tree.tsv");
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if recover && error.kind() == io::ErrorKind::NotFound => {
                String::from("EDOT2\n")
            }
            Err(error) => return Err(error),
        };
        let mut lines = text.lines();
        if lines.next() != Some("EDOT2") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported tree index",
            ));
        }
        let mut notes = Vec::new();
        let mut indices = HashMap::new();
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
            let field = parts.next().ok_or_else(invalid)?;
            if field.contains(['\t', '\r', '\n']) {
                return Err(invalid());
            }
            let title = field.to_owned();
            if parts.next().is_some() || id == 0 || indices.insert(id, notes.len()).is_some() {
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
            if note.parent != 0 && !indices.contains_key(&note.parent) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Missing parent note",
                ));
            }
        }
        // If a previous write stopped after creating a text file, keep that file visible.
        let mut root_count = notes.iter().filter(|n| n.parent == 0).count();
        let entries = if recover {
            Some(fs::read_dir(dir.join("notes"))?)
        } else {
            None
        };
        for entry in entries.into_iter().flatten() {
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
            if id == 0 || indices.contains_key(&id) {
                continue;
            }
            let order = root_count as u32;
            root_count += 1;
            indices.insert(id, notes.len());
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
        // 0 = unseen, 1 = on this parent chain, 2 = checked. Each note is
        // visited at most twice, without recursion even for very deep trees.
        let mut states = vec![0u8; notes.len()];
        for start in 0..notes.len() {
            let mut current = Some(start);
            while let Some(index) = current {
                match states[index] {
                    1 => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Tree contains a cycle",
                        ));
                    }
                    2 => break,
                    _ => states[index] = 1,
                }
                let parent = notes[index].parent;
                current = (parent != 0).then(|| indices[&parent]);
            }
            current = Some(start);
            while let Some(index) = current {
                if states[index] != 1 {
                    break;
                }
                states[index] = 2;
                let parent = notes[index].parent;
                current = (parent != 0).then(|| indices[&parent]);
            }
        }
        Ok(Self { notes, next_id })
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        let mut text = String::from("EDOT2\n");
        for n in &self.notes {
            if n.title.contains(['\t', '\r', '\n']) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Titles cannot contain tabs or newlines",
                ));
            }
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                n.id, n.parent, n.order, n.title
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

pub fn normalize_title(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\t', '\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loader_test_dir(label: &str) -> PathBuf {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "edot-model-{label}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("notes")).unwrap();
        dir
    }

    #[test]
    fn plain_titles_round_trip_without_hex_guessing() {
        let dir = loader_test_dir("plain-titles");
        let text = "EDOT2\r\n1\t0\t0\t한글 😀 제목\r\n2\t1\t0\t414243\r\n3\t0\t1\t\r\n";
        fs::write(dir.join("tree.tsv"), text).unwrap();
        let book = Notebook::reload(&dir).unwrap();
        assert_eq!(book.get(1).unwrap().title, "한글 😀 제목");
        assert_eq!(book.get(2).unwrap().title, "414243");
        assert_eq!(book.get(3).unwrap().title, "");
        book.save(&dir).unwrap();
        let saved = fs::read_to_string(dir.join("tree.tsv")).unwrap();
        assert_eq!(saved, text.replace("\r\n", "\n"));
        assert_eq!(Notebook::load(&dir).unwrap().notes, book.notes);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_plain_titles_cannot_corrupt_the_index() {
        let dir = loader_test_dir("invalid-titles");
        let original = "EDOT2\n1\t0\t0\t원래 제목\n";
        fs::write(dir.join("tree.tsv"), original).unwrap();
        let mut book = Notebook::load(&dir).unwrap();
        for title in ["탭\t제목", "줄\n바꿈", "줄\r바꿈", "줄\r\n바꿈"] {
            book.get_mut(1).unwrap().title = title.into();
            assert_eq!(
                book.save(&dir).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(fs::read_to_string(dir.join("tree.tsv")).unwrap(), original);
            assert!(!dir.join("tree.tmp").exists());
        }
        for text in [
            "EDOT2\n1\t0\t0\t탭\t제목\n",
            "EDOT2\n1\t0\t0\t줄\r바꿈\n",
            "EDOT2\n1\t0\t0\t줄\n바꿈\n",
        ] {
            fs::write(dir.join("tree.tsv"), text).unwrap();
            assert_eq!(
                Notebook::reload(&dir).err().unwrap().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(fs::read_to_string(dir.join("tree.tsv")).unwrap(), text);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loader_preserves_row_order_forward_parents_and_sparse_ids() {
        let dir = loader_test_dir("rows");
        let expected = Notebook {
            notes: vec![
                Note {
                    id: u64::MAX,
                    parent: 42,
                    order: 7,
                    title: "한글 제목 😀".into(),
                },
                Note {
                    id: 3,
                    parent: 42,
                    order: 7,
                    title: String::new(),
                },
                Note {
                    id: 42,
                    parent: 0,
                    order: 99,
                    title: "root".into(),
                },
            ],
            next_id: u64::MAX,
        };
        expected.save(&dir).unwrap();
        let original = fs::read(dir.join("tree.tsv")).unwrap();
        let loaded = Notebook::load(&dir).unwrap();
        assert_eq!(loaded.notes, expected.notes);
        assert_eq!(loaded.next_id, u64::MAX);
        assert_eq!(loaded.children(42), vec![3, u64::MAX]);
        assert_eq!(fs::read(dir.join("tree.tsv")).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn live_reload_requires_an_index_and_does_not_resurrect_leftover_bodies() {
        let dir = loader_test_dir("reload");
        fs::write(dir.join("notes/7.txt"), "leftover body").unwrap();
        assert!(Notebook::reload(&dir).is_err());
        assert!(!dir.join("tree.tsv").exists());
        fs::write(dir.join("tree.tsv"), "EDOT2\n").unwrap();
        assert!(Notebook::reload(&dir).unwrap().notes.is_empty());
        assert_eq!(Notebook::load(&dir).unwrap().notes[0].id, 7);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loader_rejects_invalid_trees_without_rewriting_files() {
        let dir = loader_test_dir("invalid");
        fs::write(dir.join("notes/9.txt"), "unchanged orphan").unwrap();
        for text in [
            "WRONG\n",
            "EDOT1\n1\t0\t0\t414243\n",
            "EDOT2\n1\t0\t0\tbad\rtitle\n",
            "EDOT2\n1\t0\t0",
            "EDOT2\n0\t0\t0\t41\n",
            "EDOT2\n1\t0\t0\t41\n1\t0\t1\t42\n",
            "EDOT2\n1\t0\t0\t41\textra\n",
            "EDOT2\n1\t9\t0\t41\n", // An orphan file cannot repair a missing parent.
            "EDOT2\n1\t1\t0\t41\n",
            "EDOT2\n3\t1\t0\t43\n4\t0\t0\t44\n1\t2\t0\t41\n2\t1\t0\t42\n",
        ] {
            fs::write(dir.join("tree.tsv"), text).unwrap();
            let error = Notebook::load(&dir).err().expect("invalid tree accepted");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{text}");
            assert_eq!(fs::read_to_string(dir.join("tree.tsv")).unwrap(), text);
            assert_eq!(
                fs::read_to_string(dir.join("notes/9.txt")).unwrap(),
                "unchanged orphan"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loader_recovers_each_orphan_id_once_with_existing_root_count() {
        let dir = loader_test_dir("recovery");
        fs::write(dir.join("tree.tsv"), "EDOT2\n2\t50\t0\t43\n50\t0\t9\t52\n").unwrap();
        for name in [
            "2.txt", "7.txt", "007.txt", "11.txt", "0.txt", "a.txt", "12.TXT",
        ] {
            fs::write(dir.join("notes").join(name), name).unwrap();
        }
        fs::create_dir(dir.join("notes/13.txt")).unwrap();
        let book = Notebook::load(&dir).unwrap();
        assert_eq!(book.notes.len(), 4);
        assert_eq!(book.notes[0].id, 2);
        assert_eq!(book.notes[1].id, 50);
        assert_eq!(book.notes[1].order, 9);
        assert_eq!(book.next_id, 51);
        for (offset, note) in book.notes[2..].iter().enumerate() {
            assert!([7, 11].contains(&note.id));
            assert_eq!(note.parent, 0);
            assert_eq!(note.order, offset as u32 + 1);
            assert_eq!(note.title, format!("복구된 메모 {}", note.id));
        }
        assert_ne!(book.notes[2].id, book.notes[3].id);
        for name in ["7.txt", "007.txt", "11.txt"] {
            assert_eq!(
                fs::read_to_string(dir.join("notes").join(name)).unwrap(),
                name
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loader_checks_a_deep_reverse_order_chain_and_its_cycle() {
        use std::fmt::Write;
        let dir = loader_test_dir("deep");
        const COUNT: u64 = 50_000;
        for cyclic in [false, true] {
            let mut text = String::from("EDOT2\n");
            for id in (1..=COUNT).rev() {
                let parent = if cyclic && id == 1 { COUNT } else { id - 1 };
                writeln!(text, "{id}\t{parent}\t0\t41").unwrap();
            }
            fs::write(dir.join("tree.tsv"), &text).unwrap();
            let result = Notebook::load(&dir);
            if cyclic {
                assert_eq!(
                    result.err().expect("cycle accepted").to_string(),
                    "Tree contains a cycle"
                );
            } else {
                let book = result.unwrap();
                assert_eq!(book.notes.len(), COUNT as usize);
                assert_eq!(book.notes[0].id, COUNT);
                assert_eq!(book.notes.last().unwrap().parent, 0);
                assert_eq!(book.next_id, COUNT + 1);
            }
            assert_eq!(fs::read_to_string(dir.join("tree.tsv")).unwrap(), text);
        }
        fs::remove_dir_all(dir).unwrap();
    }

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
        let dir = loader_test_dir("persist");
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
        let dir = loader_test_dir("orphan");
        fs::write(dir.join("notes/7.txt"), "살아있는 내용").unwrap();
        let mut book = Notebook::load(&dir).unwrap();
        assert_eq!(book.get(7).unwrap().title, "복구된 메모 7");
        assert_eq!(book.add(0, "새 메모".into()), 8);
        let _ = fs::remove_dir_all(dir);
    }
}
