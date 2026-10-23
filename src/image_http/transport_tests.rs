use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex, atomic::AtomicBool},
    thread,
    time::Instant,
};

struct Server {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let seen = requests.clone();
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(stream) => stream,
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
                    let count = stream.read(&mut buffer).unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                }
                if request.is_empty() {
                    continue;
                }
                let text = String::from_utf8(request).unwrap();
                let path = text.split_whitespace().nth(1).unwrap().to_owned();
                seen.lock().unwrap().push(text);
                let response = match path.as_str() {
                    "/ok" => b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello".as_slice(),
                    "/redirect" => b"HTTP/1.1 302 Found\r\nLocation: /ok\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    "/loop" => b"HTTP/1.1 302 Found\r\nLocation: /loop\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    "/file" => b"HTTP/1.1 302 Found\r\nLocation: file:///etc/passwd\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    "/chunked" => b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nhel\r\n2\r\nlo\r\n0\r\n\r\n",
                    "/gzip" => b"HTTP/1.1 200 OK\x0d\x0aContent-Encoding: gzip\x0d\x0aContent-Length: 25\x0d\x0aConnection: close\x0d\x0a\x0d\x0a\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\x0a\xcbH\xcd\xc9\xc9\x07\x00\x86\xa6\x106\x05\x00\x00\x00",
                    "/error" => b"HTTP/1.1 404 Not Found\r\nContent-Length: 2147483647\r\nConnection: close\r\n\r\n",
                    "/slow" => {
                        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n").unwrap();
                        thread::sleep(Duration::from_millis(150));
                        let _ = stream.write_all(b"hello");
                        continue;
                    }
                    _ => panic!("unexpected request: {path}"),
                };
                let _ = stream.write_all(response);
            }
        });
        Self {
            base,
            requests,
            stop,
            thread: Some(worker),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.thread.take() {
            let result = worker.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

fn request(url: &str, follow: bool) -> Request<AsyncBody> {
    let mut builder = Request::builder().uri(url);
    if follow {
        builder = builder.extension(RedirectPolicy::FollowLimit(2));
    }
    builder.body(().into()).unwrap()
}

#[test]
fn native_transport_bounds_status_redirects_decoded_body_and_deadline() {
    let server = Server::new();
    for path in ["/ok", "/redirect", "/chunked", "/gzip"] {
        let mut output = Vec::new();
        assert_eq!(
            download_to(
                request(&format!("{}{path}", server.base), true),
                5,
                Duration::from_secs(3),
                &mut output
            )
            .unwrap(),
            200
        );
        assert_eq!(output, b"hello");
    }
    let mut output = Vec::new();
    assert_eq!(
        download_to(
            request(&format!("{}/redirect", server.base), false),
            5,
            Duration::from_secs(3),
            &mut output
        )
        .unwrap(),
        302
    );
    assert!(output.is_empty());
    assert_eq!(
        download_to(
            request(&format!("{}/error", server.base), true),
            5,
            Duration::from_secs(3),
            &mut output
        )
        .unwrap(),
        404
    );
    assert!(output.is_empty());
    for path in ["/loop", "/file"] {
        assert!(
            download_to(
                request(&format!("{}{path}", server.base), true),
                5,
                Duration::from_secs(3),
                &mut output
            )
            .is_err()
        );
    }
    assert!(
        download_to(
            request(&format!("{}/ok", server.base), true),
            4,
            Duration::from_secs(3),
            &mut output
        )
        .unwrap_err()
        .to_string()
        .contains("size limit")
    );
    let start = Instant::now();
    assert!(
        download_to(
            request(&format!("{}/slow", server.base), true),
            5,
            Duration::from_millis(50),
            &mut Vec::new()
        )
        .is_err()
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(server.requests.lock().unwrap().len() >= 8);
}
