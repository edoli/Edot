//! Archived note bodies, optional metadata and collision-safe restoration.
use crate::model::{Note, NoteId, Notebook};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Entry {
    pub note: Note,
    pub stamp: u128,
    pub has_metadata: bool,
}
impl Entry {
    pub fn key(&self) -> String {
        format!("{}-{}", self.note.id, self.stamp)
    }
    fn body(&self, dir: &Path) -> PathBuf {
        dir.join("trash").join(format!("{}.txt", self.key()))
    }
    fn metadata(&self, dir: &Path) -> PathBuf {
        dir.join("trash").join(format!("{}.meta", self.key()))
    }
}

pub fn list(dir: &Path) -> io::Result<Vec<Entry>> {
    let files = match fs::read_dir(dir.join("trash")) {
        Ok(files) => files,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let mut entries = vec![];
    for file in files {
        let file = file?;
        if !file.file_type()?.is_file() {
            continue;
        }
        let name = file.file_name();
        let Some((id, stamp)) = name
            .to_str()
            .and_then(|name| name.strip_suffix(".txt"))
            .and_then(|stem| stem.split_once('-'))
        else {
            continue;
        };
        let (Ok(id), Ok(stamp)) = (id.parse::<NoteId>(), stamp.parse::<u128>()) else {
            continue;
        };
        if id == 0 || name != format!("{id}-{stamp}.txt").as_str() {
            continue;
        }
        let mut entry = Entry {
            note: Note {
                id,
                parent: 0,
                order: u32::MAX,
                title: format!("삭제된 노트 {id}"),
            },
            stamp,
            has_metadata: false,
        };
        if let Ok(text) = fs::read_to_string(entry.metadata(dir))
            && let Some(note) = parse_metadata(&text, id)
        {
            entry.note = note;
            entry.has_metadata = true;
        }
        entries.push(entry);
    }
    entries.sort_by(|a, b| {
        b.stamp
            .cmp(&a.stamp)
            .then(a.note.order.cmp(&b.note.order))
            .then(a.note.id.cmp(&b.note.id))
    });
    Ok(entries)
}

fn parse_metadata(text: &str, id: NoteId) -> Option<Note> {
    let mut lines = text.lines();
    if lines.next()? != "EDOTTRASH1" {
        return None;
    }
    let mut fields = lines.next()?.splitn(4, '\t');
    if fields.next()?.parse::<NoteId>().ok()? != id {
        return None;
    }
    let parent = fields.next()?.parse().ok()?;
    let order = fields.next()?.parse().ok()?;
    let title = fields.next()?.to_owned();
    if parent == id || title.contains(['\t', '\r', '\n']) || lines.next().is_some() {
        return None;
    }
    Some(Note {
        id,
        parent,
        order,
        title,
    })
}

pub fn archive(book: &mut Notebook, dir: &Path, selected: &[NoteId]) -> io::Result<Vec<NoteId>> {
    if selected.iter().any(|&id| book.get(id).is_none()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unknown selected note",
        ));
    }
    let ids = book.selected_subtrees(selected);
    if ids.is_empty() {
        return Ok(ids);
    }
    fs::create_dir_all(dir.join("trash"))?;
    let mut stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Reserve a batch name before moving any bodies; never replace an archive.
    while ids.iter().any(|id| {
        dir.join("trash").join(format!("{id}-{stamp}.txt")).exists()
            || dir
                .join("trash")
                .join(format!("{id}-{stamp}.meta"))
                .exists()
    }) {
        stamp = stamp
            .checked_add(1)
            .ok_or_else(|| io::Error::other("휴지통 이름을 만들지 못했습니다"))?;
    }
    let entries: Vec<_> = ids
        .iter()
        .map(|id| Entry {
            note: book.get(*id).unwrap().clone(),
            stamp,
            has_metadata: true,
        })
        .collect();
    let mut metadata = vec![];
    let mut moved = vec![];
    let previous = book.clone();
    let result = (|| {
        for entry in &entries {
            if entry.note.title.contains(['\t', '\r', '\n']) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "잘못된 노트 제목입니다",
                ));
            }
            let path = entry.metadata(dir);
            let mut file = fs::File::create_new(&path)?;
            metadata.push(path);
            writeln!(
                file,
                "EDOTTRASH1\n{}\t{}\t{}\t{}",
                entry.note.id, entry.note.parent, entry.note.order, entry.note.title
            )?;
        }
        for entry in &entries {
            let source = book.file(dir, entry.note.id);
            let target = entry.body(dir);
            if target.try_exists()? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "휴지통 파일이 이미 존재합니다",
                ));
            }
            fs::rename(&source, &target)?;
            moved.push((source, target));
        }
        let deleted: HashSet<_> = ids.iter().copied().collect();
        book.notes.retain(|n| !deleted.contains(&n.id));
        book.save(dir)
    })();
    if let Err(error) = result {
        *book = previous;
        let mut failures = vec![];
        for (source, target) in moved.iter().rev() {
            if let Err(e) = fs::rename(target, source) {
                failures.push(e.to_string());
            }
        }
        for path in metadata {
            if !path.with_extension("txt").exists() {
                let _ = fs::remove_file(path);
            }
        }
        return if failures.is_empty() {
            Err(error)
        } else {
            Err(io::Error::other(format!(
                "{error} · 본문 복구 실패: {}",
                failures.join("; ")
            )))
        };
    }
    Ok(ids)
}

pub fn subtree(entries: &[Entry], root: &Entry) -> Vec<Entry> {
    let mut children: HashMap<NoteId, Vec<&Entry>> = HashMap::new();
    if root.has_metadata {
        for entry in entries
            .iter()
            .filter(|e| e.has_metadata && e.stamp == root.stamp)
        {
            children.entry(entry.note.parent).or_default().push(entry);
        }
    }
    let mut result = vec![];
    let mut pending = vec![root];
    let mut seen = HashSet::new();
    while let Some(entry) = pending.pop() {
        if seen.insert(entry.note.id) {
            result.push(entry.clone());
            if let Some(children) = children.get(&entry.note.id) {
                pending.extend(children.iter().rev().copied());
            }
        }
    }
    result
}

pub struct Restored {
    pub ids: Vec<NoteId>,
    pub warning: Option<String>,
}
pub fn restore(book: &mut Notebook, dir: &Path, key: &str) -> io::Result<Restored> {
    let entries = list(dir)?;
    let root = entries
        .iter()
        .find(|e| e.key() == key)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "휴지통 항목이 사라졌습니다"))?;
    let selected = subtree(&entries, root);
    let mut candidate = book.clone();
    let mut mapped = HashMap::new();
    let mut used: HashSet<_> = book.notes.iter().map(|n| n.id).collect();
    let mut created = vec![];
    let result = (|| {
        for entry in &selected {
            let mut id = entry.note.id;
            if used.contains(&id) || candidate.file(dir, id).try_exists()? {
                id = (0..64)
                    .find_map(|_| {
                        let mut bytes = [0; 8];
                        match getrandom::fill(&mut bytes) {
                            Ok(()) => {
                                let id = u64::from_le_bytes(bytes);
                                (id != 0
                                    && !used.contains(&id)
                                    && !candidate.file(dir, id).exists())
                                .then_some(Ok(id))
                            }
                            Err(e) => Some(Err(io::Error::other(e.to_string()))),
                        }
                    })
                    .ok_or_else(|| io::Error::other("복원할 노트 ID를 만들지 못했습니다"))??;
            }
            let path = candidate.file(dir, id);
            let mut target = fs::File::create_new(&path)?;
            created.push(path);
            io::copy(&mut fs::File::open(entry.body(dir))?, &mut target)?;
            used.insert(id);
            mapped.insert(entry.note.id, id);
        }
        for entry in &selected {
            let mut note = entry.note.clone();
            note.id = mapped[&note.id];
            note.parent = if entry.note.id != root.note.id && mapped.contains_key(&note.parent) {
                mapped[&note.parent]
            } else if !mapped.contains_key(&note.parent)
                && book.get(note.parent).is_some()
                && !entries
                    .iter()
                    .any(|e| e.stamp == entry.stamp && e.note.id == note.parent)
            {
                note.parent
            } else {
                0
            };
            candidate.notes.push(note);
        }
        candidate.save(dir)
    })();
    if let Err(error) = result {
        let mut failures = vec![];
        for path in created {
            if let Err(e) = fs::remove_file(path) {
                failures.push(e.to_string());
            }
        }
        return if failures.is_empty() {
            Err(error)
        } else {
            Err(io::Error::other(format!(
                "{error} · 복원 임시 본문 정리 실패: {}",
                failures.join("; ")
            )))
        };
    }
    *book = candidate;
    let mut retained = 0;
    for entry in &selected {
        if fs::remove_file(entry.body(dir)).is_err() {
            retained += 1;
        } else {
            let _ = fs::remove_file(entry.metadata(dir));
        }
    }
    Ok(Restored {
        ids: selected.iter().map(|e| mapped[&e.note.id]).collect(),
        warning: (retained > 0)
            .then(|| format!("복원되었습니다. 휴지통 파일 {retained}개를 정리하지 못했습니다.")),
    })
}

pub const PREVIEW_LIMIT: u64 = 256 * 1024;
pub fn preview(dir: &Path, entry: &Entry) -> io::Result<(String, bool)> {
    let mut bytes = vec![];
    fs::File::open(entry.body(dir))?
        .take(PREVIEW_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() as u64 > PREVIEW_LIMIT;
    if truncated {
        bytes.truncate(PREVIEW_LIMIT as usize);
    }
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(e) if truncated && e.error_len().is_none() => {
            std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap()
        }
        Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
    };
    Ok((text.to_owned(), truncated))
}

#[cfg(test)]
mod tests;
