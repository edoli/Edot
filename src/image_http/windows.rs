use super::*;
use gpui::http_client::Host;
use std::{
    ffi::c_void,
    ptr::{null, null_mut},
    time::Instant,
};
use windows_sys::Win32::Networking::WinHttp::*;

struct InternetHandle(*mut c_void);

impl InternetHandle {
    fn new(handle: *mut c_void) -> Result<Self> {
        if handle.is_null() {
            Err(io::Error::last_os_error().into())
        } else {
            Ok(Self(handle))
        }
    }

    fn option(&self, name: u32, value: u32) -> Result<()> {
        // SAFETY: The handle is live and the option reads a DWORD synchronously.
        check(unsafe { WinHttpSetOption(self.0, name, (&value as *const u32).cast(), 4) })
    }
}

impl Drop for InternetHandle {
    fn drop(&mut self) {
        // SAFETY: This wrapper exclusively owns the handle.
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn check(success: i32) -> Result<()> {
    if success == 0 {
        Err(io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn set_timeouts(handle: &InternetHandle, deadline: Instant) -> Result<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(anyhow!("Image request timed out"));
    }
    let timeout = remaining.as_millis().clamp(1, 10_000) as i32;
    // SAFETY: Synchronous handles and positive millisecond timeouts.
    check(unsafe {
        WinHttpSetTimeouts(
            handle.0,
            timeout.min(5_000),
            timeout.min(5_000),
            timeout,
            timeout,
        )
    })
}

/// Shared WinHTTP transport. Update payloads stream to disk instead of RAM.
pub(crate) fn download_to(
    request: Request<AsyncBody>,
    max_bytes: usize,
    max_time: Duration,
    output: &mut impl io::Write,
) -> Result<u16> {
    if request.method() != Method::GET {
        return Err(anyhow!("The image HTTP client only supports GET"));
    }
    let url = Url::parse(&request.uri().to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(anyhow!("Unsupported image URL scheme"));
    }
    let host = match url.host().ok_or_else(|| anyhow!("Missing image host"))? {
        Host::Ipv6(address) => address.to_string(),
        host => host.to_string(),
    };
    let host = wide(&host);
    let path = wide(&match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    });
    let timeout = request
        .extensions()
        .get::<RequestTimeout>()
        .map_or(max_time, |timeout| timeout.0.min(max_time));
    let deadline = Instant::now() + timeout;

    // SAFETY: All strings are NUL terminated, optional pointers are null, and
    // the owned session/connection outlive each child handle.
    let session = InternetHandle::new(unsafe {
        WinHttpOpen(
            wide("Edot").as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        )
    })?;
    set_timeouts(&session, deadline)?;
    let connection = InternetHandle::new(unsafe {
        WinHttpConnect(
            session.0,
            host.as_ptr(),
            url.port_or_known_default().unwrap(),
            0,
        )
    })?;
    let handle = InternetHandle::new(unsafe {
        WinHttpOpenRequest(
            connection.0,
            wide("GET").as_ptr(),
            path.as_ptr(),
            null(),
            null(),
            null(),
            if url.scheme() == "https" {
                WINHTTP_FLAG_SECURE
            } else {
                0
            },
        )
    })?;
    let redirects = match request.extensions().get::<RedirectPolicy>() {
        Some(RedirectPolicy::FollowAll) => 10,
        Some(RedirectPolicy::FollowLimit(limit)) => (*limit).min(10),
        _ => 0,
    };
    handle.option(
        WINHTTP_OPTION_REDIRECT_POLICY,
        if redirects == 0 {
            WINHTTP_OPTION_REDIRECT_POLICY_NEVER
        } else {
            WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP
        },
    )?;
    if redirects > 0 {
        handle.option(WINHTTP_OPTION_MAX_HTTP_AUTOMATIC_REDIRECTS, redirects)?;
    }
    handle.option(
        WINHTTP_OPTION_DISABLE_FEATURE,
        WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION,
    )?;
    handle.option(
        WINHTTP_OPTION_DECOMPRESSION,
        WINHTTP_DECOMPRESSION_FLAG_GZIP | WINHTTP_DECOMPRESSION_FLAG_DEFLATE,
    )?;
    let mut headers = String::new();
    for (name, value) in request.headers() {
        headers.push_str(name.as_str());
        headers.push_str(": ");
        headers.push_str(value.to_str()?);
        headers.push_str("\r\n");
    }
    let headers = wide(&headers);
    // SAFETY: This is a synchronous GET without a request body. Headers stay
    // alive for the call. Status and read buffers have their advertised sizes.
    check(unsafe {
        WinHttpSendRequest(
            handle.0,
            headers.as_ptr(),
            (headers.len() - 1) as u32,
            null(),
            0,
            0,
            0,
        )
    })?;
    check(unsafe { WinHttpReceiveResponse(handle.0, null_mut()) })?;
    let mut status = 0u32;
    let mut status_size = 4;
    check(unsafe {
        WinHttpQueryHeaders(
            handle.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&mut status as *mut u32).cast(),
            &mut status_size,
            null_mut(),
        )
    })?;
    let mut total = 0usize;
    if (200..300).contains(&status) {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            set_timeouts(&handle, deadline)?;
            let mut read = 0;
            check(unsafe {
                WinHttpReadData(
                    handle.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut read,
                )
            })?;
            if read == 0 {
                break;
            }
            if read as usize > max_bytes.saturating_sub(total) {
                return Err(anyhow!("Remote download exceeds the size limit"));
            }
            output.write_all(&buffer[..read as usize])?;
            total += read as usize;
        }
    }
    Ok(status as u16)
}
