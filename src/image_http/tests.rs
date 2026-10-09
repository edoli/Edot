use super::*;
use gpui::{Asset, ImageAssetLoader, ImageCacheError, Resource};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, atomic::AtomicBool},
    thread::{self, JoinHandle},
};

const GIF: &[u8] = b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\xff\xff\x21\xf9\x04\x01\0\0\0\0\x2c\0\0\0\0\x01\0\x01\0\0\x02\x02\x44\x01\0\x3b";

fn load(
    source: Resource,
    cx: &mut gpui::App,
) -> Pin<Box<dyn Future<Output = Result<Arc<gpui::RenderImage>, ImageCacheError>> + Send>> {
    Box::pin(ImageAssetLoader::load(source, cx))
}

struct Server {
    base: String,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let shutdown = stopped.clone();
        let thread = thread::spawn(move || {
            while !shutdown.load(Ordering::Relaxed) {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.ends_with(b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).unwrap();
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                if request.is_empty() {
                    continue;
                }
                let request = String::from_utf8(request).unwrap();
                let path = request.split_whitespace().nth(1).unwrap();
                let (status, headers, body): (_, _, &[u8]) = match path {
                    "/redirect" => ("302 Found", "Location: /image?name=%ED%95%9C\r\n", b""),
                    "/loop" => ("302 Found", "Location: /loop\r\n", b""),
                    "/missing" => ("404 Not Found", "", b"missing"),
                    "/invalid" => ("200 OK", "", b"not an image"),
                    "/slow" => {
                        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\n").unwrap();
                        thread::sleep(Duration::from_millis(200));
                        let _ = stream.write_all(b"x");
                        continue;
                    }
                    "/chunked" => {
                        write!(stream, "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n", GIF.len()).unwrap();
                        stream.write_all(GIF).unwrap();
                        stream.write_all(b"\r\n0\r\n\r\n").unwrap();
                        continue;
                    }
                    "/image?name=%ED%95%9C" | "/large" => {
                        ("200 OK", "Content-Type: image/gif\r\n", GIF)
                    }
                    _ => panic!("Unexpected test request: {path}"),
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                let _ = stream.write_all(body);
            }
        });
        Self {
            base,
            stopped,
            thread: Some(thread),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        let result = self.thread.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

#[gpui::test]
async fn remote_images_decode_redirect_and_fail_without_changing_local_loading(
    cx: &mut gpui::TestAppContext,
) {
    let server = Server::start();
    let client = Arc::new(ImageHttpClient::new(cx.background_executor.clone()));
    assert!(client.workers.get().is_none());
    cx.update(|cx| cx.set_http_client(client.clone()));
    for path in ["/image?name=%ED%95%9C", "/redirect", "/chunked"] {
        let image = cx
            .update(|cx| load(Resource::Uri(format!("{}{path}", server.base).into()), cx))
            .await
            .unwrap();
        assert_eq!(image.size(0).width.0, 1);
        assert_eq!(image.size(0).height.0, 1);
        assert_eq!(image.frame_count(), 1);
    }
    let response = client
        .get(&format!("{}/redirect", server.base), ().into(), false)
        .await
        .unwrap();
    assert_eq!(response.status(), 302);
    let missing = cx
        .update(|cx| load(Resource::Uri(format!("{}/missing", server.base).into()), cx))
        .await;
    assert!(matches!(missing, Err(ImageCacheError::BadStatus { status, .. }) if status == 404));
    for path in ["/invalid", "/loop"] {
        assert!(
            cx.update(|cx| { load(Resource::Uri(format!("{}{path}", server.base).into()), cx) })
                .await
                .is_err()
        );
    }
    let oversized = Request::builder()
        .uri(format!("{}/large", server.base))
        .body(().into())
        .unwrap();
    assert!(
        download(oversized, 16)
            .err()
            .unwrap()
            .to_string()
            .contains("size limit")
    );
    let slow = Request::builder()
        .uri(format!("{}/slow", server.base))
        .extension(RequestTimeout(Duration::from_millis(50)))
        .body(().into())
        .unwrap();
    assert!(client.send(slow).await.is_err());

    let local = cx
        .update(|cx| {
            load(
                Resource::Path(std::path::Path::new("assets/app-icon/edot.png").into()),
                cx,
            )
        })
        .await
        .unwrap();
    assert!(local.size(0).width.0 > 0);

    // Optional live HTTPS smoke check; normal test runs stay entirely local.
    if let Ok(url) = std::env::var("EDOT_TEST_IMAGE_URL") {
        let image = cx
            .update(|cx| load(Resource::Uri(url.into()), cx))
            .await
            .unwrap();
        assert!(image.size(0).width.0 > 0);
    }
}
