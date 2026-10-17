use super::*;
use gpui::{ExternalPaths, FileDropEvent, InputEvent as _, Resource};

fn drop_files(
    f: &Fixture,
    cx: &mut TestAppContext,
    paths: &[PathBuf],
    position: gpui::Point<gpui::Pixels>,
) {
    cx.update_window(f.window, |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_event(
            gpui::MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            FileDropEvent::Entered {
                position,
                paths: ExternalPaths(paths.iter().cloned().collect()),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        window.dispatch_event(FileDropEvent::Pending { position }.to_platform_input(), cx);
        let editor = f.view.read(cx).active_editor().unwrap();
        let accepted = paths
            .iter()
            .any(|path| crate::widgets::InputState::supports_image_path(path))
            && editor.read(cx).accepts_image_drop(position, window);
        assert_eq!(editor.read(cx).image_drop_feedback().is_some(), accepted);
        window.dispatch_event(FileDropEvent::Submit { position }.to_platform_input(), cx);
        window.dispatch_event(FileDropEvent::Ended.to_platform_input(), cx);
        assert!(editor.read(cx).image_drop_feedback().is_none());
    })
    .unwrap();
}

#[gpui::test]
fn pasted_images_replace_selection_follow_edits_and_undo(cx: &mut TestAppContext) {
    use gpui::{ClipboardItem, Image, ImageFormat};
    let f = Fixture::new(cx);
    let png = fs::read("assets/app-icon/edot.png").unwrap();
    let item = ClipboardItem::new_image(&Image::from_bytes(ImageFormat::Png, png.clone()));
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| {
            state.set_value("AB🙂CD", window, cx);
            state.set_selected_range(2..6, cx);
            state.focus(window, cx);
        });
        cx.write_to_clipboard(item.clone());
        editor
    });
    f.press(cx, "ctrl-v");
    // The copy completes asynchronously: retain the selection until success and
    // follow intervening edits instead of replacing the new cursor's selection.
    f.update(cx, |_, window, cx| {
        assert_eq!(editor.read(cx).value(), "AB🙂CD");
        editor.update(cx, |state, cx| {
            state.set_selected_range(0..0, cx);
            state.replace("앞", window, cx);
        });
    });
    cx.run_until_parked();
    let tag = "![clipboard-image](<images/clipboard-image.png>)";
    let expected = format!("앞AB{tag}CD");
    f.update(cx, |this, _, cx| {
        assert_eq!(editor.read(cx).value(), expected);
        assert!(this.save_all(cx));
    });
    assert_eq!(
        fs::read(f.dir.join("images/clipboard-image.png")).unwrap(),
        png
    );
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[0]))).unwrap(),
        expected
    );
    f.press(cx, "ctrl-z");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "앞AB🙂CD")
    });
    f.press(cx, "ctrl-y");
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), expected));
    f.press(cx, "ctrl-v");
    cx.run_until_parked();
    assert_eq!(
        fs::read(f.dir.join("images/clipboard-image-2.png")).unwrap(),
        png
    );

    // A failed import must leave the selected text and existing images intact.
    f.update(cx, |_, window, cx| {
        editor.update(cx, |state, cx| state.select_all(window, cx));
        cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Png,
            b"invalid".to_vec(),
        )));
    });
    let before = f.update(cx, |_, _, cx| editor.read(cx).value().to_string());
    f.press(cx, "ctrl-v");
    cx.run_until_parked();
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), before));
    assert_eq!(fs::read_dir(f.dir.join("images")).unwrap().count(), 2);
}

#[gpui::test]
fn pasted_image_files_and_text_respect_editor_mode(cx: &mut TestAppContext) {
    use crate::widgets::TextMode;
    use gpui::{ClipboardEntry, ClipboardItem, Image, ImageFormat};
    let f = Fixture::new(cx);
    let png = fs::read("assets/app-icon/edot.png").unwrap();
    let image = ClipboardItem::new_image(&Image::from_bytes(ImageFormat::Png, png.clone()));
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| {
            state.set_value("keep", window, cx);
            state.select_all(window, cx);
            state.focus(window, cx);
            state.set_text_mode(TextMode::PlainText, cx);
        });
        cx.write_to_clipboard(image.clone());
        editor
    });
    f.press(cx, "ctrl-v");
    cx.run_until_parked();
    assert!(!f.dir.join("images").exists());
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "keep");
        editor.update(cx, |state, cx| {
            state.set_text_mode(TextMode::Markdown, cx);
            state.set_read_only(true, cx);
        });
    });
    f.press(cx, "ctrl-v");
    cx.run_until_parked();
    assert!(!f.dir.join("images").exists());

    let sources = f.dir.join("paste-sources");
    fs::create_dir_all(&sources).unwrap();
    let first = sources.join("사진.png");
    let second = sources.join("second.png");
    fs::write(&first, &png).unwrap();
    fs::write(&second, &png).unwrap();
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| state.set_read_only(false, cx));
        cx.write_to_clipboard(
            ClipboardEntry::ExternalPaths(ExternalPaths(
                vec![first.clone(), second.clone()].into(),
            ))
            .into(),
        );
    });
    f.press(cx, "ctrl-v");
    cx.run_until_parked();
    let tags = "![사진](<images/사진.png>)\n![second](<images/second.png>)";
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), tags));
    assert_eq!(fs::read(f.dir.join("images/사진.png")).unwrap(), png);
    assert_eq!(fs::read(&first).unwrap(), png);
    f.press(cx, "ctrl-z");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "keep");
        cx.write_to_clipboard(ClipboardItem::new_string("ordinary text".into()));
    });
    f.press(cx, "ctrl-v");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "ordinary text")
    });
}

// A real COM IDataObject exercises the same formats supplied by browser OLE
// drags without sending input to a user's browser or desktop.
mod native {
    use windows::{
        Win32::{
            Foundation::{E_NOTIMPL, S_OK},
            System::{
                Com::*,
                DataExchange::RegisterClipboardFormatW,
                Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
            },
        },
        core::{BOOL, HRESULT, Ref, Result, implement, w},
    };
    #[implement(IDataObject)]
    struct Data {
        id: u16,
        bytes: Vec<u8>,
    }
    impl IDataObject_Impl for Data_Impl {
        fn GetData(&self, format: *const FORMATETC) -> Result<STGMEDIUM> {
            unsafe {
                if (*format).cfFormat != self.id || (*format).tymed & TYMED_HGLOBAL.0 as u32 == 0 {
                    return Err(E_NOTIMPL.into());
                }
                let handle = GlobalAlloc(GMEM_MOVEABLE, self.bytes.len())?;
                let pointer = GlobalLock(handle);
                assert!(!pointer.is_null());
                std::ptr::copy_nonoverlapping(
                    self.bytes.as_ptr(),
                    pointer.cast(),
                    self.bytes.len(),
                );
                let _ = GlobalUnlock(handle);
                Ok(STGMEDIUM {
                    tymed: TYMED_HGLOBAL.0 as u32,
                    u: STGMEDIUM_0 { hGlobal: handle },
                    ..Default::default()
                })
            }
        }
        fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
            unsafe {
                if (*format).cfFormat == self.id {
                    S_OK
                } else {
                    E_NOTIMPL
                }
            }
        }
        fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, _: *mut FORMATETC) -> HRESULT {
            E_NOTIMPL
        }
        fn SetData(&self, _: *const FORMATETC, _: *const STGMEDIUM, _: BOOL) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn EnumFormatEtc(&self, _: u32) -> Result<IEnumFORMATETC> {
            Err(E_NOTIMPL.into())
        }
        fn DAdvise(&self, _: *const FORMATETC, _: u32, _: Ref<IAdviseSink>) -> Result<u32> {
            Err(E_NOTIMPL.into())
        }
        fn DUnadvise(&self, _: u32) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
            Err(E_NOTIMPL.into())
        }
    }
    pub fn html(html: &str) -> IDataObject {
        Data {
            id: unsafe { RegisterClipboardFormatW(w!("HTML Format")) as u16 },
            bytes: html.bytes().chain(Some(0)).collect(),
        }
        .into()
    }
    pub fn url(url: &str) -> IDataObject {
        Data {
            id: unsafe { RegisterClipboardFormatW(w!("UniformResourceLocatorW")) as u16 },
            bytes: url
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_ne_bytes)
                .collect(),
        }
        .into()
    }
    pub fn files(paths: &[std::path::PathBuf]) -> IDataObject {
        let mut bytes = Vec::new();
        // DROPFILES with UTF-16 paths, terminated by two NULs.
        for value in [20u32, 0, 0, 0, 1] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for path in paths {
            bytes.extend(
                path.to_str()
                    .unwrap()
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes),
            );
        }
        bytes.extend_from_slice(&[0, 0]);
        Data { id: 15, bytes }.into()
    }
}

#[gpui::test]
fn browser_image_drops_show_positions_and_import_downloaded_images(cx: &mut TestAppContext) {
    use crate::desktop::image_drop;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    use windows::Win32::{
        Foundation::POINTL,
        System::{
            Ole::{DROPEFFECT_COPY, DROPEFFECT_NONE},
            SystemServices::MODIFIERKEYS_FLAGS,
        },
    };
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        let executor = cx.background_executor().clone();
        cx.set_http_client(std::sync::Arc::new(
            crate::image_http::ImageHttpClient::new(executor),
        ));
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| {
            state.set_value("AB🙂CD\n", window, cx);
            state.select_all(window, cx);
        });
        editor
    });
    let (target, p, later) = cx
        .update_window(f.window, |_, window, cx| {
            window.render_frame(cx);
            let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
            let a = handler.bounds_for_range(2..4, window, cx).unwrap();
            let b = handler.bounds_for_range(4..5, window, cx).unwrap();
            (
                image_drop::test_target(&f.view, window, cx),
                POINTL {
                    x: f32::from(a.left()).ceil() as i32,
                    y: f32::from(a.center().y) as i32,
                },
                POINTL {
                    x: f32::from(b.left()).ceil() as i32,
                    y: f32::from(b.center().y) as i32,
                },
            )
        })
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/image?query=1&size=2",
        listener.local_addr().unwrap()
    );
    let png = fs::read("assets/app-icon/edot.png").unwrap();
    let expected = png.clone();
    let server = std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "browser image request never arrived"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(error) => panic!("{error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut buffer = [0; 4096];
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
        }
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            png.len()
        )
        .unwrap();
        stream.write_all(&png).unwrap();
    });
    let html = format!(
        "SourceURL:https://example.com/page\r\n<img src='https://outside.example/image.png'><!--StartFragment--><a href='https://example.com'><IMG SRC=\"{}\"></a><!--EndFragment-->",
        url.replace('&', "&amp;")
    );
    assert_eq!(image_drop::image_urls(&html), vec![url.clone()]);
    assert_eq!(
        image_drop::image_urls("SourceURL:https://example.com/page\r\n<img src='../img/a.png'>"),
        vec!["https://example.com/img/a.png"]
    );
    let data = native::html(&html);
    let mut effect = DROPEFFECT_COPY;
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| {
            state.set_text_mode(crate::widgets::TextMode::PlainText, cx)
        })
    });
    cx.update_window(f.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    for data in [&data, &native::url(&url)] {
        unsafe {
            target
                .DragEnter(data, MODIFIERKEYS_FLAGS(0), p, &mut effect)
                .unwrap();
            target
                .Drop(data, MODIFIERKEYS_FLAGS(0), p, &mut effect)
                .unwrap();
        }
        cx.run_until_parked();
        assert!(!f.dir.join("images").exists());
        f.update(cx, |this, _, cx| {
            assert!(this.error.is_none());
            assert_eq!(editor.read(cx).value(), "AB🙂CD\n");
            assert_eq!(editor.read(cx).selected_range(), 0..9);
            assert!(editor.read(cx).image_drop_feedback().is_none());
        });
    }
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| {
            state.set_text_mode(crate::widgets::TextMode::Markdown, cx)
        })
    });
    cx.update_window(f.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    unsafe {
        target
            .DragEnter(&data, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
    }
    assert_eq!(effect, DROPEFFECT_COPY);
    cx.run_until_parked();
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).image_drop_feedback(), Some(2));
        assert_eq!(editor.read(cx).selected_range(), 0..9);
    });
    cx.update_window(f.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    unsafe {
        target
            .DragOver(MODIFIERKEYS_FLAGS(0), later, &mut effect)
            .unwrap();
    }
    cx.run_until_parked();
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).image_drop_feedback(), Some(6))
    });
    unsafe {
        target
            .DragOver(MODIFIERKEYS_FLAGS(0), POINTL { x: 40, y: p.y }, &mut effect)
            .unwrap();
    }
    cx.run_until_parked();
    f.update(cx, |_, _, cx| {
        assert!(editor.read(cx).image_drop_feedback().is_none())
    });
    unsafe {
        target
            .DragOver(MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
        target.DragLeave().unwrap();
    }
    cx.run_until_parked();
    f.update(cx, |_, _, cx| {
        assert!(editor.read(cx).image_drop_feedback().is_none())
    });
    // URL-only browser formats use the same import path as HTML. Query URLs have
    // no extension; the saved type must be inferred from the downloaded bytes.
    let data = native::url(&url);
    unsafe {
        target
            .DragEnter(&data, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
        target
            .Drop(&data, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
    }
    cx.run_until_parked();
    server.join().unwrap();
    f.update(cx, |_, _, cx| {
        assert_eq!(
            editor.read(cx).value(),
            "AB![web-image](<images/web-image.png>)🙂CD\n"
        );
        assert!(editor.read(cx).image_drop_feedback().is_none());
    });
    f.press(cx, "ctrl-z");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "AB🙂CD\n");
    });
    assert_eq!(
        fs::read(f.dir.join("images/web-image.png")).unwrap(),
        expected
    );
    // Embedded browser thumbnails work too; invalid HTTP/content never inserts
    // a tag or leaves a partially copied batch behind.
    let data = native::html(
        "<img src='data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAICRAEAOw=='>",
    );
    unsafe {
        target
            .DragEnter(&data, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
        target
            .Drop(&data, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
    }
    cx.run_until_parked();
    assert!(f.dir.join("images/web-image.gif").exists());
    let before = f.update(cx, |_, _, cx| editor.read(cx).value());
    let invalid = native::html("<img src='data:image/png;base64,YmFk'>");
    unsafe {
        target
            .DragEnter(&invalid, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
        target
            .Drop(&invalid, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
    }
    cx.run_until_parked();
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), before));
    assert_eq!(fs::read_dir(f.dir.join("images")).unwrap().count(), 2);
    f.update(cx, |this, _, cx| {
        assert!(this.error.is_some());
        this.error = None;
        cx.notify();
    });
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| state.set_read_only(true, cx))
    });
    unsafe {
        target
            .DragEnter(&data, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
        target
            .Drop(&data, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
    }
    cx.run_until_parked();
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), before));
    assert_eq!(fs::read_dir(f.dir.join("images")).unwrap().count(), 2);
    effect = DROPEFFECT_COPY;
    let unsupported = native::url("javascript:alert(1)");
    unsafe {
        target
            .DragEnter(&unsupported, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
        target.DragLeave().unwrap();
    }
    assert_eq!(effect, DROPEFFECT_NONE);
    cx.run_until_parked();
    // Replacing the native OLE target must preserve Explorer file drops too.
    let file = f.dir.join("한글.png");
    fs::write(&file, &expected).unwrap();
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| state.set_read_only(false, cx))
    });
    let data = native::files(&[file]);
    effect = DROPEFFECT_COPY;
    cx.update_window(f.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    unsafe {
        target
            .DragEnter(&data, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
    }
    cx.run_until_parked();
    f.update(cx, |_, _, cx| {
        assert!(editor.read(cx).image_drop_feedback().is_some())
    });
    unsafe {
        target
            .Drop(&data, MODIFIERKEYS_FLAGS(0), p, &mut effect)
            .unwrap();
    }
    cx.run_until_parked();
    assert_eq!(fs::read(f.dir.join("images/한글.png")).unwrap(), expected);
    f.update(cx, |_, _, cx| {
        assert!(
            editor
                .read(cx)
                .value()
                .contains("![한글](<images/한글.png>)")
        );
        assert!(editor.read(cx).image_drop_feedback().is_none());
    });
}

#[gpui::test]
fn image_drops_use_space_paths_preserve_files_and_follow_edits(cx: &mut TestAppContext) {
    let f = Fixture::new(cx);
    let editor = f.update(cx, |this, window, cx| {
        let editor = this.active_editor().unwrap();
        editor.update(cx, |state, cx| state.set_value("AB🙂CD\n", window, cx));
        editor
    });
    let sources = f.dir.join("test-image-sources");
    fs::create_dir_all(&sources).unwrap();
    let first = sources.join("한글 [사진](1).PNG");
    let second = sources.join("second.png");
    let unsupported = sources.join("ignored.txt");
    let png = fs::read("assets/app-icon/edot.png").unwrap();
    fs::write(&first, &png).unwrap();
    fs::write(&second, &png).unwrap();
    fs::write(&unsupported, "text").unwrap();
    let position = cx
        .update_window(f.window, |_, window, cx| {
            window.render_frame(cx);
            let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
            let bounds = handler.bounds_for_range(2..4, window, cx).unwrap();
            point(bounds.left() + px(0.1), bounds.center().y)
        })
        .unwrap();

    // Plain Text, read-only and non-image drops must not create an images directory.
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| {
            state.set_text_mode(crate::widgets::TextMode::PlainText, cx)
        })
    });
    drop_files(&f, cx, std::slice::from_ref(&first), position);
    f.update(cx, |this, window, cx| {
        // The app-side guard also rejects calls that bypass GPUI's drop hitbox.
        this.drop_images(
            editor.clone(),
            std::slice::from_ref(&first),
            position,
            window,
            cx,
        );
    });
    cx.run_until_parked();
    assert!(!f.dir.join("images").exists());
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "AB🙂CD\n");
        editor.update(cx, |state, cx| {
            state.set_text_mode(crate::widgets::TextMode::Markdown, cx)
        });
    });
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| state.set_read_only(true, cx))
    });
    drop_files(&f, cx, std::slice::from_ref(&first), position);
    cx.run_until_parked();
    assert!(!f.dir.join("images").exists());
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| state.set_read_only(false, cx))
    });
    drop_files(&f, cx, std::slice::from_ref(&unsupported), position);
    cx.run_until_parked();
    assert!(!f.dir.join("images").exists());

    fs::create_dir_all(f.dir.join("images")).unwrap();
    let existing = f.dir.join("images/한글 [사진](1).PNG");
    fs::write(&existing, "existing image").unwrap();
    f.update(cx, |_, window, cx| {
        editor.update(cx, |state, cx| state.select_all(window, cx))
    });
    drop_files(
        &f,
        cx,
        &[first.clone(), second.clone(), unsupported],
        position,
    );
    // Change the text before the queued copy starts; the insertion must follow it.
    f.update(cx, |_, window, cx| {
        editor.update(cx, |state, cx| {
            state.set_selected_range(0..0, cx);
            state.replace("앞", window, cx);
        })
    });
    cx.run_until_parked();
    let tags =
        "![한글 \\[사진\\](1)](<images/한글 [사진](1)-2.PNG>)\n![second](<images/second.png>)";
    let expected = format!("앞AB{tags}🙂CD\n");
    f.update(cx, |this, _, cx| {
        assert_eq!(editor.read(cx).value(), expected);
        assert!(
            this.open
                .iter()
                .find(|tab| tab.id == f.ids[0])
                .unwrap()
                .dirty
        );
        assert!(this.save_all(cx));
    });
    assert_eq!(fs::read(&first).unwrap(), png);
    assert_eq!(fs::read(&existing).unwrap(), b"existing image");
    let imported = f.dir.join("images/한글 [사진](1)-2.PNG");
    assert_eq!(fs::read(&imported).unwrap(), png);
    assert_eq!(fs::read(f.dir.join("images/second.png")).unwrap(), png);
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[0]))).unwrap(),
        expected
    );
    f.press(cx, "ctrl-z");
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), "앞AB🙂CD\n")
    });
    assert!(imported.exists());
    f.press(cx, "ctrl-y");
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), expected));

    // The reading screen must request the actual Space/images path, preserving
    // spaces, brackets, Unicode and parentheses from the Markdown destination.
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| state.set_read_only(true, cx));
    });
    cx.update_window(f.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
    cx.update_window(f.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let resource = Resource::Path(imported.clone().into());
    cx.update(|cx| {
        assert!(cx.has_asset::<gpui::ImgResourceLoader>(&resource));
        assert!(
            cx.fetch_asset::<gpui::ImgResourceLoader>(&resource)
                .unwrap()
                .is_ok()
        );
    });

    // Reusing an image already in this Space does not duplicate it. A failed
    // batch cleans up only its new copies and keeps all pre-existing files.
    let image_count = fs::read_dir(f.dir.join("images")).unwrap().count();
    assert!(
        super::super::images::import_images(&f.dir, std::slice::from_ref(&imported))
            .unwrap()
            .contains("images/한글 [사진](1)-2.PNG")
    );
    assert_eq!(
        fs::read_dir(f.dir.join("images")).unwrap().count(),
        image_count
    );
    assert!(
        super::super::images::import_images(&f.dir, &[first, sources.join("missing.png")]).is_err()
    );
    assert_eq!(
        fs::read_dir(f.dir.join("images")).unwrap().count(),
        image_count
    );
    assert!(imported.exists());

    // An IME composition started during copying finishes before tag insertion.
    f.update(cx, |_, window, cx| {
        editor.update(cx, |state, cx| {
            state.set_read_only(false, cx);
            state.set_value("AB", window, cx);
        })
    });
    cx.run_until_parked();
    let position = cx
        .update_window(f.window, |_, window, cx| {
            window.render_frame(cx);
            let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
            let bounds = handler.bounds_for_range(1..2, window, cx).unwrap();
            let position = point(bounds.left() + px(0.1), bounds.center().y);
            assert_eq!(
                handler.character_index_for_point(position, window, cx),
                Some(1),
                "{bounds:?}"
            );
            position
        })
        .unwrap();
    let body = gpui::VisualTestContext::from_window(f.window, cx)
        .debug_bounds("body-editor")
        .unwrap();
    assert!(
        body.contains(&position),
        "body={body:?}, position={position:?}"
    );
    drop_files(&f, cx, std::slice::from_ref(&second), position);
    f.update(cx, |this, _, cx| {
        assert_eq!(
            editor.read(cx).selected_range(),
            1..1,
            "status={} read_only={}",
            this.status,
            editor.read(cx).is_read_only()
        )
    });
    cx.update_window(f.window, |_, window, cx| {
        let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
        handler.replace_and_mark_text_in_range(None, "ㅎ", Some(1..1), window, cx);
    })
    .unwrap();
    cx.run_until_parked();
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), "AㅎB"));
    cx.update_window(f.window, |_, window, cx| {
        let mut handler = ElementInputHandler::new(Bounds::default(), editor.clone());
        handler.replace_text_in_range(None, "한", window, cx);
    })
    .unwrap();
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(50));
    cx.run_until_parked();
    let composed = "A한![second](<images/second-2.png>)B";
    f.update(cx, |_, _, cx| assert_eq!(editor.read(cx).value(), composed));

    // Switching to Plain Text while copying must not insert a Markdown tag.
    drop_files(&f, cx, std::slice::from_ref(&second), position);
    f.update(cx, |_, _, cx| {
        editor.update(cx, |state, cx| {
            state.set_text_mode(crate::widgets::TextMode::PlainText, cx)
        })
    });
    cx.run_until_parked();
    f.update(cx, |_, _, cx| {
        assert_eq!(editor.read(cx).value(), composed);
        editor.update(cx, |state, cx| {
            state.set_text_mode(crate::widgets::TextMode::Markdown, cx)
        });
    });

    // Closing the owning tab while a copy is queued must not edit another note.
    drop_files(&f, cx, std::slice::from_ref(&second), position);
    f.update(cx, |this, window, cx| {
        this.close_tabs(f.ids[0], crate::tabs::CloseMode::Current, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(f.dir.join(format!("notes/{}.txt", f.ids[0]))).unwrap(),
        composed
    );
}
