//! Native OLE drops: Explorer paths and browser image HTML/URLs.
use super::*;
use gpui::InputEvent as _;
use raw_window_handle::RawWindowHandle;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};
use windows::{
    Win32::{
        Foundation::{HWND, POINT, POINTL},
        Graphics::Gdi::ScreenToClient,
        System::{
            Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL},
            DataExchange::RegisterClipboardFormatW,
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
            Ole::{
                DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl,
                RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
            },
            SystemServices::MODIFIERKEYS_FLAGS,
        },
        UI::{
            HiDpi::GetDpiForWindow,
            Shell::{DragQueryFileW, HDROP},
        },
    },
    core::{PCWSTR, Ref, Result as WinResult, implement},
};

#[derive(Default)]
enum Offer {
    Files(Vec<PathBuf>),
    Web(Vec<String>),
    #[default]
    Unsupported,
}
#[derive(Clone, Copy)]
enum Phase {
    Enter,
    Move,
    Leave,
    Drop,
}
struct Event {
    phase: Phase,
    position: Point<Pixels>,
    offer: Rc<Offer>,
}
type Callback = Box<dyn Fn(Event, &mut Window, &mut App)>;

struct Events {
    queue: RefCell<VecDeque<Event>>,
    scheduled: Cell<bool>,
    app: AsyncApp,
    window: AnyWindowHandle,
    callback: Callback,
}
impl Events {
    fn push(self: &Rc<Self>, event: Event) {
        let mut queue = self.queue.borrow_mut();
        // OLE may deliver moves faster than a frame; only their latest position matters.
        if matches!(event.phase, Phase::Move)
            && queue
                .back()
                .is_some_and(|last| matches!(last.phase, Phase::Move))
        {
            *queue.back_mut().unwrap() = event;
        } else {
            queue.push_back(event);
        }
        drop(queue);
        if self.scheduled.replace(true) {
            return;
        }
        let events = self.clone();
        // OLE can invoke us inside GPUI's outgoing DoDragDrop while App is borrowed.
        // Drain later on the UI executor, never reenter App from a COM callback.
        self.app
            .foreground_executor()
            .spawn(async move {
                let mut app = events.app.clone();
                loop {
                    let result = app.update_window(events.window, |_, window, cx| {
                        loop {
                            let event = events.queue.borrow_mut().pop_front();
                            let Some(event) = event else {
                                break;
                            };
                            (events.callback)(event, window, cx);
                        }
                    });
                    if result.is_err_and(|error| error.is::<std::cell::BorrowMutError>()) {
                        app.background_executor()
                            .timer(std::time::Duration::from_millis(4))
                            .await;
                    } else {
                        events.queue.borrow_mut().clear();
                        break;
                    }
                }
                events.scheduled.set(false);
            })
            .detach();
    }
}

#[implement(IDropTarget)]
struct Target {
    hwnd: HWND,
    offer: RefCell<Rc<Offer>>,
    events: Rc<Events>,
}
impl Target {
    fn emit(&self, phase: Phase, pt: Option<&POINTL>, effect: *mut DROPEFFECT) {
        let mut p = pt.map_or(POINT::default(), |p| POINT { x: p.x, y: p.y });
        // SAFETY: This target belongs to this live HWND; OLE supplies screen pixels.
        let scale = unsafe {
            let _ = ScreenToClient(self.hwnd, &mut p);
            GetDpiForWindow(self.hwnd).max(96) as f32 / 96.
        };
        let offer = self.offer.borrow().clone();
        if !effect.is_null() {
            // SAFETY: OLE supplies a writable effect DWORD for this callback.
            unsafe {
                *effect = if matches!(*offer, Offer::Unsupported)
                    || !effect.read().contains(DROPEFFECT_COPY)
                {
                    DROPEFFECT_NONE
                } else {
                    DROPEFFECT_COPY
                };
            }
        }
        self.events.push(Event {
            phase,
            position: point(px(p.x as f32 / scale), px(p.y as f32 / scale)),
            offer,
        });
    }
}
impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> WinResult<()> {
        *self.offer.borrow_mut() = Rc::new(data.as_ref().map(read_offer).unwrap_or_default());
        self.emit(Phase::Enter, Some(pt), effect);
        Ok(())
    }
    fn DragOver(
        &self,
        _: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> WinResult<()> {
        self.emit(Phase::Move, Some(pt), effect);
        Ok(())
    }
    fn DragLeave(&self) -> WinResult<()> {
        self.emit(Phase::Leave, None, std::ptr::null_mut());
        *self.offer.borrow_mut() = Rc::new(Offer::Unsupported);
        Ok(())
    }
    fn Drop(
        &self,
        _: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> WinResult<()> {
        self.emit(Phase::Drop, Some(pt), effect);
        *self.offer.borrow_mut() = Rc::new(Offer::Unsupported);
        Ok(())
    }
}

pub(super) fn install(view: &Entity<Edot>, window: &mut Window, cx: &mut App) -> WinResult<()> {
    let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
        return Ok(());
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Ok(());
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let target = target(view, window, cx, hwnd);
    // SAFETY: OLE is initialized by GPUI. Registration owns a COM reference until
    // the Windows backend revokes it when the window is destroyed.
    unsafe {
        RevokeDragDrop(hwnd)?;
        RegisterDragDrop(hwnd, &target)
    }
}

fn target(view: &Entity<Edot>, window: &Window, cx: &App, hwnd: HWND) -> IDropTarget {
    let view = view.downgrade();
    let events = Rc::new(Events {
        queue: RefCell::new(VecDeque::new()),
        scheduled: Cell::new(false),
        app: cx.to_async(),
        window: window.window_handle(),
        callback: Box::new(move |event, window, cx| {
            match &*event.offer {
                Offer::Files(paths) => {
                    if matches!(event.phase, Phase::Enter) {
                        // Update hit testing before GPUI turns Enter into a pressed move.
                        window.dispatch_event(
                            MouseMoveEvent {
                                position: event.position,
                                pressed_button: None,
                                modifiers: Default::default(),
                            }
                            .to_platform_input(),
                            cx,
                        );
                    }
                    let input = match event.phase {
                        Phase::Enter => FileDropEvent::Entered {
                            position: event.position,
                            paths: ExternalPaths(paths.iter().cloned().collect()),
                        },
                        Phase::Move => FileDropEvent::Pending {
                            position: event.position,
                        },
                        Phase::Leave => FileDropEvent::Exited,
                        Phase::Drop => FileDropEvent::Submit {
                            position: event.position,
                        },
                    };
                    window.dispatch_event(input.to_platform_input(), cx);
                }
                Offer::Web(urls) => {
                    let _ = view.update(cx, |this, cx| {
                        let Some(editor) = this.active_editor() else {
                            return;
                        };
                        let accepted = editor.read(cx).accepts_image_drop(event.position, window);
                        editor.update(cx, |state, cx| {
                            state.show_image_drop(
                                matches!(event.phase, Phase::Enter | Phase::Move)
                                    .then_some(event.position),
                                window,
                                cx,
                            )
                        });
                        if matches!(event.phase, Phase::Drop) && accepted {
                            this.drop_web_images(editor, urls.clone(), event.position, window, cx);
                        }
                    });
                }
                Offer::Unsupported => {}
            }
        }),
    });
    Target {
        hwnd,
        offer: RefCell::new(Rc::new(Offer::Unsupported)),
        events,
    }
    .into()
}

#[cfg(test)]
pub(super) fn test_target(view: &Entity<Edot>, window: &Window, cx: &App) -> IDropTarget {
    target(view, window, cx, HWND::default())
}

fn format(id: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: id,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}
fn read_offer(data: &IDataObject) -> Offer {
    // SAFETY: GetData returns a medium owned by this call; always release it.
    if let Ok(mut medium) = unsafe { data.GetData(&format(15)) } {
        let mut paths = Vec::new();
        unsafe {
            let drop = HDROP(medium.u.hGlobal.0);
            let count = DragQueryFileW(drop, u32::MAX, None).min(10_000);
            for index in 0..count {
                let length = DragQueryFileW(drop, index, None) as usize;
                if length > 32_767 {
                    continue;
                }
                let mut buffer = vec![0; length + 1];
                let read = DragQueryFileW(drop, index, Some(&mut buffer)) as usize;
                paths.push(PathBuf::from(String::from_utf16_lossy(&buffer[..read])));
            }
            ReleaseStgMedium(&mut medium);
        }
        if !paths.is_empty() {
            return Offer::Files(paths);
        }
    }
    if let Some(html) = read_string(data, "HTML Format", false) {
        let urls = image_urls(&html);
        if !urls.is_empty() {
            return Offer::Web(urls);
        }
    }
    for (name, wide) in [
        ("text/x-moz-url", true),
        ("UniformResourceLocatorW", true),
        ("UniformResourceLocator", false),
        ("", true),
    ] {
        if let Some(text) = read_string(data, name, wide)
            && let Some(url) = text
                .lines()
                .next()
                .map(str::trim)
                .filter(|url| supported_url(url))
        {
            return Offer::Web(vec![url.to_owned()]);
        }
    }
    Offer::Unsupported
}
fn read_string(data: &IDataObject, name: &str, wide: bool) -> Option<String> {
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: Name is NUL terminated; format IDs are process-independent.
    let id = if name.len() == 1 {
        13
    } else {
        unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) as u16 }
    };
    let mut medium = unsafe { data.GetData(&format(id)) }.ok()?;
    // SAFETY: FORMATETC requires HGLOBAL. Bound memory and release/unlock all paths.
    unsafe {
        let handle = medium.u.hGlobal;
        let length = GlobalSize(handle);
        let pointer = if length <= 32 * 1024 * 1024 {
            GlobalLock(handle)
        } else {
            std::ptr::null_mut()
        };
        let text = if pointer.is_null() {
            None
        } else if wide {
            let units = std::slice::from_raw_parts(pointer.cast::<u16>(), length / 2);
            Some(String::from_utf16_lossy(
                &units[..units.iter().position(|u| *u == 0).unwrap_or(units.len())],
            ))
        } else {
            let bytes = std::slice::from_raw_parts(pointer.cast::<u8>(), length);
            Some(
                String::from_utf8_lossy(
                    &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())],
                )
                .into_owned(),
            )
        };
        if !pointer.is_null() {
            let _ = GlobalUnlock(handle);
        }
        ReleaseStgMedium(&mut medium);
        text
    }
}

fn supported_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://") || url.starts_with("data:image/")
}
pub(super) fn image_urls(html: &str) -> Vec<String> {
    static IMAGES: std::sync::LazyLock<regex_lite::Regex> = std::sync::LazyLock::new(|| {
        regex_lite::Regex::new(r#"(?is)<img\b[^>]*?\s+src\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#)
            .unwrap()
    });
    let base = html
        .lines()
        .find_map(|line| line.strip_prefix("SourceURL:"))
        .and_then(|url| gpui::http_client::Url::parse(url.trim()).ok());
    let offset = |label: &str| {
        html.lines()
            .find_map(|line| line.strip_prefix(label))?
            .trim()
            .parse::<usize>()
            .ok()
    };
    let fragment = offset("StartFragment:")
        .zip(offset("EndFragment:"))
        .and_then(|(start, end)| html.get(start..end))
        .or_else(|| {
            html.split_once("<!--StartFragment-->")
                .and_then(|(_, rest)| {
                    rest.split_once("<!--EndFragment-->")
                        .map(|(fragment, _)| fragment)
                })
        })
        .unwrap_or(html);
    let mut urls = Vec::new();
    for capture in IMAGES.captures_iter(fragment) {
        let source = capture
            .get(1)
            .or_else(|| capture.get(2))
            .or_else(|| capture.get(3))
            .unwrap()
            .as_str();
        let mut source = source
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&apos;", "'");
        if !supported_url(&source)
            && let Some(base) = &base
            && let Ok(url) = base.join(&source)
        {
            source = url.to_string();
        }
        if supported_url(&source) && !urls.contains(&source) {
            urls.push(source);
        }
        if urls.len() == 100 {
            break;
        }
    }
    urls
}
