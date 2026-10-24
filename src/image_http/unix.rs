use super::*;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_long, c_void},
    ptr::null_mut,
    time::Instant,
};

type Easy = *mut c_void;
type List = *mut c_void;
struct Api {
    init: unsafe extern "C" fn() -> Easy,
    cleanup: unsafe extern "C" fn(Easy),
    setopt: unsafe extern "C" fn(Easy, c_int, ...) -> c_int,
    perform: unsafe extern "C" fn(Easy) -> c_int,
    append: unsafe extern "C" fn(List, *const c_char) -> List,
    free_list: unsafe extern "C" fn(List),
    error: unsafe extern "C" fn(c_int) -> *const c_char,
}

impl Api {
    fn get() -> Result<&'static Self> {
        static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();
        API.get_or_init(|| unsafe { Self::load() }.map_err(|error| error.to_string()))
            .as_ref()
            .map_err(|error| anyhow!("{error}"))
    }

    unsafe fn load() -> Result<Self> {
        #[cfg(target_os = "macos")]
        let library = c"/usr/lib/libcurl.4.dylib";
        #[cfg(not(target_os = "macos"))]
        let library = c"libcurl.so.4";
        // The library remains loaded for the process lifetime: cached function pointers
        // and libcurl's global initialization may be used by either dedicated worker.
        let handle = unsafe { libc::dlopen(library.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return Err(anyhow!("시스템 libcurl을 불러오지 못했습니다"));
        }
        unsafe fn symbol(handle: Easy, name: &CStr) -> Result<*mut c_void> {
            let value = unsafe { libc::dlsym(handle, name.as_ptr()) };
            if value.is_null() {
                Err(anyhow!(
                    "libcurl API를 찾지 못했습니다: {}",
                    name.to_string_lossy()
                ))
            } else {
                Ok(value)
            }
        }
        // SAFETY: each symbol is checked and cast to its documented libcurl C ABI.
        unsafe {
            let global_init: unsafe extern "C" fn(c_long) -> c_int =
                std::mem::transmute(symbol(handle, c"curl_global_init")?);
            if global_init(3) != 0 {
                return Err(anyhow!("libcurl 초기화에 실패했습니다"));
            }
            Ok(Self {
                init: std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> Easy>(symbol(
                    handle,
                    c"curl_easy_init",
                )?),
                cleanup: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(Easy)>(symbol(
                    handle,
                    c"curl_easy_cleanup",
                )?),
                setopt: std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn(Easy, c_int, ...) -> c_int,
                >(symbol(handle, c"curl_easy_setopt")?),
                perform: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(Easy) -> c_int>(
                    symbol(handle, c"curl_easy_perform")?,
                ),
                append: std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn(List, *const c_char) -> List,
                >(symbol(handle, c"curl_slist_append")?),
                free_list: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(List)>(symbol(
                    handle,
                    c"curl_slist_free_all",
                )?),
                error: std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn(c_int) -> *const c_char,
                >(symbol(handle, c"curl_easy_strerror")?),
            })
        }
    }
}

struct Handle {
    api: &'static Api,
    easy: Easy,
    headers: List,
}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: handle/list are exclusively owned and no callback survives perform.
        unsafe {
            (self.api.cleanup)(self.easy);
            (self.api.free_list)(self.headers);
        }
    }
}
impl Handle {
    fn check(&self, code: c_int) -> Result<()> {
        if code == 0 {
            return Ok(());
        }
        // SAFETY: curl_easy_strerror returns a static NUL-terminated string.
        let message = unsafe { CStr::from_ptr((self.api.error)(code)) };
        Err(anyhow!("HTTP 다운로드 실패: {}", message.to_string_lossy()))
    }
    fn long(&self, option: c_int, value: c_long) -> Result<()> {
        // SAFETY: these options take long, not an ABI-dependent Rust integer.
        self.check(unsafe { (self.api.setopt)(self.easy, option, value) })
    }
    fn string(&self, option: c_int, value: &CStr) -> Result<()> {
        // libcurl copies these option strings during setopt.
        self.check(unsafe { (self.api.setopt)(self.easy, option, value.as_ptr()) })
    }
}

struct Transfer<'a> {
    output: &'a mut dyn io::Write,
    max_bytes: usize,
    total: usize,
    status: u16,
    location: Option<String>,
    status_abort: bool,
    deadline: Instant,
    result: Result<()>,
}

unsafe extern "C" fn write_callback(
    data: *mut c_char,
    size: usize,
    count: usize,
    info: *mut c_void,
) -> usize {
    // SAFETY: perform is synchronous and the context/buffer live for the call.
    let state = unsafe { &mut *info.cast::<Transfer<'_>>() };
    let Some(length) = size.checked_mul(count) else {
        return 0;
    };
    if Instant::now() >= state.deadline {
        state.result = Err(anyhow!("Image request timed out"));
        return 0;
    }
    if length > state.max_bytes.saturating_sub(state.total) {
        state.result = Err(anyhow!("Remote download exceeds the size limit"));
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) };
    if let Err(error) = state.output.write_all(bytes) {
        state.result = Err(error.into());
        return 0;
    }
    state.total += length;
    length
}

unsafe extern "C" fn header_callback(
    data: *mut c_char,
    size: usize,
    count: usize,
    info: *mut c_void,
) -> usize {
    // SAFETY: libcurl supplies an initialized header line and our live context.
    let state = unsafe { &mut *info.cast::<Transfer<'_>>() };
    let Some(length) = size.checked_mul(count) else {
        return 0;
    };
    if length > 64 * 1024 {
        state.result = Err(anyhow!("HTTP header exceeds the size limit"));
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) };
    let Ok(line) = std::str::from_utf8(bytes) else {
        return length;
    };
    if line.starts_with("HTTP/") {
        state.status = line
            .split_whitespace()
            .nth(1)
            .and_then(|status| status.parse().ok())
            .unwrap_or(0);
        state.location = None;
    } else if let Some((name, value)) = line.split_once(':') {
        if name.eq_ignore_ascii_case("location") {
            state.location = Some(value.trim().to_owned());
        }
    } else if line.trim().is_empty() && state.status >= 300 {
        // Error/redirect bodies are not needed and can themselves be arbitrarily large.
        state.status_abort = true;
        return 0;
    }
    length
}

pub(crate) fn download_to(
    request: Request<AsyncBody>,
    max_bytes: usize,
    max_time: Duration,
    output: &mut impl io::Write,
) -> Result<u16> {
    if request.method() != Method::GET {
        return Err(anyhow!("The image HTTP client only supports GET"));
    }
    let mut url = Url::parse(&request.uri().to_string())?;
    let original_origin = url.origin();
    let limit = match request.extensions().get::<RedirectPolicy>() {
        Some(RedirectPolicy::FollowAll) => 10,
        Some(RedirectPolicy::FollowLimit(limit)) => (*limit).min(10),
        _ => 0,
    };
    let timeout = request
        .extensions()
        .get::<RequestTimeout>()
        .map_or(max_time, |timeout| timeout.0.min(max_time));
    let deadline = Instant::now() + timeout;
    let api = Api::get()?;
    for followed in 0..=limit {
        if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
            return Err(anyhow!("Unsupported image URL scheme or missing host"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(anyhow!(
                "인증 정보가 포함된 이미지 주소는 지원하지 않습니다"
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(anyhow!("Image request timed out"));
        }
        // SAFETY: global init has completed; this easy handle is used by this thread only.
        let easy = unsafe { (api.init)() };
        if easy.is_null() {
            return Err(anyhow!("HTTP 요청을 만들지 못했습니다"));
        }
        let mut state = Transfer {
            output,
            max_bytes,
            total: 0,
            status: 0,
            location: None,
            status_abort: false,
            deadline,
            result: Ok(()),
        };
        let mut handle = Handle {
            api,
            easy,
            headers: null_mut(),
        };
        let milliseconds = remaining.as_millis().clamp(1, c_long::MAX as u128) as c_long;
        handle.string(10002, &CString::new(url.as_str())?)?; // URL
        handle.string(10018, c"Edot")?; // USERAGENT
        handle.string(10102, c"gzip,deflate")?; // ACCEPT_ENCODING; limit is on decoded bytes
        handle.long(181, 3)?; // PROTOCOLS: HTTP | HTTPS (available on older system curl too)
        handle.long(52, 0)?; // FOLLOWLOCATION: handle each hop to reject every HTTPS downgrade
        handle.long(99, 1)?; // NOSIGNAL: worker threads must not install signal handlers
        handle.long(155, milliseconds)?; // TIMEOUT_MS: one deadline across all redirects
        handle.long(156, milliseconds.min(5000))?; // CONNECTTIMEOUT_MS
        handle.long(19, 1)?; // LOW_SPEED_LIMIT
        handle.long(20, 10)?; // LOW_SPEED_TIME
        handle.long(107, 0)?; // HTTPAUTH: no automatic authentication
        handle.long(111, 0)?; // PROXYAUTH
        handle.long(64, 1)?; // SSL_VERIFYPEER
        handle.long(81, 2)?; // SSL_VERIFYHOST
        for (name, value) in request.headers() {
            if url.origin() != original_origin && (name == "authorization" || name == "cookie") {
                continue;
            }
            let header = CString::new(format!("{name}: {}", value.to_str()?))?;
            // SAFETY: curl_slist_append copies the string and preserves the old list on failure.
            let headers = unsafe { (api.append)(handle.headers, header.as_ptr()) };
            if headers.is_null() {
                return Err(anyhow!("HTTP 헤더를 만들지 못했습니다"));
            }
            handle.headers = headers;
        }
        let context = (&mut state as *mut Transfer<'_>).cast::<c_void>();
        // SAFETY: pointer/function option types match the libcurl ABI; context and
        // header list outlive synchronous perform and cleanup follows it on all paths.
        unsafe {
            handle.check((api.setopt)(easy, 10023, handle.headers))?; // HTTPHEADER
            handle.check((api.setopt)(easy, 10001, context))?; // WRITEDATA
            handle.check((api.setopt)(
                easy,
                20011,
                write_callback as unsafe extern "C" fn(_, _, _, _) -> _,
            ))?;
            handle.check((api.setopt)(easy, 10029, context))?; // HEADERDATA
            handle.check((api.setopt)(
                easy,
                20079,
                header_callback as unsafe extern "C" fn(_, _, _, _) -> _,
            ))?;
        }
        let result = unsafe { (api.perform)(easy) };
        state.result?;
        if !state.status_abort {
            handle.check(result)?;
        }
        let status = state.status;
        if limit > 0
            && matches!(status, 301 | 302 | 303 | 307 | 308)
            && let Some(location) = state.location.take()
        {
            if followed == limit {
                return Err(anyhow!("HTTP redirect limit exceeded"));
            }
            let next = url.join(&location)?;
            if url.scheme() == "https" && next.scheme() != "https" {
                return Err(anyhow!("HTTPS에서 안전하지 않은 주소로 이동할 수 없습니다"));
            }
            url = next;
        } else {
            if !(100..=599).contains(&status) {
                return Err(anyhow!("Invalid HTTP response status"));
            }
            return Ok(status);
        }
    }
    Err(anyhow!("HTTP redirect limit exceeded"))
}
