//! Three inotify directory watches, with a socket to interrupt an idle poll on drop.
use super::{Changes, Events};
use std::{
    ffi::CString,
    io,
    net::Shutdown,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, net::UnixStream},
    },
    path::Path,
    thread::{self, JoinHandle},
};

pub struct Watcher {
    stop: UnixStream,
    thread: Option<JoinHandle<()>>,
    events: Events,
}

struct Watches {
    root: i32,
    notes: Option<i32>,
    metadata: Option<i32>,
}

fn add(fd: &OwnedFd, path: &Path) -> io::Result<i32> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "감시 경로에 NUL이 있습니다"))?;
    // SAFETY: path is NUL-terminated and fd owns a live inotify instance.
    let watch = unsafe {
        libc::inotify_add_watch(
            fd.as_raw_fd(),
            path.as_ptr(),
            libc::IN_ONLYDIR
                | libc::IN_MODIFY
                | libc::IN_CLOSE_WRITE
                | libc::IN_ATTRIB
                | libc::IN_CREATE
                | libc::IN_DELETE
                | libc::IN_MOVED_FROM
                | libc::IN_MOVED_TO
                | libc::IN_DELETE_SELF
                | libc::IN_MOVE_SELF
                | libc::IN_UNMOUNT,
        )
    };
    if watch < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(watch)
    }
}

fn optional(fd: &OwnedFd, path: &Path) -> io::Result<Option<i32>> {
    match add(fd, path) {
        Ok(watch) => Ok(Some(watch)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

impl Watcher {
    pub fn new(dir: &Path) -> io::Result<(Self, Events)> {
        // SAFETY: no pointers; a successful descriptor is immediately owned below.
        let raw = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: raw is a fresh descriptor returned by inotify_init1.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        // Watch the root first so replacement of either child cannot race startup.
        let watches = Watches {
            root: add(&fd, dir)?,
            notes: optional(&fd, &dir.join("notes"))?,
            metadata: optional(&fd, &dir.join(".edot"))?,
        };
        let (stop, receiver) = UnixStream::pair()?;
        let events = Events::default();
        let worker_events = events.clone();
        let thread = thread::Builder::new()
            .name("edot-space-watch".into())
            .spawn(move || {
                // All watches are installed before initial reconciliation.
                worker_events.send(Ok(Changes::all()));
                if let Err(error) = run(fd, receiver, watches, &worker_events) {
                    worker_events.send(Err(error));
                }
                worker_events.close();
            })?;
        Ok((
            Self {
                stop,
                thread: Some(thread),
                events: events.clone(),
            },
            events,
        ))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.events.close();
        // EOF wakes poll even when the filesystem has been completely idle.
        let _ = self.stop.shutdown(Shutdown::Both);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(fd: OwnedFd, stop: UnixStream, watches: Watches, events: &Events) -> io::Result<()> {
    let mut polls = [
        libc::pollfd {
            fd: stop.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        // SAFETY: polls points to two initialized pollfd values for owned descriptors.
        let ready = unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as _, -1) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if polls[0].revents != 0 {
            return Ok(());
        }
        if polls[1].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::other("inotify 감시가 중단되었습니다"));
        }
        if polls[1].revents & libc::POLLIN == 0 {
            continue;
        }
        loop {
            // A continuous event flood must not delay Space switching/shutdown.
            if events.0.lock().unwrap().closed {
                return Ok(());
            }
            // SAFETY: the byte buffer is writable and fd is nonblocking and owned.
            let count =
                unsafe { libc::read(fd.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
            if count < 0 {
                let error = io::Error::last_os_error();
                match error.kind() {
                    io::ErrorKind::WouldBlock => break,
                    io::ErrorKind::Interrupted => continue,
                    _ => return Err(error),
                }
            }
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "inotify 응답이 종료되었습니다",
                ));
            }
            events.send(Ok(parse(&buffer[..count as usize], &watches)?));
        }
    }
}

fn parse(mut bytes: &[u8], watches: &Watches) -> io::Result<Changes> {
    let mut changes = Changes::default();
    while !bytes.is_empty() {
        let header = bytes
            .get(..16)
            .ok_or_else(|| io::Error::other("잘못된 inotify 알림"))?;
        let watch = i32::from_ne_bytes(header[..4].try_into().unwrap());
        let mask = u32::from_ne_bytes(header[4..8].try_into().unwrap());
        let length = u32::from_ne_bytes(header[12..16].try_into().unwrap()) as usize;
        let end = 16usize
            .checked_add(length)
            .ok_or_else(|| io::Error::other("잘못된 inotify 길이"))?;
        let name = bytes
            .get(16..end)
            .ok_or_else(|| io::Error::other("잘린 inotify 알림"))?;
        if mask & libc::IN_Q_OVERFLOW != 0 {
            // Recreate the watches as well as rescanning: directory events may be lost.
            return Err(io::Error::other("파일 감시 알림 초과 · 전체 재검사"));
        }
        if watch == watches.root || Some(watch) == watches.notes || Some(watch) == watches.metadata
        {
            if mask
                & (libc::IN_IGNORED | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF | libc::IN_UNMOUNT)
                != 0
            {
                return Err(io::Error::other(
                    "감시 폴더가 이동·삭제되었습니다 · 감시 재시작",
                ));
            }
            let name = name.split(|byte| *byte == 0).next().unwrap_or_default();
            if let Ok(name) = std::str::from_utf8(name) {
                if watch == watches.root {
                    if matches!(name, "notes" | ".edot")
                        && mask
                            & (libc::IN_CREATE
                                | libc::IN_DELETE
                                | libc::IN_MOVED_FROM
                                | libc::IN_MOVED_TO)
                            != 0
                    {
                        return Err(io::Error::other("노트 폴더가 교체되었습니다 · 감시 재시작"));
                    }
                } else if Some(watch) == watches.notes {
                    changes.relative_path(&format!("notes/{name}"));
                } else if Some(watch) == watches.metadata {
                    changes.relative_path(&format!(".edot/{name}"));
                }
            }
        }
        bytes = &bytes[end..];
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loss_invalidated_watches_and_malformed_records_require_restart() {
        let watches = Watches {
            root: 1,
            notes: Some(2),
            metadata: Some(3),
        };
        let record = |watch: i32, mask: u32, length: u32| {
            [
                watch.to_ne_bytes(),
                mask.to_ne_bytes(),
                0u32.to_ne_bytes(),
                length.to_ne_bytes(),
            ]
            .concat()
        };
        assert!(parse(&[0; 15], &watches).is_err());
        assert!(parse(&record(2, libc::IN_MODIFY, 4096), &watches).is_err());
        assert!(parse(&record(-1, libc::IN_Q_OVERFLOW, 0), &watches).is_err());
        for mask in [
            libc::IN_IGNORED,
            libc::IN_MOVE_SELF,
            libc::IN_DELETE_SELF,
            libc::IN_UNMOUNT,
        ] {
            assert!(parse(&record(2, mask, 0), &watches).is_err());
        }
        assert!(
            parse(&record(99, libc::IN_IGNORED, 0), &watches)
                .unwrap()
                .is_empty()
        );
    }
}
