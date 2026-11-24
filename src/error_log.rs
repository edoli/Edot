//! Local diagnostics for application errors and Rust panics.
use std::{
    backtrace::Backtrace,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    panic,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const FILE_NAME: &str = "error.log";
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn install() {
    if LOG_PATH
        .set(crate::space::config_dir().join(FILE_NAME))
        .is_err()
    {
        return;
    }

    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("unnamed");
        let backtrace = Backtrace::force_capture();
        record(
            "PANIC",
            format_args!("thread {thread_name}: {info}\nBacktrace:\n{backtrace}"),
        );
        previous(info);
    }));
}

#[allow(unused)]
pub(crate) fn error(message: fmt::Arguments<'_>) {
    if LOG_PATH.get().is_some() {
        record("ERROR", message);
        let _ = writeln!(io::stderr().lock(), "{message}");
    }
}

fn record(level: &str, message: fmt::Arguments<'_>) {
    let Some(path) = LOG_PATH.get() else {
        return;
    };
    let time = crate::datetime::current(true).unwrap_or_else(|_| "unknown time".into());
    let entry = format!(
        "[{time}] {level} Edot {} (pid {})\n{message}\n\n",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
    );

    if let Err(error) = append(path, &entry) {
        #[cfg(windows)]
        if append(&std::env::temp_dir().join("Edot").join(FILE_NAME), &entry).is_ok() {
            return;
        }
        let _ = writeln!(
            io::stderr().lock(),
            "Could not write {}: {error}",
            path.display()
        );
    }
}

fn append(path: &Path, entry: &str) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(entry.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{process::Command, time::SystemTime};

    #[test]
    fn writes_errors_and_panics_to_the_configuration_directory() {
        if let Some(dir) = std::env::var_os("EDOT_TEST_ERROR_LOG_DIR") {
            install();
            assert!(!PathBuf::from(dir).exists());
            error(format_args!("controlled application error"));
            panic!("controlled diagnostic panic");
        }

        let dir = std::env::temp_dir().join(format!(
            "edot-error-log-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "error_log::tests::writes_errors_and_panics_to_the_configuration_directory",
            ])
            .env("EDOT_TEST_ERROR_LOG_DIR", &dir)
            .env("EDOT_CONFIG_DIR", &dir)
            .output()
            .unwrap();
        assert!(!child.status.success());
        let path = dir.join(FILE_NAME);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("ERROR Edot"));
        assert!(contents.contains("controlled application error"));
        assert!(contents.contains("PANIC Edot"));
        assert!(contents.contains("error_log.rs:"));
        assert!(contents.contains("controlled diagnostic panic"));
        assert!(contents.contains("Backtrace:\n"));
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
