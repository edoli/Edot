use crate::model::replace_file;
use std::io;
use std::path::Path;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Registry::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemeMode,
    pub font: String,
    pub font_size: u32,
    pub wrap: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            font: "Consolas".into(),
            font_size: 18,
            wrap: true,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> io::Result<Self> {
        let mut lines = text.lines();
        if lines.next() != Some("EDOTSETTINGS1") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "지원하지 않는 설정 형식입니다.",
            ));
        }
        let mut settings = Self::default();
        for line in lines {
            let Some((key, value)) = line.split_once('\t') else {
                continue;
            };
            match (key, value) {
                ("theme", "light") => settings.theme = ThemeMode::Light,
                ("theme", "dark") => settings.theme = ThemeMode::Dark,
                ("theme", "system") => settings.theme = ThemeMode::System,
                ("font", font) if valid_font(font) => settings.font = font.into(),
                ("font_size", size) => {
                    if let Ok(size @ 8..=72) = size.parse::<u32>() {
                        settings.font_size = size;
                    }
                }
                ("wrap", "1") => settings.wrap = true,
                ("wrap", "0") => settings.wrap = false,
                _ => {}
            }
        }
        Ok(settings)
    }

    pub fn load(dir: &Path) -> io::Result<Self> {
        match std::fs::read_to_string(dir.join("settings.tsv")) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        let mode = match self.theme {
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
            ThemeMode::System => "system",
        };
        replace_file(
            &dir.join("settings.tsv"),
            format!(
                "EDOTSETTINGS1\ntheme\t{mode}\nfont\t{}\nfont_size\t{}\nwrap\t{}\n",
                self.font, self.font_size, self.wrap as u8
            )
            .as_bytes(),
        )
    }

    pub fn dark(&self) -> bool {
        match self.theme {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::System => system_dark(),
        }
    }
}

fn valid_font(font: &str) -> bool {
    !font.trim().is_empty()
        && font.encode_utf16().count() < 32
        && !font.chars().any(char::is_control)
}

fn system_dark() -> bool {
    let path: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let name: Vec<u16> = "AppsUseLightTheme".encode_utf16().chain(Some(0)).collect();
    let mut light = 1u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            path.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut light as *mut _ as *mut _,
            &mut size,
        );
    }
    light == 0
}

/// Called only on the first visit to settings; no font enumeration at startup.
pub unsafe fn installed_fonts(hwnd: HWND) -> Vec<String> {
    unsafe extern "system" fn collect(
        font: *const LOGFONTW,
        _: *const TEXTMETRICW,
        _: u32,
        data: LPARAM,
    ) -> i32 {
        let face = unsafe { &(*font).lfFaceName };
        let len = face.iter().position(|&c| c == 0).unwrap_or(face.len());
        let name = String::from_utf16_lossy(&face[..len]);
        if valid_font(&name) && !name.starts_with('@') {
            unsafe { &mut *(data as *mut Vec<String>) }.push(name);
        }
        1
    }
    let mut names = Vec::new();
    unsafe {
        let dc = GetDC(hwnd);
        if !dc.is_null() {
            let font = LOGFONTW {
                lfCharSet: DEFAULT_CHARSET,
                ..Default::default()
            };
            EnumFontFamiliesExW(dc, &font, Some(collect), &mut names as *mut _ as LPARAM, 0);
            ReleaseDC(hwnd, dc);
        }
    }
    names.sort_unstable();
    names.dedup();
    names
}

#[derive(Clone, Copy)]
pub struct Panel {
    pub host: HWND,
    pub theme: HWND,
    pub font: HWND,
    pub size: HWND,
    pub wrap: HWND,
    pub close: HWND,
    pub theme_label: HWND,
    pub font_label: HWND,
    pub size_label: HWND,
}

impl Panel {
    pub fn dropdown(self, id: usize) -> Option<HWND> {
        [self.theme, self.font, self.size]
            .into_iter()
            .find(|&control| unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(control) as usize == id
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_atomic_replace() {
        let dir = std::env::temp_dir().join(format!("edot-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(Settings::load(&dir).unwrap(), Settings::default());
        let mut settings = Settings {
            theme: ThemeMode::Dark,
            font: "맑은 고딕".into(),
            font_size: 24,
            wrap: false,
        };
        settings.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir).unwrap(), settings);
        settings.theme = ThemeMode::Light;
        settings.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir).unwrap(), settings);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_values_fall_back_but_unknown_version_is_rejected() {
        let text = format!(
            "EDOTSETTINGS1\ntheme\tunknown\nwrap\tbad\nfont\t{}\nfuture\t1\n",
            "a".repeat(32)
        );
        assert_eq!(Settings::parse(&text).unwrap(), Settings::default());
        assert!(Settings::parse("EDOTSETTINGS2\n").is_err());
        assert!(Settings::parse("garbage").is_err());
        assert!(!valid_font("bad\tfont"));
        assert!(!valid_font("\0"));
        assert!(!valid_font(" "));
        assert_eq!(
            Settings::parse("EDOTSETTINGS1\nfont\tArial\n")
                .unwrap()
                .font_size,
            18
        );
        for size in ["7", "73", "0", "-1", "bad", "4294967296"] {
            assert_eq!(
                Settings::parse(&format!("EDOTSETTINGS1\nfont_size\t{size}\n"))
                    .unwrap()
                    .font_size,
                18
            );
        }
    }
}
