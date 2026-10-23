//! Bounded HTTP downloads on dedicated workers, with OS-specific transports.
use gpui::{
    BackgroundExecutor, DedicatedExecutor,
    http_client::{
        AsyncBody, HttpClient, Method, RedirectPolicy, Request, RequestTimeout, Response, Result,
        Url, anyhow, http::HeaderValue,
    },
};
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::download_to;
#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::download_to;

const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_REQUEST_TIME: Duration = Duration::from_secs(30);

pub async fn image_bytes(client: &dyn HttpClient, url: &str) -> Result<Vec<u8>> {
    if let Some(data) = url.strip_prefix("data:image/") {
        let (header, body) = data
            .split_once(',')
            .ok_or_else(|| anyhow!("잘못된 이미지 주소입니다"))?;
        if !header.ends_with(";base64") || body.len() > MAX_IMAGE_BYTES * 4 / 3 + 4 {
            return Err(anyhow!("지원하지 않거나 너무 큰 이미지 데이터입니다"));
        }
        let mut bytes = Vec::with_capacity(body.len() * 3 / 4);
        let mut bits = 0u32;
        let mut count = 0u32;
        let mut padded = false;
        for byte in body.bytes() {
            if byte == b'=' {
                padded = true;
                continue;
            }
            if byte.is_ascii_whitespace() {
                continue;
            }
            if padded {
                return Err(anyhow!("잘못된 이미지 데이터입니다"));
            }
            let value = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return Err(anyhow!("잘못된 이미지 데이터입니다")),
            };
            bits = (bits << 6) | u32::from(value);
            count += 6;
            if count >= 8 {
                count -= 8;
                bytes.push((bits >> count) as u8);
            }
        }
        if count >= 6 || bits & ((1 << count) - 1) != 0 || bytes.len() > MAX_IMAGE_BYTES {
            return Err(anyhow!("잘못된 이미지 데이터입니다"));
        }
        return Ok(bytes);
    }
    let response = client.get(url, AsyncBody::empty(), true).await?;
    if !response.status().is_success() {
        return Err(anyhow!("이미지 서버 응답: {}", response.status()));
    }
    match response.into_body().0 {
        gpui::http_client::Inner::Bytes(bytes) => Ok(bytes.into_inner().to_vec()),
        _ => Err(anyhow!("이미지 응답을 읽을 수 없습니다")),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod transport_tests;

pub struct ImageHttpClient {
    executor: BackgroundExecutor,
    // No networking threads are created until a remote image is requested.
    workers: OnceLock<[DedicatedExecutor; 2]>,
    next_worker: AtomicUsize,
}

impl ImageHttpClient {
    pub fn new(executor: BackgroundExecutor) -> Self {
        Self {
            executor,
            workers: OnceLock::new(),
            next_worker: AtomicUsize::new(0),
        }
    }
}

impl HttpClient for ImageHttpClient {
    fn user_agent(&self) -> Option<&HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&Url> {
        // Each transport applies the platform proxy policy internally.
        None
    }

    fn send(
        &self,
        request: Request<AsyncBody>,
    ) -> Pin<Box<dyn Future<Output = Result<Response<AsyncBody>>> + Send>> {
        let workers = self.workers.get_or_init(|| {
            let executor = self.executor.scheduler_executor();
            std::array::from_fn(|_| DedicatedExecutor::new(&executor))
        });
        let worker = self.next_worker.fetch_add(1, Ordering::Relaxed) % workers.len();
        Box::pin(workers[worker].spawn(async move { download(request, MAX_IMAGE_BYTES) }))
    }
}

fn download(request: Request<AsyncBody>, max_bytes: usize) -> Result<Response<AsyncBody>> {
    let mut bytes = Vec::new();
    let status = download_to(request, max_bytes, MAX_REQUEST_TIME, &mut bytes)?;
    Ok(Response::builder().status(status).body(bytes.into())?)
}
