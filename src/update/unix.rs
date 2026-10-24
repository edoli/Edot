//! Download a native package and prepare a helper before the editor saves and exits.
use super::*;
use std::{
    fs::{self, OpenOptions},
    io,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Instant,
};

pub struct Handoff {
    directory: PathBuf,
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

pub fn prepare(release: &Release, exe: &Path, space: &Path, config: &Path) -> Result<Handoff> {
    let executable = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| exe.to_owned());
    let executable = std::path::absolute(executable)?;
    let space = std::path::absolute(space)?;
    let config = std::path::absolute(config)?;
    let mut random = [0; 16];
    getrandom::fill(&mut random)
        .map_err(|error| anyhow!("임시 폴더를 만들 수 없습니다: {error}"))?;
    let name: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let directory = std::env::temp_dir().join(format!("edot-update-{name}"));
    fs::create_dir(&directory)?;
    let mut handoff = Handoff {
        directory,
        child: None,
        committed: false,
    };
    let package = handoff.directory.join(&release.asset_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&package)?;
    let status = get(
        &release.installer_url(),
        release.size as usize,
        Duration::from_secs(300),
        &mut file,
    )?;
    file.sync_all()?;
    if status != 200 || file.metadata()?.len() != release.size {
        return Err(anyhow!(
            "업데이트 파일 다운로드가 완전하지 않습니다 (응답 {status})"
        ));
    }
    drop(file);
    fs::write(
        handoff.directory.join("update.sh"),
        include_str!("../../installer/update.sh"),
    )?;
    // All restart paths are shell-quoted data and the active Space remains inherited.
    fs::write(
        handoff.directory.join("restart.sh"),
        format!("#!/bin/sh\nexec {}\n", sh_quote(&executable)?),
    )?;
    let child = Command::new("sh")
        .arg(handoff.directory.join("update.sh"))
        .env("EDOT_UPDATE_DIR", &handoff.directory)
        .env("EDOT_UPDATE_EXE", &executable)
        .env("EDOT_UPDATE_PACKAGE", &package)
        .env("EDOT_UPDATE_VERSION", &release.version)
        .env("EDOT_UPDATE_HASH", &release.sha256)
        .env("EDOT_UPDATE_PARENT", std::process::id().to_string())
        .env("EDOT_SPACE_DIR", space)
        .env("EDOT_CONFIG_DIR", &config)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    handoff.child = Some(child);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if handoff.child.as_mut().unwrap().try_wait()?.is_some() {
            return Err(anyhow!(
                read_result(&config)
                    .unwrap_or_else(|| "업데이트 도우미를 시작하지 못했습니다".into())
            ));
        }
        if handoff.directory.join("ready").is_file() {
            return Ok(handoff);
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("업데이트 도우미 응답 시간이 초과되었습니다"));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn sh_quote(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| anyhow!("업데이트 경로를 문자열로 변환하지 못했습니다"))?;
    Ok(format!("'{}'", path.replace('\'', "'\\''")))
}
