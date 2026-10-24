//! Windows installation discovery and installer handoff.
use super::*;
use std::{
    fs::{self, OpenOptions},
    io,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::PathBuf,
    process::{Child, Command},
    time::Instant,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};

const HELPER_SCRIPT: &str = include_str!("../../installer/update.ps1");

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
        if self.child.as_mut().unwrap().try_wait()?.is_some() {
            return Err(io::Error::other("업데이트 도우미가 종료되었습니다"));
        }
        fs::write(self.directory.join("go.pending"), b"install")?;
        fs::rename(self.directory.join("go.pending"), self.directory.join("go"))?;
        self.committed = true;
        Ok(())
    }
}

pub fn prepare(
    release: &Release,
    exe: &Path,
    scope: Option<InstallScope>,
    space: &Path,
    config: &Path,
) -> Result<Handoff> {
    if install_scope(exe) != scope {
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
    let installer = handoff.directory.join(if scope.is_some() {
        "setup.exe"
    } else {
        "package.zip"
    });
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
            if scope == Some(InstallScope::Machine) {
                "machine"
            } else if scope.is_some() {
                "user"
            } else {
                "portable"
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
