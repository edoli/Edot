use crate::storage::replace_file;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A stable numeric identity; tree IDs use base64url, sessions use binary, file names use decimal.
pub type NoteId = u64;

// Unpadded base64url of exactly eight big-endian bytes: ten full sextets
// followed by the remaining four bits and two zero padding bits.
fn encode_note_id(id: NoteId) -> [u8; 11] {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut encoded = [0; 11];
    for (index, byte) in encoded[..10].iter_mut().enumerate() {
        *byte = ALPHABET[((id >> (58 - index * 6)) & 63) as usize];
    }
    encoded[10] = ALPHABET[((id & 15) << 2) as usize];
    encoded
}

fn decode_note_id(text: &str) -> Option<NoteId> {
    if text.len() != 11 {
        return None;
    }
    let mut id = 0;
    for (index, byte) in text.bytes().enumerate() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        if index == 10 {
            // Reject alternate spellings with nonzero unused padding bits.
            if value & 3 != 0 {
                return None;
            }
            return Some((id << 4) | NoteId::from(value >> 2));
        }
        id = (id << 6) | NoteId::from(value);
    }
    None
}

fn random_note_id() -> io::Result<NoteId> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes)
        .map_err(|error| io::Error::other(format!("노트 ID를 생성하지 못했습니다 ({error})")))?;
    Ok(NoteId::from_le_bytes(bytes))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: NoteId,
    pub parent: NoteId,
    pub order: u32,
    pub title: String,
}

#[derive(Clone, Default)]
pub struct Notebook {
    pub notes: Vec<Note>,
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
                String::from("EDOT4\n")
            }
            Err(error) => return Err(error),
        };
        let mut lines = text.lines();
        if lines.next() != Some("EDOT4") {
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
            let id = decode_note_id(parts.next().ok_or_else(invalid)?).ok_or_else(invalid)?;
            let parent = decode_note_id(parts.next().ok_or_else(invalid)?).ok_or_else(invalid)?;
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
                .and_then(|s| s.parse::<NoteId>().ok())
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
                title: format!("복구된 노트 {id}"),
            });
        }
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
        Ok(Self { notes })
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        let mut text = String::from("EDOT4\n");
        for n in &self.notes {
            if n.title.contains(['\t', '\r', '\n']) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Titles cannot contain tabs or newlines",
                ));
            }
            // The encoder emits only ASCII; conversion does not allocate.
            let id = encode_note_id(n.id);
            let parent = encode_note_id(n.parent);
            text.push_str(std::str::from_utf8(&id).expect("base64url is ASCII"));
            text.push('\t');
            text.push_str(std::str::from_utf8(&parent).expect("base64url is ASCII"));
            text.push_str(&format!("\t{}\t{}\n", n.order, n.title));
        }
        replace_file(&dir.join("tree.tsv"), text.as_bytes())
    }

    pub fn file(&self, dir: &Path, id: NoteId) -> PathBuf {
        dir.join("notes").join(format!("{id}.txt"))
    }

    pub fn add(&mut self, parent: NoteId, title: String) -> io::Result<NoteId> {
        self.add_with_rng(parent, title, random_note_id)
    }

    fn add_with_rng(
        &mut self,
        parent: NoteId,
        title: String,
        mut random: impl FnMut() -> io::Result<NoteId>,
    ) -> io::Result<NoteId> {
        for _ in 0..64 {
            let id = random()?;
            if id == 0 || self.get(id).is_some() {
                continue;
            }
            let order = self.notes.iter().filter(|n| n.parent == parent).count() as u32;
            self.notes.push(Note {
                id,
                parent,
                order,
                title,
            });
            return Ok(id);
        }
        Err(io::Error::other(
            "중복되지 않는 노트 ID를 생성하지 못했습니다.",
        ))
    }

    pub fn get(&self, id: NoteId) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }
    pub fn get_mut(&mut self, id: NoteId) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    pub fn children(&self, parent: NoteId) -> Vec<NoteId> {
        let mut children: Vec<_> = self.notes.iter().filter(|n| n.parent == parent).collect();
        children.sort_by_key(|n| (n.order, n.id));
        children.into_iter().map(|n| n.id).collect()
    }

    pub fn is_descendant(&self, ancestor: NoteId, candidate: NoteId) -> bool {
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

    pub fn move_note(&mut self, id: NoteId, parent: NoteId, before: Option<NoteId>) -> bool {
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
    pub fn selected_roots(&self, ids: &[NoteId]) -> Vec<NoteId> {
        if ids.is_empty() {
            return Vec::new();
        }
        let selected: std::collections::HashSet<_> = ids.iter().copied().collect();
        let mut children: std::collections::HashMap<NoteId, Vec<_>> =
            std::collections::HashMap::new();
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

    pub fn move_notes(&mut self, ids: &[NoteId], parent: NoteId, before: Option<NoteId>) -> bool {
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
        let mut affected: std::collections::HashMap<NoteId, Vec<_>> = roots
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

    pub fn group_notes(&mut self, ids: &[NoteId]) -> io::Result<Option<NoteId>> {
        if ids.is_empty() || ids.iter().any(|&id| self.get(id).is_none()) {
            return Ok(None);
        }
        let roots = self.selected_roots(ids);
        let Some(first) = roots.first().and_then(|&id| self.get(id)) else {
            return Ok(None);
        };
        let mut parent = first.parent;
        while parent != 0 && !roots.iter().all(|&id| self.is_descendant(parent, id)) {
            let Some(ancestor) = self.get(parent) else {
                return Ok(None);
            };
            parent = ancestor.parent;
        }
        let before = self.children(parent).into_iter().find(|&id| {
            roots
                .iter()
                .any(|&root| root == id || self.is_descendant(id, root))
        });
        let folder = self.add(parent, "새 폴더".into())?;
        self.move_note(folder, parent, before);
        self.move_notes(&roots, folder, None);
        Ok(Some(folder))
    }

    pub fn selected_subtrees(&self, ids: &[NoteId]) -> Vec<NoteId> {
        self.selected_roots(ids)
            .into_iter()
            .flat_map(|id| self.subtree(id))
            .collect()
    }

    /// Archive a whole selection and commit its index once. Failed file/index
    /// writes restore the previous tree and every file already archived.
    pub fn trash_notes(&mut self, dir: &Path, selected: &[NoteId]) -> io::Result<Vec<NoteId>> {
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

    fn set_orders(&mut self, parent: NoteId, siblings: &[NoteId]) {
        for (order, &id) in siblings.iter().enumerate() {
            if let Some(n) = self.get_mut(id) {
                n.parent = parent;
                n.order = order as u32;
            }
        }
    }

    pub fn subtree(&self, id: NoteId) -> Vec<NoteId> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            out.extend(self.children(out[i]));
            i += 1;
        }
        out
    }

    pub fn remove(&mut self, id: NoteId) -> Vec<NoteId> {
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
    fn base64url_ids_match_standard_vectors_and_reject_invalid_encodings() {
        for (id, expected) in [
            (0, "AAAAAAAAAAA"),
            (1, "AAAAAAAAAAE"),
            (0x0123_4567_89ab_cdef, "ASNFZ4mrze8"),
            (0xfb00_0000_0000_0000, "-wAAAAAAAAA"),
            (NoteId::MAX, "__________8"),
        ] {
            assert_eq!(encode_note_id(id).as_slice(), expected.as_bytes());
            assert_eq!(decode_note_id(expected), Some(id));
        }
        for bit in 0..64 {
            for id in [1 << bit, !(1 << bit)] {
                let encoded = encode_note_id(id);
                assert_eq!(
                    decode_note_id(std::str::from_utf8(&encoded).unwrap()),
                    Some(id)
                );
            }
        }
        for invalid in [
            "",
            "AAAAAAAAAA",
            "AAAAAAAAAAAA",
            "AAAAAAAAAAE=",
            "AAAAAAAAAA+",
            "AAAAAAAAAA/",
            "AAAAAAAAAAé",
            "AAAAAAAAA E",
            "AAAAAAAAAAF",
            "___________",
        ] {
            assert_eq!(decode_note_id(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn random_ids_survive_reload_and_reject_collisions_without_mutation() {
        let dir = loader_test_dir("random-ids");
        let original = "EDOT4\nAAAAAAAAAAc\tAAAAAAAAAAA\t0\told\nAAAAAAAAAAo\tAAAAAAAAAAA\t1\tten\n__________8\tAAAAAAAAAAo\t0\tmax\n";
        fs::write(dir.join("tree.tsv"), original).unwrap();
        fs::write(dir.join("notes/7.txt"), "existing body").unwrap();
        let mut book = Notebook::load(&dir).unwrap();
        let high = (1u64 << 63) + 7;
        let mut candidates = [0, 7, NoteId::MAX, high].into_iter();
        assert_eq!(
            book.add_with_rng(7, "child".into(), || Ok(candidates.next().unwrap()))
                .unwrap(),
            high
        );
        let before = book.notes.clone();
        assert!(
            book.add_with_rng(0, "failed".into(), || Err(io::Error::other("rng failed")))
                .is_err()
        );
        assert!(book.add_with_rng(0, "collision".into(), || Ok(7)).is_err());
        assert_eq!(book.notes, before);
        // Equal snapshots represent independent devices with no shared counter.
        let mut other = book.clone();
        let mut ids = std::collections::HashSet::from([7, 10, NoteId::MAX, high]);
        for _ in 0..64 {
            for book in [&mut book, &mut other] {
                let id = book.add(0, "new".into()).unwrap();
                assert_ne!(id, 0);
                assert!(ids.insert(id));
            }
        }
        assert_eq!(fs::read_to_string(dir.join("tree.tsv")).unwrap(), original);
        book.save(&dir).unwrap();
        let saved = fs::read_to_string(dir.join("tree.tsv")).unwrap();
        assert!(saved.starts_with("EDOT4\n"));
        assert!(saved.contains("AAAAAAAAAAo\tAAAAAAAAAAA\t1\tten\n"));
        assert!(saved.contains("__________8\tAAAAAAAAAAo\t0\tmax\n"));
        assert!(saved.contains("gAAAAAAAAAc\tAAAAAAAAAAc\t0\tchild\n"));
        assert_eq!(Notebook::reload(&dir).unwrap().notes, book.notes);
        assert_eq!(book.file(&dir, high), dir.join(format!("notes/{high}.txt")));
        assert_eq!(
            fs::read_to_string(dir.join("notes/7.txt")).unwrap(),
            "existing body"
        );
        fs::write(book.file(&dir, high), "new body").unwrap();
        book.trash_notes(&dir, &[high]).unwrap();
        let mut reopened = Notebook::load(&dir).unwrap();
        assert!(reopened.get(high).is_none());
        assert!(!ids.contains(&reopened.add(7, "after restart".into()).unwrap()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn plain_titles_and_decimal_order_round_trip() {
        let dir = loader_test_dir("plain-titles");
        let text = "EDOT4\r\nAAAAAAAAABA\tAAAAAAAAAAA\t10\t한글 😀 제목\r\nAAAAAAAAAAI\tAAAAAAAAABA\t0\t414243\r\nAAAAAAAAAAM\tAAAAAAAAAAA\t1\t\r\n";
        fs::write(dir.join("tree.tsv"), text).unwrap();
        let mut book = Notebook::reload(&dir).unwrap();
        assert_eq!(book.get(16).unwrap().title, "한글 😀 제목");
        assert_eq!(book.get(16).unwrap().order, 10);
        assert_eq!(book.get(2).unwrap().parent, 16);
        assert!(book.get(10).is_none());
        assert_eq!(book.file(&dir, 16), dir.join("notes/16.txt"));
        assert_eq!(book.get(2).unwrap().title, "414243");
        assert_eq!(book.get(3).unwrap().title, "");
        assert_eq!(fs::read_to_string(dir.join("tree.tsv")).unwrap(), text);
        book.save(&dir).unwrap();
        let saved = text.replace("\r\n", "\n");
        assert_eq!(fs::read_to_string(dir.join("tree.tsv")).unwrap(), saved);
        assert_eq!(Notebook::load(&dir).unwrap().notes, book.notes);
        for title in ["탭\t제목", "줄\r\n바꿈"] {
            book.get_mut(16).unwrap().title = title.into();
            assert_eq!(
                book.save(&dir).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(fs::read_to_string(dir.join("tree.tsv")).unwrap(), saved);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn live_reload_requires_an_index_and_does_not_resurrect_leftover_bodies() {
        let dir = loader_test_dir("reload");
        fs::write(dir.join("notes/7.txt"), "leftover body").unwrap();
        assert!(Notebook::reload(&dir).is_err());
        assert!(!dir.join("tree.tsv").exists());
        fs::write(dir.join("tree.tsv"), "EDOT4\n").unwrap();
        assert!(Notebook::reload(&dir).unwrap().notes.is_empty());
        let recovered = Notebook::load(&dir).unwrap();
        assert_eq!(recovered.notes[0].id, 7);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loader_rejects_invalid_trees_without_rewriting_files() {
        let dir = loader_test_dir("invalid");
        fs::write(dir.join("notes/9.txt"), "unchanged orphan").unwrap();
        for text in [
            "WRONG\n",
            "EDOT1\n1\t0\t0\t414243\n",
            "EDOT2\n1\t0\t0\tplain title\n",
            "EDOT3\n1\t0\t0\tplain title\n",
            "EDOT4\n1\t0\t0\tdecimal IDs\n",
            "EDOT4\nAAAAAAAAAAE\tAAAAAAAAAAA\t0\tbad\rtitle\n",
            "EDOT4\nAAAAAAAAAAE\tAAAAAAAAAAA\t0",
            "EDOT4\nAAAAAAAAAAA\tAAAAAAAAAAA\t0\t41\n",
            "EDOT4\nAAAAAAAAAAE\tAAAAAAAAAAA\t0\t41\nAAAAAAAAAAE\tAAAAAAAAAAA\t1\t42\n",
            "EDOT4\nAAAAAAAAAAE\tAAAAAAAAAAA\t0\t41\textra\n",
            "EDOT4\nAAAAAAAAAAE\tAAAAAAAAAAk\t0\t41\n", // An orphan file cannot repair a missing parent.
            "EDOT4\nAAAAAAAAAAE\tAAAAAAAAAAE\t0\t41\n",
            "EDOT4\nAAAAAAAAAAM\tAAAAAAAAAAE\t0\t43\nAAAAAAAAAAQ\tAAAAAAAAAAA\t0\t44\nAAAAAAAAAAE\tAAAAAAAAAAI\t0\t41\nAAAAAAAAAAI\tAAAAAAAAAAE\t0\t42\n",
            "EDOT4\nAAAAAAAAAAF\tAAAAAAAAAAA\t0\tinvalid padding bits\n",
            "EDOT4\nAAAAAAAAAAE\t___________\t0\tinvalid parent\n",
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
    fn batch_moves_preserve_tree_order_and_validate_before_mutating() {
        let mut book = Notebook::default();
        let a = book.add(0, "A".into()).unwrap();
        let b = book.add(0, "B".into()).unwrap();
        let c = book.add(0, "C".into()).unwrap();
        let child = book.add(a, "A1".into()).unwrap();
        let folder = book.add(0, "Folder".into()).unwrap();
        let existing = book.add(folder, "existing".into()).unwrap();
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
        let a = book.add(0, "A".into()).unwrap();
        let child = book.add(a, "child".into()).unwrap();
        let b = book.add(0, "B".into()).unwrap();
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
}
