//! Vault directory notifications. Only tree.tsv and numeric note files are relevant.
use std::{
    collections::HashSet,
    future::poll_fn,
    io,
    path::Path,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
};

#[derive(Default, Debug, Clone)]
pub struct Changes {
    pub tree: bool,
    pub rescan: bool,
    pub notes: HashSet<u64>,
}

impl Changes {
    pub fn all() -> Self {
        Self {
            rescan: true,
            ..Self::default()
        }
    }
    pub fn is_empty(&self) -> bool {
        !self.tree && !self.rescan && self.notes.is_empty()
    }
    pub fn merge(&mut self, other: Self) {
        self.tree |= other.tree;
        self.rescan |= other.rescan;
        if !self.rescan {
            self.notes.extend(other.notes);
            if self.notes.len() > 4096 {
                self.rescan = true;
            }
        }
        if self.rescan {
            self.notes.clear();
        }
    }
    fn path(&mut self, name: &str) {
        if self.rescan {
            return;
        }
        let name = name.replace('\\', "/").to_ascii_lowercase();
        if name == "tree.tsv" {
            self.tree = true;
        } else if name == "notes" {
            self.rescan = true;
        } else if let Some(name) = name.strip_prefix("notes/")
            && let Some(id) = name
                .strip_suffix(".txt")
                .and_then(|s| s.parse::<u64>().ok())
            && id != 0
        {
            self.merge(Self {
                notes: HashSet::from([id]),
                ..Self::default()
            });
        }
    }
}

#[derive(Default)]
struct State {
    changes: Changes,
    error: Option<io::Error>,
    closed: bool,
    waker: Option<Waker>,
}

#[derive(Clone, Default)]
pub struct Events(Arc<Mutex<State>>);

impl Events {
    fn send(&self, result: io::Result<Changes>) {
        let waker = {
            let mut state = self.0.lock().unwrap();
            if state.closed {
                return;
            }
            match result {
                Ok(changes) => state.changes.merge(changes),
                Err(error) => state.error = Some(error),
            }
            if state.changes.is_empty() && state.error.is_none() {
                None
            } else {
                state.waker.take()
            }
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    fn close(&self) {
        let waker = {
            let mut state = self.0.lock().unwrap();
            state.closed = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    pub fn take(&self) -> Option<io::Result<Changes>> {
        let mut state = self.0.lock().unwrap();
        if let Some(error) = state.error.take() {
            Some(Err(error))
        } else if !state.changes.is_empty() {
            Some(Ok(std::mem::take(&mut state.changes)))
        } else {
            None
        }
    }
    pub async fn next(&self) -> Option<io::Result<Changes>> {
        poll_fn(|cx| {
            let mut state = self.0.lock().unwrap();
            if let Some(error) = state.error.take() {
                Poll::Ready(Some(Err(error)))
            } else if !state.changes.is_empty() {
                Poll::Ready(Some(Ok(std::mem::take(&mut state.changes))))
            } else if state.closed {
                Poll::Ready(None)
            } else {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

#[cfg(windows)]
pub use native::Watcher;

#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        fs::OpenOptions,
        os::windows::{
            fs::OpenOptionsExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        thread::{self, JoinHandle},
    };
    use windows_sys::Win32::{
        Foundation::{ERROR_NOTIFY_ENUM_DIR, WAIT_OBJECT_0},
        Storage::FileSystem::{
            FILE_ACTION_MODIFIED, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
            FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
            FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, ReadDirectoryChangesW,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Threading::{CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects},
        },
    };

    pub struct Watcher {
        stop: Arc<OwnedHandle>,
        thread: Option<JoinHandle<()>>,
        events: Events,
    }

    fn event() -> io::Result<OwnedHandle> {
        // SAFETY: unnamed manual-reset event, owned until all users have stopped.
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateEventW returned a fresh handle requiring CloseHandle.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    impl Watcher {
        pub fn new(dir: &Path) -> io::Result<(Self, Events)> {
            let directory = OpenOptions::new()
                .access_mode(FILE_LIST_DIRECTORY)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
                .open(dir)?;
            let stop = Arc::new(event()?);
            let done = event()?;
            let events = Events::default();
            let worker_stop = stop.clone();
            let worker_events = events.clone();
            let thread = thread::Builder::new()
                .name("edot-vault-watch".into())
                .spawn(move || {
                    let result = run(directory, worker_stop, done, &worker_events);
                    if let Err(error) = result {
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
            // Close/wake on the owner's thread before canceling native I/O.
            self.events.close();
            // SAFETY: the shared stop event outlives the worker and this call.
            unsafe {
                SetEvent(self.stop.as_raw_handle());
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn run(
        directory: std::fs::File,
        stop: Arc<OwnedHandle>,
        done: OwnedHandle,
        events: &Events,
    ) -> io::Result<()> {
        let handle = directory.as_raw_handle();
        let waits = [stop.as_raw_handle(), done.as_raw_handle()];
        let mut buffer = vec![0u32; 16 * 1024]; // DWORD-aligned, 64 KiB.
        let mut overlapped = OVERLAPPED {
            hEvent: done.as_raw_handle(),
            ..OVERLAPPED::default()
        };
        // Establish the watch before the initial reconciliation to cover startup races.
        let mut first = true;
        loop {
            // SAFETY: handles, buffer and OVERLAPPED stay alive until I/O completes.
            let queued = unsafe {
                ResetEvent(done.as_raw_handle());
                ReadDirectoryChangesW(
                    handle,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * 4) as u32,
                    1,
                    FILE_NOTIFY_CHANGE_FILE_NAME
                        | FILE_NOTIFY_CHANGE_DIR_NAME
                        | FILE_NOTIFY_CHANGE_LAST_WRITE
                        | FILE_NOTIFY_CHANGE_SIZE,
                    std::ptr::null_mut(),
                    &mut overlapped,
                    None,
                )
            };
            if queued == 0 {
                return Err(io::Error::last_os_error());
            }
            if first {
                events.send(Ok(Changes::all()));
                first = false;
            }
            // SAFETY: both event handles are valid throughout this wait.
            let wait = unsafe { WaitForMultipleObjects(2, waits.as_ptr(), 0, INFINITE) };
            if wait != WAIT_OBJECT_0 + 1 {
                let error = io::Error::last_os_error();
                // SAFETY: cancel and drain before dropping the buffer/OVERLAPPED.
                unsafe {
                    CancelIoEx(handle, &overlapped);
                    let mut bytes = 0;
                    GetOverlappedResult(handle, &overlapped, &mut bytes, 1);
                }
                return if wait == WAIT_OBJECT_0 {
                    Ok(())
                } else {
                    Err(error)
                };
            }
            let mut bytes = 0;
            // SAFETY: completion was signaled; the output buffer is now readable.
            let success = unsafe { GetOverlappedResult(handle, &overlapped, &mut bytes, 0) };
            if success == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NOTIFY_ENUM_DIR as i32) {
                    events.send(Ok(Changes::all()));
                    continue;
                }
                return Err(error);
            }
            events.send(Ok(if bytes == 0 {
                Changes::all()
            } else {
                // SAFETY: the API initialized exactly `bytes` bytes in the buffer.
                parse(unsafe {
                    std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), bytes as usize)
                })
            }));
        }
    }

    fn parse(bytes: &[u8]) -> Changes {
        let mut changes = Changes::default();
        let mut offset = 0;
        loop {
            let Some(header) = bytes.get(offset..offset + 12) else {
                return Changes::all();
            };
            let next = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
            let len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
            let Some(name) = bytes.get(offset + 12..offset + 12 + len) else {
                return Changes::all();
            };
            if !len.is_multiple_of(2) {
                return Changes::all();
            }
            let wide: Vec<_> = name
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect();
            let name = String::from_utf16_lossy(&wide);
            let action = u32::from_le_bytes(header[4..8].try_into().unwrap());
            // Creating a child also touches the notes directory's timestamp.
            // The child's own notification is sufficient for that case.
            if !name.eq_ignore_ascii_case("notes") || action != FILE_ACTION_MODIFIED {
                changes.path(&name);
            }
            if next == 0 {
                break;
            }
            if next < 12 + len {
                return Changes::all();
            }
            offset += next;
        }
        changes
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn native_notifications_cover_replace_rename_delete_and_cancel() {
            use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
            assert!(parse(&[]).rescan);
            assert!(parse(&[0u8; 11]).rescan);
            let dir = std::env::temp_dir().join(format!(
                "edot-native-watch-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(dir.join("notes")).unwrap();
            let (watcher, events) = Watcher::new(&dir).unwrap();
            let wait = |predicate: &dyn Fn(&Changes) -> bool| {
                let start = Instant::now();
                let mut collected = Changes::default();
                loop {
                    if let Some(changes) = events.take() {
                        collected.merge(changes.unwrap());
                    }
                    if predicate(&collected) {
                        break;
                    }
                    assert!(
                        start.elapsed() < Duration::from_secs(5),
                        "missing event: {collected:?}"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
            };
            wait(&|changes| changes.rescan); // Ready: first native read has been queued.
            std::fs::write(dir.join("notes/7.txt"), "created").unwrap();
            wait(&|changes| changes.notes.contains(&7));
            std::fs::write(dir.join("notes/7.txt"), "modified").unwrap();
            wait(&|changes| changes.notes.contains(&7));
            std::fs::write(dir.join("notes/7.tmp"), "replacement").unwrap();
            std::fs::rename(dir.join("notes/7.tmp"), dir.join("notes/7.txt")).unwrap();
            wait(&|changes| changes.notes.contains(&7));
            std::fs::rename(dir.join("notes/7.txt"), dir.join("notes/8.txt")).unwrap();
            wait(&|changes| changes.notes.contains(&7) && changes.notes.contains(&8));
            std::fs::remove_file(dir.join("notes/8.txt")).unwrap();
            wait(&|changes| changes.notes.contains(&8));
            std::fs::write(dir.join("tree.tmp"), "EDOT2\n").unwrap();
            std::fs::rename(dir.join("tree.tmp"), dir.join("tree.tsv")).unwrap();
            wait(&|changes| changes.tree);
            std::thread::sleep(Duration::from_millis(30));
            let _ = events.take();
            std::fs::write(dir.join("session.tsv"), "session").unwrap();
            std::fs::write(dir.join("settings.ini"), "settings").unwrap();
            std::thread::sleep(Duration::from_millis(50));
            assert!(events.take().is_none());
            let start = Instant::now();
            drop(watcher); // Cancels a pending native read, joins and closes handles.
            assert!(start.elapsed() < Duration::from_secs(2));
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_async_receiver_wakes_on_a_native_thread_and_finishes_on_close() {
        use std::{
            future::Future,
            sync::atomic::{AtomicBool, Ordering},
            task::{Context, Wake},
        };
        struct Probe(AtomicBool);
        impl Wake for Probe {
            fn wake(self: Arc<Self>) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let events = Events::default();
        let probe = Arc::new(Probe(AtomicBool::new(false)));
        let waker = Waker::from(probe.clone());
        let mut cx = Context::from_waker(&waker);
        let mut next = Box::pin(events.next());
        assert!(next.as_mut().poll(&mut cx).is_pending());
        let sender = events.clone();
        std::thread::spawn(move || {
            sender.send(Ok(Changes {
                tree: true,
                ..Default::default()
            }))
        })
        .join()
        .unwrap();
        assert!(probe.0.load(Ordering::SeqCst));
        assert!(
            matches!(next.as_mut().poll(&mut cx), Poll::Ready(Some(Ok(changes))) if changes.tree)
        );
        drop(next);
        let mut next = Box::pin(events.next());
        assert!(next.as_mut().poll(&mut cx).is_pending());
        events.close();
        assert!(matches!(next.as_mut().poll(&mut cx), Poll::Ready(None)));
        events.send(Ok(Changes::all()));
        assert!(events.take().is_none());
    }
}
