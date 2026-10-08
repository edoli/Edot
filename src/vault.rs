//! Vault selection and device-wide configuration paths.
use crate::storage::replace_config_file;
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Paths {
    pub vault: PathBuf,
    pub config: PathBuf,
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("EDOT_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if let Some(path) = std::env::var_os("APPDATA") {
                PathBuf::from(path).join("Edot")
            } else if let Some(path) = std::env::var_os("LOCALAPPDATA") {
                PathBuf::from(path).join("EdotConfig")
            } else {
                PathBuf::from("EdotConfig")
            }
        })
}

impl Paths {
    pub fn resolve() -> io::Result<Self> {
        let explicit = std::env::var_os("EDOT_VAULT_DIR")
            .or_else(|| std::env::var_os("EDOT_DATA_DIR"))
            .map(PathBuf::from);
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self::select(
            std::env::args_os().skip(1),
            config_dir(),
            explicit,
            local.join("Edot"),
        )
    }

    fn select(
        args: impl IntoIterator<Item = OsString>,
        config: PathBuf,
        preferred: Option<PathBuf>,
        legacy: PathBuf,
    ) -> io::Result<Self> {
        let mut args = args.into_iter();
        let mut cli = None;
        while let Some(arg) = args.next() {
            if arg == "--vault" {
                cli = Some(PathBuf::from(
                    args.next().filter(|path| !path.is_empty()).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "--vault 뒤에 폴더 경로가 필요합니다.",
                        )
                    })?,
                ));
            }
        }
        let vault = if let Some(path) = cli.or(preferred) {
            path
        } else {
            match std::fs::read_to_string(config.join("last-vault.txt")) {
                Ok(path) => {
                    let path = PathBuf::from(path);
                    if !path.is_dir() {
                        return Err(io::Error::new(
                            io::ErrorKind::NotFound,
                            "마지막 vault를 찾을 수 없습니다. --vault로 폴더를 지정하세요.",
                        ));
                    }
                    path
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if legacy.join("tree.tsv").try_exists()? || legacy.join("notes").is_dir() {
                        legacy
                    } else {
                        legacy.join("vaults/Default")
                    }
                }
                Err(error) => return Err(error),
            }
        };
        Ok(Self { vault, config })
    }

    pub fn remember(&self) -> io::Result<()> {
        let path = std::fs::canonicalize(&self.vault)?;
        let path = path.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "vault 경로를 저장할 수 없습니다.",
            )
        })?;
        replace_config_file(&self.config.join("last-vault.txt"), path.as_bytes())
    }
}

pub fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "edot-vault-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        root
    }
    #[test]
    fn chooses_cli_then_environment_then_remembered_vault() {
        let root = fixture();
        let config = root.join("config");
        let legacy = root.join("old");
        let env = root.join("env");
        let cli = root.join("cli 한글");
        let choose =
            |args, preferred| Paths::select(args, config.clone(), preferred, legacy.clone());
        let selected = choose(
            vec!["--vault".into(), cli.clone().into_os_string()],
            Some(env.clone()),
        )
        .unwrap();
        assert_eq!(selected.vault, cli);
        std::fs::create_dir(&cli).unwrap();
        selected.remember().unwrap();
        assert_eq!(choose(vec![], Some(env.clone())).unwrap().vault, env);
        assert_eq!(
            choose(vec![], None).unwrap().vault,
            std::fs::canonicalize(&cli).unwrap()
        );
        assert!(choose(vec!["--vault".into()], None).is_err());
        std::fs::write(
            config.join("last-vault.txt"),
            root.join("missing").to_str().unwrap(),
        )
        .unwrap();
        assert!(choose(vec![], None).is_err());
        assert_eq!(choose(vec![], Some(cli.clone())).unwrap().vault, cli);
    }
    #[test]
    fn preserves_existing_store_and_places_new_default_in_vaults() {
        let root = fixture();
        let config = root.join("config");
        let legacy = root.join("old");
        assert_eq!(
            Paths::select(vec![], config.clone(), None, legacy.clone())
                .unwrap()
                .vault,
            legacy.join("vaults/Default")
        );
        std::fs::create_dir_all(legacy.join("notes")).unwrap();
        assert_eq!(
            Paths::select(vec![], config, None, legacy.clone())
                .unwrap()
                .vault,
            legacy
        );
    }
}
