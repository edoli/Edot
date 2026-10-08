//! Space selection and device-wide configuration paths.
use crate::storage::replace_config_file;
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Paths {
    pub space: PathBuf,
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
        let explicit = std::env::var_os("EDOT_SPACE_DIR").map(PathBuf::from);
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self::select(
            std::env::args_os().skip(1),
            config_dir(),
            explicit,
            local.join("Edot/spaces/Default"),
        )
    }

    fn select(
        args: impl IntoIterator<Item = OsString>,
        config: PathBuf,
        preferred: Option<PathBuf>,
        default_space: PathBuf,
    ) -> io::Result<Self> {
        let mut args = args.into_iter();
        let mut cli = None;
        while let Some(arg) = args.next() {
            if arg == "--space" {
                cli = Some(PathBuf::from(
                    args.next().filter(|path| !path.is_empty()).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "--space 뒤에 폴더 경로가 필요합니다.",
                        )
                    })?,
                ));
            }
        }
        let space = if let Some(path) = cli.or(preferred) {
            path
        } else {
            match std::fs::read_to_string(config.join("last-space.txt")) {
                Ok(path) => {
                    let path = PathBuf::from(path);
                    if !path.is_dir() {
                        return Err(io::Error::new(
                            io::ErrorKind::NotFound,
                            "마지막 space를 찾을 수 없습니다. --space로 폴더를 지정하세요.",
                        ));
                    }
                    path
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => default_space,
                Err(error) => return Err(error),
            }
        };
        Ok(Self { space, config })
    }

    pub fn remember(&self) -> io::Result<()> {
        let path = std::fs::canonicalize(&self.space)?;
        let path = path.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "space 경로를 저장할 수 없습니다.",
            )
        })?;
        replace_config_file(&self.config.join("last-space.txt"), path.as_bytes())
    }
}

pub fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

const METADATA_FILE: &str = "space.ini";

fn metadata(path: &Path) -> io::Result<crate::settings::ini::Document> {
    let text = match std::fs::read_to_string(path.join(METADATA_FILE)) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let document = crate::settings::ini::Document::parse(&text)?;
    if document
        .value("version")
        .is_some_and(|version| version != "1")
    {
        return Err(io::Error::other("지원하지 않는 Space 정보 버전입니다."));
    }
    Ok(document)
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && !name.chars().any(char::is_control)
}

pub fn read_name(path: &Path) -> io::Result<String> {
    let document = metadata(path)?;
    match document.value("name") {
        Some(value) if valid_name(value) => Ok(value.trim().into()),
        Some(_) => Err(io::Error::other("Space 이름이 잘못되었습니다.")),
        None => Ok(name(path)),
    }
}

pub fn rename(path: &Path, name: &str) -> io::Result<()> {
    if !valid_name(name) {
        return Err(io::Error::other(
            "Space 이름은 비어 있거나 제어 문자를 포함할 수 없습니다.",
        ));
    }
    // Require an existing Space, so a missing synced folder is never recreated.
    if !path.is_dir() {
        return Err(io::Error::other("Space 폴더가 존재하지 않습니다."));
    }
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.join(".space.lock"))?;
    lock.lock()?;
    let document = metadata(path)?;
    let quoted = name.trim().replace('\\', "\\\\").replace('"', "\\\"");
    let text =
        document.updated_values(&[("version", "1".into()), ("name", format!("\"{quoted}\""))]);
    replace_config_file(&path.join(METADATA_FILE), text.as_bytes())
}

pub fn initialize_metadata(path: &Path) -> io::Result<()> {
    let name = read_name(path)?;
    if !path.join(METADATA_FILE).try_exists()? {
        rename(path, &name)?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
}

#[derive(Default, Clone, Debug)]
pub struct Registry {
    pub entries: Vec<Entry>,
}

pub fn same_path(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_owned());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_owned());
    left.as_os_str().to_string_lossy().to_lowercase()
        == right.as_os_str().to_string_lossy().to_lowercase()
}

impl Registry {
    const FILE: &str = "spaces.tsv";

    pub fn load(config: &Path) -> io::Result<Self> {
        let text = match std::fs::read_to_string(config.join(Self::FILE)) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let mut lines = text.lines();
        if lines.next() != Some("EDOTSPACES1") {
            return Err(io::Error::other("Space 목록 형식이 잘못되었습니다."));
        }
        let mut entries = Vec::new();
        for line in lines {
            if line.is_empty() || line.chars().any(char::is_control) {
                return Err(io::Error::other("Space 목록 항목이 잘못되었습니다."));
            }
            let path = PathBuf::from(line);
            if !path.is_absolute()
                || entries
                    .iter()
                    .any(|entry: &Entry| same_path(&entry.path, &path))
            {
                return Err(io::Error::other("Space 경로가 잘못되거나 중복되었습니다."));
            }
            entries.push(Entry {
                name: read_name(&path).unwrap_or_else(|_| name(&path)),
                path,
            });
        }
        Ok(Self { entries })
    }

    pub fn initialize(config: &Path, current: &Path) -> io::Result<Self> {
        let _lock = Self::lock(config)?;
        if config.join(Self::FILE).try_exists()? {
            return Self::load(config);
        }
        let mut registry = Self::default();
        registry.add(current)?;
        registry.save(config)?;
        Ok(registry)
    }

    pub fn register(config: &Path, path: &Path) -> io::Result<Self> {
        Self::edit(config, |registry| registry.add(path))
    }

    pub fn remove(config: &Path, path: &Path) -> io::Result<Self> {
        Self::edit(config, |registry| {
            registry
                .entries
                .retain(|entry| !same_path(&entry.path, path));
            Ok(())
        })
    }

    fn add(&mut self, path: &Path) -> io::Result<()> {
        let path = std::fs::canonicalize(path)?;
        if !self
            .entries
            .iter()
            .any(|entry| same_path(&entry.path, &path))
        {
            self.entries.push(Entry {
                name: read_name(&path)?,
                path,
            });
        }
        Ok(())
    }

    fn lock(config: &Path) -> io::Result<std::fs::File> {
        std::fs::create_dir_all(config)?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(config.join(".spaces.lock"))?;
        lock.lock()?;
        Ok(lock)
    }

    fn edit(config: &Path, edit: impl FnOnce(&mut Self) -> io::Result<()>) -> io::Result<Self> {
        let _lock = Self::lock(config)?;
        let mut registry = Self::load(config)?;
        edit(&mut registry)?;
        registry.save(config)?;
        Ok(registry)
    }

    fn save(&self, config: &Path) -> io::Result<()> {
        let mut text = String::from("EDOTSPACES1\n");
        for entry in &self.entries {
            let path = entry
                .path
                .to_str()
                .filter(|path| !path.chars().any(char::is_control))
                .ok_or_else(|| io::Error::other("Space 경로를 저장할 수 없습니다."))?;
            text.push_str(path);
            text.push('\n');
        }
        replace_config_file(&config.join(Self::FILE), text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "edot-space-{}-{}",
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
    fn chooses_cli_then_environment_then_remembered_space() {
        let root = fixture();
        let config = root.join("config");
        let default_space = root.join("spaces/Default");
        let env = root.join("env");
        let cli = root.join("cli 한글");
        let choose =
            |args, preferred| Paths::select(args, config.clone(), preferred, default_space.clone());
        assert_eq!(choose(vec![], None).unwrap().space, default_space);
        let selected = choose(
            vec!["--space".into(), cli.clone().into_os_string()],
            Some(env.clone()),
        )
        .unwrap();
        assert_eq!(selected.space, cli);
        std::fs::create_dir(&cli).unwrap();
        selected.remember().unwrap();
        assert_eq!(choose(vec![], Some(env.clone())).unwrap().space, env);
        assert_eq!(
            choose(vec![], None).unwrap().space,
            std::fs::canonicalize(&cli).unwrap()
        );
        assert!(choose(vec!["--space".into()], None).is_err());
        std::fs::write(
            config.join("last-space.txt"),
            root.join("missing").to_str().unwrap(),
        )
        .unwrap();
        assert!(choose(vec![], None).is_err());
        assert_eq!(choose(vec![], Some(cli.clone())).unwrap().space, cli);
    }

    #[test]
    fn space_names_live_in_the_space_and_registry_removal_preserves_data() {
        let root = fixture();
        let first = root.join("first");
        let second = root.join("second");
        let config = root.join("config");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        std::fs::write(first.join("memo.txt"), "keep me").unwrap();
        initialize_metadata(&first).unwrap();
        assert!(first.join("space.ini").is_file());
        std::fs::write(
            first.join(METADATA_FILE),
            "# keep\r\nversion = 1\r\nname = \"first\" # label\r\ncustom = yes\r\n",
        )
        .unwrap();
        let original = Registry::initialize(&config, &first).unwrap();
        assert_eq!(original.entries.len(), 1);
        assert!(
            std::fs::read_to_string(config.join("spaces.tsv"))
                .unwrap()
                .starts_with("EDOTSPACES1\n")
        );
        Registry::register(&config, &second).unwrap();
        Registry::register(&config, &first.join(".")).unwrap();
        let label = r#"한글 \ "이름" # ; 🙂"#;
        rename(&first, label).unwrap();
        let metadata = std::fs::read_to_string(first.join(METADATA_FILE)).unwrap();
        assert!(metadata.contains("# keep\r\n"));
        assert!(metadata.contains("# label\r\ncustom = yes\r\n"));
        assert_eq!(read_name(&first).unwrap(), label);
        let registry = Registry::load(&config).unwrap();
        assert_eq!(registry.entries.len(), 2);
        assert_eq!(registry.entries[0].name, label);
        assert!(
            !std::fs::read_to_string(config.join(Registry::FILE))
                .unwrap()
                .contains(label)
        );
        let other_config = root.join("other-device");
        assert_eq!(
            Registry::initialize(&other_config, &first).unwrap().entries[0].name,
            label
        );
        assert!(rename(&first, "bad\nname").is_err());
        assert_eq!(read_name(&first).unwrap(), label);
        Registry::remove(&config, &first).unwrap();
        assert_eq!(Registry::load(&config).unwrap().entries.len(), 1);
        assert_eq!(
            std::fs::read_to_string(first.join("memo.txt")).unwrap(),
            "keep me"
        );
        assert_eq!(read_name(&first).unwrap(), label);
        // A stale in-memory list cannot overwrite another process's registration.
        Registry::remove(&config, &first).unwrap();
        assert_eq!(
            Registry::load(&config).unwrap().entries[0].path,
            std::fs::canonicalize(&second).unwrap()
        );
        std::fs::write(first.join(METADATA_FILE), "version = 9\nname = old\n").unwrap();
        assert!(rename(&first, "new").is_err());
        assert_eq!(
            std::fs::read_to_string(first.join(METADATA_FILE)).unwrap(),
            "version = 9\nname = old\n"
        );
        std::fs::write(config.join(Registry::FILE), "bad list").unwrap();
        assert!(Registry::register(&config, &first).is_err());
        assert_eq!(
            std::fs::read_to_string(config.join(Registry::FILE)).unwrap(),
            "bad list"
        );
    }
}
