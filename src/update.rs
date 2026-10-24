//! Release discovery and a saved-data-first handoff to the Windows installer.
use gpui::http_client::{AsyncBody, RedirectPolicy, Request, Result, anyhow};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{install_scope, prepare};

pub const VERSION: &str = env!("EDOT_APP_VERSION");
pub const BUILD_LABEL: &str = env!("EDOT_BUILD_LABEL");
const REPOSITORY: &str = env!("EDOT_RELEASE_REPOSITORY");
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;
pub const RESULT_FILE: &str = "update-result.txt";

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
    #[cfg(target_os = "windows")]
    pub size: u64,
    #[cfg(target_os = "windows")]
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
            #[cfg(target_os = "windows")]
            size,
            #[cfg(target_os = "windows")]
            sha256,
        })
    }

    pub fn page_url(&self) -> String {
        format!(
            "https://github.com/{REPOSITORY}/releases/tag/v{}",
            self.version
        )
    }

    #[cfg(any(target_os = "windows", test))]
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
    #[cfg(target_os = "windows")]
    Installing,
    Failed(String),
}

impl State {
    pub fn is_busy(&self) -> bool {
        match self {
            Self::Checking => true,
            #[cfg(target_os = "windows")]
            Self::Installing => true,
            _ => false,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
}
