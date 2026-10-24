//! Import dropped or pasted images and insert references into the owning note.
use super::*;
use crate::widgets::input::DropInsertion;
use std::{fs::OpenOptions, io::Read, path::Path, time::Duration};

fn import_clipboard_images(space: &Path, images: &[gpui::Image]) -> io::Result<String> {
    let mut created = Vec::new();
    let result = (|| {
        let mut tags = Vec::new();
        for image in images {
            let extension = image_extension(&image.bytes)?;
            let name = copy_image(
                &space.join("images"),
                &format!("clipboard-image.{extension}"),
                &mut image.bytes.as_slice(),
                &mut created,
            )?;
            tags.push(image_tag(&name, &name));
        }
        Ok(tags.join("\n"))
    })();
    if result.is_err() {
        for path in created {
            let _ = fs::remove_file(path);
        }
    }
    result
}

fn copy_image(
    directory: &Path,
    name: &str,
    source: &mut impl Read,
    created: &mut Vec<PathBuf>,
) -> io::Result<String> {
    fs::create_dir_all(directory)?;
    let path = Path::new(name);
    let stem = path.file_stem().unwrap().to_str().unwrap();
    let extension = path.extension().unwrap().to_str().unwrap();
    for number in 1..=10_000 {
        let name = if number == 1 {
            name.to_owned()
        } else {
            format!("{stem}-{number}.{extension}")
        };
        let target = directory.join(&name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(mut destination) => {
                created.push(target);
                io::copy(source, &mut destination)?;
                return Ok(name);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(
        "중복되지 않는 이미지 파일 이름을 만들 수 없습니다",
    ))
}

fn image_tag(name: &str, original: &str) -> String {
    let alt = Path::new(original)
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    format!("![{alt}](<images/{name}>)")
}

fn image_extension(bytes: &[u8]) -> io::Result<&'static str> {
    let extension = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "jpg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "webp"
    } else if bytes.starts_with(b"BM") {
        "bmp"
    } else if std::str::from_utf8(bytes).is_ok_and(|text| {
        let text = text.trim_start_matches('\u{feff}').trim_start();
        (text.starts_with("<svg") || text.starts_with("<?xml") || text.starts_with("<!--"))
            && text.contains("<svg")
            && !text.contains("<html")
    }) {
        "svg"
    } else {
        return Err(io::Error::other("지원하는 이미지 파일이 아닙니다"));
    };
    Ok(extension)
}

pub(super) fn import_web_images(space: &Path, images: &[(String, Vec<u8>)]) -> io::Result<String> {
    let mut created = Vec::new();
    let result = (|| {
        let mut tags = Vec::new();
        for (url, bytes) in images {
            let extension = image_extension(bytes)?;
            let url = gpui::http_client::Url::parse(url).ok();
            let stem = url
                .as_ref()
                .and_then(|url| url.path_segments()?.next_back())
                .and_then(|name| Path::new(name).file_stem()?.to_str())
                .map(|name| {
                    name.chars()
                        .take(80)
                        .map(|c| {
                            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                                c
                            } else {
                                '_'
                            }
                        })
                        .collect::<String>()
                })
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "image".into());
            // Prefixing also avoids Windows reserved names such as CON and AUX.
            let name = copy_image(
                &space.join("images"),
                &format!("web-{stem}.{extension}"),
                &mut bytes.as_slice(),
                &mut created,
            )?;
            tags.push(image_tag(&name, &name));
        }
        Ok(tags.join("\n"))
    })();
    if result.is_err() {
        for path in created {
            let _ = fs::remove_file(path);
        }
    }
    result
}

pub(super) fn import_images(space: &Path, paths: &[PathBuf]) -> io::Result<String> {
    let directory = space.join("images");
    let mut created = Vec::new();
    let result = (|| {
        let mut tags = Vec::new();
        for path in paths
            .iter()
            .filter(|path| InputState::supports_image_path(path))
        {
            let mut source = fs::File::open(path)?;
            if !source.metadata()?.is_file() {
                return Err(io::Error::other("이미지 파일을 선택해 주세요"));
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| io::Error::other("이미지 파일 이름을 읽을 수 없습니다"))?;
            fs::create_dir_all(&directory)?;
            let original = name;
            let source_path = fs::canonicalize(path)?;
            let images_path = fs::canonicalize(&directory)?;
            let name = if source_path
                .parent()
                .is_some_and(|parent| crate::space::same_path(parent, &images_path))
            {
                name.to_owned()
            } else {
                copy_image(&directory, name, &mut source, &mut created)?
            };
            // Angle-bracket destinations preserve spaces, Unicode and parentheses.
            tags.push(image_tag(&name, original));
        }
        Ok(tags.join("\n"))
    })();
    if result.is_err() {
        // Only this batch's newly created files are removed; sources and earlier
        // images are never overwritten or deleted.
        for path in created {
            let _ = fs::remove_file(path);
        }
    }
    result
}

impl Edot {
    pub(super) fn paste_images(
        &mut self,
        editor: Entity<InputState>,
        item: &ClipboardItem,
        anchor: DropInsertion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<_> = item
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                ClipboardEntry::ExternalPaths(paths) => Some(&paths.0),
                _ => None,
            })
            .flatten()
            .filter(|path| InputState::supports_image_path(path))
            .cloned()
            .collect();
        // Explorer may include a thumbnail too; import the original files once.
        let sources = if paths.is_empty() {
            ImageSources::Clipboard(
                item.entries()
                    .iter()
                    .filter_map(|entry| match entry {
                        ClipboardEntry::Image(image) => Some(image.clone()),
                        _ => None,
                    })
                    .collect(),
            )
        } else {
            ImageSources::Files(paths)
        };
        self.start_image_import(editor, sources, anchor, window, cx);
    }

    pub(super) fn drop_web_images(
        &mut self,
        editor: Entity<InputState>,
        urls: Vec<String>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_image_drop(editor, ImageSources::Web(urls), position, window, cx);
    }
    pub(super) fn drop_images(
        &mut self,
        editor: Entity<InputState>,
        paths: &[PathBuf],
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<_> = paths
            .iter()
            .filter(|path| InputState::supports_image_path(path))
            .cloned()
            .collect();
        if paths.is_empty() {
            return;
        }
        self.start_image_drop(editor, ImageSources::Files(paths), position, window, cx);
    }

    fn start_image_drop(
        &mut self,
        editor: Entity<InputState>,
        sources: ImageSources,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.startup_failed || !editor.read(cx).can_insert_images() {
            return;
        }
        let anchor = editor.update(cx, |state, cx| state.begin_drop(position, window, cx));
        self.start_image_import(editor, sources, anchor, window, cx);
    }

    fn start_image_import(
        &mut self,
        editor: Entity<InputState>,
        sources: ImageSources,
        anchor: DropInsertion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.startup_failed || !editor.read(cx).can_insert_images() {
            return;
        }
        let Some(id) = self
            .open
            .iter()
            .find(|tab| tab.editor.as_ref() == Some(&editor))
            .map(|tab| tab.id)
        else {
            return;
        };
        let space = self.dir.clone();
        let destination = space.clone();
        self.status = "이미지 복사 중…".into();
        let client = cx.http_client();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let mut result = Some(match sources {
                ImageSources::Files(paths) => cx.background_executor().spawn(async move { import_images(&destination, &paths) }).await,
                ImageSources::Clipboard(images) => cx.background_executor().spawn(async move { import_clipboard_images(&destination, &images) }).await,
                ImageSources::Web(urls) => {
                    let mut images = Vec::new();
                    let mut error = None;
                    let mut total = 0;
                    for url in urls {
                        let http = client.clone();
                        let address = url.clone();
                        let downloaded = cx.background_executor().spawn(async move {
                            crate::image_http::image_bytes(http.as_ref(), &address).await
                        }).await;
                        match downloaded {
                            Ok(bytes) => {
                                total += bytes.len();
                                if total > 64 * 1024 * 1024 { error = Some(io::Error::other("이미지 전체 크기가 너무 큽니다")); break; }
                                images.push((url, bytes));
                            }
                            Err(failure) => { error = Some(io::Error::other(failure.to_string())); break; }
                        }
                    }
                    if let Some(error) = error { Err(error) }
                    else { cx.background_executor().spawn(async move { import_web_images(&destination, &images) }).await }
                }
            });
            loop {
                let pending = this.update_in(cx, |this, window, cx| {
                    if this.dir != space || !this.open.iter().any(|tab| tab.id == id && tab.editor.as_ref() == Some(&editor)) {
                        return false;
                    }
                    if result.as_ref().is_some_and(Result::is_ok)
                        && editor.read(cx).can_insert_images() && editor.read(cx).is_composing() {
                        return true;
                    }
                    match result.take().unwrap() {
                        Ok(tags) => {
                            let inserted = editor.update(cx, |state, cx| state.finish_drop(&anchor, &tags, cx));
                            if inserted {
                                if this.session.active == Some(id) {
                                    editor.update(cx, |state, cx| state.focus(window, cx));
                                }
                            } else {
                                this.status = "이미지 복사 완료 · 편집 상태가 바뀌어 태그 삽입이 취소되었습니다".into();
                            }
                        }
                        Err(error) => this.fail("이미지를 추가하지 못했습니다", error, cx),
                    }
                    cx.notify();
                    false
                }).unwrap_or(false);
                if !pending { break; }
                cx.background_executor().timer(Duration::from_millis(50)).await;
            }
        }).detach();
    }
}

enum ImageSources {
    Files(Vec<PathBuf>),
    Web(Vec<String>),
    Clipboard(Vec<gpui::Image>),
}
