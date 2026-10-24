//! A serial dispatch queue owns callbacks; teardown drains it before freeing context.
use super::classify;
use crate::watch::{Changes, Events};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    io,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};

type CfRef = *const c_void;
type Stream = *mut c_void;
type Queue = *mut c_void;
type Callback =
    unsafe extern "C" fn(Stream, *mut c_void, usize, *mut c_void, *const u32, *const u64);

#[repr(C)]
struct StreamContext {
    version: isize,
    info: *mut c_void,
    retain: Option<unsafe extern "C" fn(CfRef) -> CfRef>,
    release: Option<unsafe extern "C" fn(CfRef)>,
    description: Option<unsafe extern "C" fn(CfRef) -> CfRef>,
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn FSEventStreamCreate(
        allocator: CfRef,
        callback: Callback,
        context: *const StreamContext,
        paths: CfRef,
        since: u64,
        latency: f64,
        flags: u32,
    ) -> Stream;
    fn FSEventStreamSetDispatchQueue(stream: Stream, queue: Queue);
    fn FSEventStreamStart(stream: Stream) -> u8;
    fn FSEventStreamStop(stream: Stream);
    fn FSEventStreamInvalidate(stream: Stream);
    fn FSEventStreamRelease(stream: Stream);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithFileSystemRepresentation(allocator: CfRef, path: *const c_char) -> CfRef;
    fn CFArrayCreate(
        allocator: CfRef,
        values: *const CfRef,
        count: isize,
        callbacks: CfRef,
    ) -> CfRef;
    fn CFRelease(value: CfRef);
}

#[link(name = "System")]
unsafe extern "C" {
    fn dispatch_queue_create(label: *const c_char, attributes: CfRef) -> Queue;
    fn dispatch_sync_f(queue: Queue, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_release(queue: Queue);
}

struct Context {
    root: PathBuf,
    events: Events,
}

pub struct Watcher {
    stream: Stream,
    queue: Queue,
    started: bool,
    context: Box<Context>,
    // With NULL CFArray callbacks, keep its string alive explicitly as well.
    paths: CfRef,
    path: CfRef,
}

impl Watcher {
    pub fn new(dir: &Path) -> io::Result<(Self, Events)> {
        let root = std::fs::canonicalize(dir)?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "Space 경로가 폴더가 아닙니다",
            ));
        }
        let path = CString::new(root.as_os_str().as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "감시 경로에 NUL이 있습니다")
        })?;
        let events = Events::default();
        let mut watcher = Self {
            stream: null_mut(),
            queue: null_mut(),
            started: false,
            context: Box::new(Context {
                root,
                events: events.clone(),
            }),
            paths: null(),
            path: null(),
        };
        // SAFETY: path is NUL-terminated; all created CF objects/queue are retained
        // until Drop, including partial-construction failures below.
        unsafe {
            watcher.path = CFStringCreateWithFileSystemRepresentation(null(), path.as_ptr());
            if watcher.path.is_null() {
                return Err(io::Error::other("FSEvents 경로를 만들지 못했습니다"));
            }
            watcher.paths = CFArrayCreate(null(), &watcher.path, 1, null());
            if watcher.paths.is_null() {
                return Err(io::Error::other("FSEvents 경로 목록을 만들지 못했습니다"));
            }
            watcher.queue = dispatch_queue_create(c"edot-space-watch".as_ptr(), null());
            if watcher.queue.is_null() {
                return Err(io::Error::other("FSEvents 큐를 만들지 못했습니다"));
            }
            let context = StreamContext {
                version: 0,
                info: (&mut *watcher.context as *mut Context).cast(),
                retain: None,
                release: None,
                description: None,
            };
            // SinceNow + NoDefer | WatchRoot | FileEvents. Do not ignore our own writes.
            watcher.stream = FSEventStreamCreate(
                null(),
                callback,
                &context,
                watcher.paths,
                u64::MAX,
                0.1,
                0x02 | 0x04 | 0x10,
            );
            if watcher.stream.is_null() {
                return Err(io::Error::other("FSEvents 감시를 만들지 못했습니다"));
            }
            FSEventStreamSetDispatchQueue(watcher.stream, watcher.queue);
            if FSEventStreamStart(watcher.stream) == 0 {
                return Err(io::Error::other("FSEvents 감시를 시작하지 못했습니다"));
            }
            watcher.started = true;
        }
        // The native stream is active before reconciliation covers startup races.
        events.send(Ok(Changes::all()));
        Ok((watcher, events))
    }
}

extern "C" fn drained(_: *mut c_void) {}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.context.events.close();
        // SAFETY: Drop runs outside the callback/serial queue. Stop and invalidate
        // prevent new callbacks, then the queue barrier waits for all in-flight
        // callbacks before releasing the stream or its Box<Context>.
        unsafe {
            if !self.stream.is_null() {
                if self.started {
                    FSEventStreamStop(self.stream);
                }
                FSEventStreamInvalidate(self.stream);
                if !self.queue.is_null() {
                    dispatch_sync_f(self.queue, null_mut(), drained);
                }
                FSEventStreamRelease(self.stream);
            }
            if !self.queue.is_null() {
                dispatch_release(self.queue);
            }
            if !self.paths.is_null() {
                CFRelease(self.paths);
            }
            if !self.path.is_null() {
                CFRelease(self.path);
            }
        }
    }
}

unsafe extern "C" fn callback(
    _: Stream,
    info: *mut c_void,
    count: usize,
    paths: *mut c_void,
    flags: *const u32,
    _: *const u64,
) {
    // SAFETY: FSEvents supplies the registered context for the stream's lifetime.
    // Drop drains the serial queue before freeing it. Callback borrows are shared.
    let context = unsafe { &*info.cast::<Context>() };
    if context.events.0.lock().unwrap().closed {
        return;
    }
    if count == 0 {
        return;
    }
    if paths.is_null() || flags.is_null() {
        context.events.send(Ok(Changes::all()));
        return;
    }
    // SAFETY: without UseCFTypes, FSEvents supplies count C-string pointers and flags.
    let paths = unsafe { std::slice::from_raw_parts(paths.cast::<*const c_char>(), count) };
    let flags = unsafe { std::slice::from_raw_parts(flags, count) };
    let mut changes = Changes::default();
    for (&path, &flags) in paths.iter().zip(flags) {
        if path.is_null() {
            changes.merge(Changes::all());
            continue;
        }
        // SAFETY: each non-null native path is NUL-terminated for this callback.
        let path = unsafe { CStr::from_ptr(path) };
        let path = Path::new(std::ffi::OsStr::from_bytes(path.to_bytes()));
        match classify(&context.root, path, flags) {
            Ok(change) => changes.merge(change),
            Err(error) => {
                context.events.send(Err(error));
                // End the receiver so existing app logic drops and restarts this stream.
                context.events.close();
                return;
            }
        }
    }
    context.events.send(Ok(changes));
}
