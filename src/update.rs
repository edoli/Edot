//! Release discovery and a saved-data-first handoff to the Windows installer.
use gpui::http_client::{AsyncBody, RedirectPolicy, Request, Result, anyhow};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};

pub const VERSION: &str = env!("EDOT_APP_VERSION");
pub const BUILD_LABEL: &str = env!("EDOT_BUILD_LABEL");
const REPOSITORY: &str = env!("EDOT_RELEASE_REPOSITORY");
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;
pub const RESULT_FILE: &str = "update-result.txt";
const HELPER_SCRIPT: &str = include_str!("../installer/update.ps1");

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Version([u16; 3]);

impl Version {
    fn parse(text: &str) -> Result<Self> {
        let mut parts = text.split('.');
        let mut numbers = [0; 3];
        for number in &mut numbers {
            let part = parts.next().ok_or_else(|| anyhow!("잘못된 버전입니다"))?;
            if part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                return Err(anyhow!("잘못된 버전입니다"));
            }
            *number = part.parse()?;
        }
        if parts.next().is_some() {
            return Err(anyhow!("잘못된 버전입니다"));
        }
        Ok(Self(numbers))
    }
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub size: u64,
    pub sha256: String,
}

impl Release {
    fn parse(text: &str) -> Result<Self> {
        let mut lines = text.lines();
        if lines.next() != Some("EDOTUPDATE1") {
            return Err(anyhow!("지원하지 않는 업데이트 정보입니다"));
        }
        let version = field(&mut lines, "version=")?.to_owned();
        Version::parse(&version)?;
        let size: u64 = field(&mut lines, "size=")?.parse()?;
        let sha256 = field(&mut lines, "sha256=")?.to_owned();
        if lines.next().is_some()
            || size == 0
            || size > MAX_INSTALLER_BYTES
            || sha256.len() != 64
            || !sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(anyhow!("잘못된 업데이트 정보입니다"));
        }
        Ok(Self {
            version,
            size,
            sha256,
        })
    }

    pub fn page_url(&self) -> String {
        format!(
            "https://github.com/{REPOSITORY}/releases/tag/v{}",
            self.version
        )
    }

    fn installer_url(&self) -> String {
        format!(
            "https://github.com/{REPOSITORY}/releases/download/v{0}/Edot-Setup-{0}-windows-x64.exe",
            self.version
        )
    }
}

fn field<'a>(lines: &mut impl Iterator<Item = &'a str>, prefix: &str) -> Result<&'a str> {
    lines
        .next()
        .and_then(|line| line.strip_prefix(prefix))
        .ok_or_else(|| anyhow!("업데이트 정보의 필드가 잘못되었습니다"))
}

#[derive(Clone, Debug, Default)]
pub enum State {
    #[default]
    Unchecked,
    Checking,
    Current,
    NoRelease,
    Available(Release),
    Installing,
    Failed(String),
}

pub fn check() -> Result<State> {
    let mut bytes = Vec::new();
    let status = get(
        &format!("https://github.com/{REPOSITORY}/releases/latest/download/edot-update.txt"),
        4096,
        Duration::from_secs(20),
        &mut bytes,
    )?;
    if status == 404 {
        return Ok(State::NoRelease);
    }
    if status != 200 {
        return Err(anyhow!("업데이트 서버 응답: {status}"));
    }
    let release = Release::parse(std::str::from_utf8(&bytes)?)?;
    Ok(
        if Version::parse(&release.version)? > Version::parse(VERSION)? {
            State::Available(release)
        } else {
            State::Current
        },
    )
}

fn get(url: &str, limit: usize, time: Duration, output: &mut impl Write) -> Result<u16> {
    let mut request = Request::get(url)
        .header("User-Agent", concat!("Edot/", env!("EDOT_APP_VERSION")))
        .body(AsyncBody::empty())?;
    request
        .extensions_mut()
        .insert(RedirectPolicy::FollowLimit(10));
    crate::image_http::download_to(request, limit, time, output)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallScope {
    User,
    Machine,
}

/// Match the actual executable directory, so a copied portable EXE is never updated in place.
pub fn install_scope(exe: &Path) -> Option<InstallScope> {
    let directory = exe.parent()?.canonicalize().ok()?;
    for (root, scope) in [
        (HKEY_CURRENT_USER, InstallScope::User),
        (HKEY_LOCAL_MACHINE, InstallScope::Machine),
    ] {
        if registered_directory(root)
            .and_then(|path| path.canonicalize().ok())
            .is_some_and(|path| crate::space::same_path(&path, &directory))
        {
            return Some(scope);
        }
    }
    None
}

fn registered_directory(root: HKEY) -> Option<PathBuf> {
    let wide = |text: &str| text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut buffer = [0u16; 32768];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    // SAFETY: Live fixed-size buffer, NUL-terminated names, and a predefined registry root.
    let status = unsafe {
        RegGetValueW(
            root,
            wide("Software\\Edoli\\Edot").as_ptr(),
            wide("InstallLocation").as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 || size < 2 {
        return None;
    }
    let length = (size as usize / 2).min(buffer.len());
    let end = buffer[..length].iter().position(|&c| c == 0)?;
    Some(PathBuf::from(String::from_utf16(&buffer[..end]).ok()?))
}

/// Owns an isolated download directory until the helper has accepted the handoff.
pub struct Handoff {
    pub directory: PathBuf,
    child: Option<Child>,
    committed: bool,
}

impl Drop for Handoff {
    fn drop(&mut self) {
        if !self.committed {
            if let Some(child) = self.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            // Only this freshly created directory and its own files are removed.
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

impl Handoff {
    pub fn commit(mut self) -> io::Result<()> {
        fs::write(self.directory.join("go.pending"), b"install")?;
        fs::rename(self.directory.join("go.pending"), self.directory.join("go"))?;
        self.committed = true;
        Ok(())
    }
}

pub fn prepare(
    release: &Release,
    exe: &Path,
    scope: InstallScope,
    space: &Path,
    config: &Path,
) -> Result<Handoff> {
    if install_scope(exe) != Some(scope) {
        return Err(anyhow!(
            "설치 경로가 변경되었습니다. 설치 프로그램을 다시 실행하세요"
        ));
    }
    // Preserve relative --space / EDOT_CONFIG_DIR paths across a different restart cwd.
    let space = std::path::absolute(space)?;
    let config = std::path::absolute(config)?;
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|e| anyhow!("임시 폴더를 만들 수 없습니다: {e}"))?;
    let name: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let directory = std::env::temp_dir().join(format!("edot-update-{name}"));
    fs::create_dir(&directory)?;
    let mut handoff = Handoff {
        directory,
        child: None,
        committed: false,
    };
    let installer = handoff.directory.join("setup.exe");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&installer)?;
    let status = get(
        &release.installer_url(),
        release.size as usize,
        Duration::from_secs(300),
        &mut file,
    )?;
    file.sync_all()?;
    if status != 200 || file.metadata()?.len() != release.size {
        return Err(anyhow!(
            "설치 파일 다운로드가 완전하지 않습니다 (응답 {status})"
        ));
    }
    drop(file);
    let script = handoff.directory.join("update.ps1");
    fs::write(&script, HELPER_SCRIPT)?;
    let system =
        std::env::var_os("SystemRoot").ok_or_else(|| anyhow!("Windows 경로를 찾을 수 없습니다"))?;
    let shell = PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    // The script is constant. Paths and release data are environment values, never executable code.
    let command = "& ([ScriptBlock]::Create([IO.File]::ReadAllText($env:EDOT_UPDATE_SCRIPT)))";
    let encoded = base64_utf16(command);
    let child = Command::new(shell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
            &encoded,
        ])
        .env("EDOT_UPDATE_SCRIPT", &script)
        .env("EDOT_UPDATE_DIR", &handoff.directory)
        .env("EDOT_UPDATE_EXE", exe)
        .env(
            "EDOT_UPDATE_SCOPE",
            if scope == InstallScope::Machine {
                "machine"
            } else {
                "user"
            },
        )
        .env("EDOT_UPDATE_HASH", &release.sha256)
        .env("EDOT_UPDATE_VERSION", &release.version)
        .env("EDOT_UPDATE_PARENT", std::process::id().to_string())
        .env("EDOT_SPACE_DIR", &space)
        .env("EDOT_CONFIG_DIR", &config)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .spawn()?;
    handoff.child = Some(child);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if handoff.directory.join("ready").is_file() {
            return Ok(handoff);
        }
        if handoff.child.as_mut().unwrap().try_wait()?.is_some() {
            return Err(anyhow!(
                read_result(&config)
                    .unwrap_or_else(|| "업데이트 도우미를 시작하지 못했습니다".into())
            ));
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("업데이트 도우미 응답 시간이 초과되었습니다"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn read_result(config: &Path) -> Option<String> {
    let mut text = String::new();
    File::open(config.join(RESULT_FILE))
        .ok()?
        .take(4096)
        .read_to_string(&mut text)
        .ok()?;
    let text = text.trim();
    // Older helpers persisted a success notice. Keep errors visible without showing that banner.
    if text.is_empty()
        || text
            .strip_prefix('v')
            .and_then(|text| text.strip_suffix(" 업데이트를 완료했습니다."))
            .is_some_and(|version| Version::parse(version).is_ok())
    {
        return None;
    }
    Some(text.to_owned())
}

fn base64_utf16(text: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = std::ffi::OsStr::new(text)
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for part in bytes.chunks(3) {
        let bits = (u32::from(part[0]) << 16)
            | (u32::from(*part.get(1).unwrap_or(&0)) << 8)
            | u32::from(*part.get(2).unwrap_or(&0));
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            result.push(if index > part.len() {
                '='
            } else {
                ALPHABET[((bits >> shift) & 63) as usize] as char
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_update_success_is_hidden_and_failure_remains_visible() {
        let config = std::env::temp_dir().join(format!(
            "edot-update-result-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&config).unwrap();
        assert!(read_result(&config).is_none());
        for result in [
            "",
            "\r\n",
            "v0.1.1 업데이트를 완료했습니다.\r\n",
            "v10.20.30 업데이트를 완료했습니다.",
        ] {
            fs::write(config.join(RESULT_FILE), result).unwrap();
            assert!(read_result(&config).is_none(), "{result}");
        }
        for result in [
            "업데이트 실패: 설치가 취소되었습니다.",
            "앱을 다시 실행하지 못했습니다: 실행 오류",
        ] {
            fs::write(config.join(RESULT_FILE), result).unwrap();
            assert_eq!(read_result(&config).as_deref(), Some(result));
        }
        fs::remove_dir_all(config).unwrap();
    }

    #[test]
    fn versions_compare_numerically_and_reject_unsafe_tags() {
        assert!(Version::parse("0.10.0").unwrap() > Version::parse("0.9.9").unwrap());
        assert!(Version::parse("1.0.0").unwrap() > Version::parse("0.99.99").unwrap());
        for invalid in [
            "v1.0.0",
            "1.0",
            "01.0.0",
            "1.0.0-beta",
            "1.0.65536",
            "1.0.0/evil",
            "1.-1.0",
            "1.0.0\n",
        ] {
            assert!(Version::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn manifest_rejects_missing_checksums_oversize_and_injected_fields() {
        let valid = format!(
            "EDOTUPDATE1\nversion=0.0.2\nsize=123\nsha256={}\n",
            "a".repeat(64)
        );
        let release = Release::parse(&valid).unwrap();
        assert!(
            release
                .installer_url()
                .ends_with("/v0.0.2/Edot-Setup-0.0.2-windows-x64.exe")
        );
        for text in [
            valid.replace("size=123", "size=0"),
            valid.replace("size=123", "size=268435457"),
            valid.replace(&"a".repeat(64), "bad"),
            valid.replace("version=0.0.2", "version=../../evil"),
            format!("{valid}url=https://evil.test\n"),
            valid.replace("sha256=", "checksum="),
        ] {
            assert!(Release::parse(&text).is_err());
        }
    }

    #[test]
    fn abandoned_handoff_removes_only_its_own_directory() {
        let directory =
            std::env::temp_dir().join(format!("edot-handoff-test-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("setup.exe"), b"incomplete").unwrap();
        drop(Handoff {
            directory: directory.clone(),
            child: None,
            committed: false,
        });
        assert!(!directory.exists());
    }
}
