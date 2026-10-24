//! GitHub release discovery and a saved-data-first updater handoff on each OS.
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
#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::prepare;

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
    pub size: u64,
    pub sha256: String,
    pub asset_name: String,
}

#[derive(serde::Deserialize)]
struct ApiRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ApiAsset>,
}

#[derive(serde::Deserialize)]
struct ApiAsset {
    name: String,
    size: u64,
    digest: Option<String>,
}

impl Release {
    fn from_api(response: ApiRelease, package: &str) -> Result<Self> {
        let version = response
            .tag_name
            .strip_prefix('v')
            .ok_or_else(|| anyhow!("잘못된 릴리스 태그입니다"))?
            .to_owned();
        Version::parse(&version)?;
        let asset = response
            .assets
            .into_iter()
            .find(|asset| asset.name == package)
            .ok_or_else(|| anyhow!("현재 플랫폼의 업데이트 파일이 없습니다"))?;
        let sha256 = asset
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .ok_or_else(|| anyhow!("업데이트 파일의 SHA-256 정보가 없습니다"))?
            .to_owned();
        if response.draft
            || response.prerelease
            || asset.size == 0
            || asset.size > MAX_INSTALLER_BYTES
            || sha256.len() != 64
            || !sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(anyhow!("잘못된 업데이트 정보입니다"));
        }
        Ok(Self {
            version,
            size: asset.size,
            sha256: sha256.to_ascii_lowercase(),
            asset_name: asset.name,
        })
    }

    fn installer_url(&self) -> String {
        format!(
            "https://github.com/{REPOSITORY}/releases/download/v{}/{}",
            self.version, self.asset_name
        )
    }
}

fn package_name(exe: &Path) -> &'static str {
    #[cfg(windows)]
    {
        if install_scope(exe).is_some() {
            "edot-installer-exe.exe"
        } else {
            "edot-windows.zip"
        }
    }
    #[cfg(target_os = "macos")]
    {
        if exe
            .ancestors()
            .any(|path| path.extension().is_some_and(|ext| ext == "app"))
        {
            "edot-installer-dmg.dmg"
        } else {
            "edot-mac.zip"
        }
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("APPIMAGE").is_some() {
            "edot-installer-appimage.AppImage"
        } else if exe == Path::new("/usr/bin/edot") {
            "edot-installer-deb.deb"
        } else {
            "edot-linux.zip"
        }
    }
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

impl State {
    pub fn is_busy(&self) -> bool {
        match self {
            Self::Checking => true,
            Self::Installing => true,
            _ => false,
        }
    }
}

pub fn check() -> Result<State> {
    let mut bytes = Vec::new();
    let status = get(
        &format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"),
        1024 * 1024,
        Duration::from_secs(20),
        &mut bytes,
    )?;
    if status == 404 {
        return Ok(State::NoRelease);
    }
    if status != 200 {
        return Err(anyhow!("업데이트 서버 응답: {status}"));
    }
    let response: ApiRelease = serde_json::from_slice(&bytes)?;
    if response.draft || response.prerelease {
        return Err(anyhow!("정식 릴리스가 아닙니다"));
    }
    let version = response
        .tag_name
        .strip_prefix('v')
        .ok_or_else(|| anyhow!("잘못된 릴리스 태그입니다"))?;
    if Version::parse(version)? <= Version::parse(VERSION)? {
        return Ok(State::Current);
    }
    let exe = std::env::current_exe()?;
    Ok(State::Available(Release::from_api(
        response,
        package_name(&exe),
    )?))
}

fn get(url: &str, limit: usize, time: Duration, output: &mut impl Write) -> Result<u16> {
    let mut request = Request::get(url)
        .header("User-Agent", concat!("Edot/", env!("EDOT_APP_VERSION")))
        .header("Accept", "application/vnd.github+json")
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
    fn github_assets_require_valid_versions_sizes_and_sha256() {
        let valid = serde_json::json!({"tag_name":"v0.0.2","draft":false,"prerelease":false,
            "assets":[{"name":"edot-windows.zip","size":123,"digest":format!("sha256:{}", "a".repeat(64))}]});
        let parse = |value: serde_json::Value| {
            Release::from_api(serde_json::from_value(value).unwrap(), "edot-windows.zip")
        };
        let release = parse(valid.clone()).unwrap();
        assert!(
            release
                .installer_url()
                .ends_with("/v0.0.2/edot-windows.zip")
        );
        for field in ["draft", "prerelease"] {
            let mut value = valid.clone();
            value[field] = true.into();
            assert!(parse(value).is_err());
        }
        for tag in ["v01.0.0", "v1.0.0-beta", "v1.0.65536", "1.0.0"] {
            let mut value = valid.clone();
            value["tag_name"] = tag.into();
            assert!(parse(value).is_err());
        }
        for size in [0, 268435457] {
            let mut value = valid.clone();
            value["assets"][0]["size"] = size.into();
            assert!(parse(value).is_err());
        }
        for digest in [
            serde_json::Value::Null,
            "sha256:bad".into(),
            "md5:abc".into(),
        ] {
            let mut value = valid.clone();
            value["assets"][0]["digest"] = digest;
            assert!(parse(value).is_err());
        }
        let mut missing = valid;
        missing["assets"] = serde_json::json!([]);
        assert!(parse(missing).is_err());
    }
}
