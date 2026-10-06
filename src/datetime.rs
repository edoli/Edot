use std::io;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::Globalization::{
    DATE_SHORTDATE, GetDateFormatEx, GetTimeFormatEx, TIME_NOSECONDS,
};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

pub fn current(include_time: bool) -> io::Result<String> {
    let mut time = SYSTEMTIME::default();
    unsafe {
        GetLocalTime(&mut time);
    }
    format_at(&time, include_time)
}

fn read_formatted(call: impl Fn(*mut u16, i32) -> i32) -> io::Result<String> {
    let required = call(null_mut(), 0);
    if required <= 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0u16; required as usize];
    let written = call(buffer.as_mut_ptr(), required);
    if written <= 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(String::from_utf16_lossy(&buffer[..written as usize - 1]))
}

fn format_at(time: &SYSTEMTIME, include_time: bool) -> io::Result<String> {
    let date = read_formatted(|buffer, length| unsafe {
        GetDateFormatEx(null(), DATE_SHORTDATE, time, null(), buffer, length, null())
    })?;
    if !include_time {
        return Ok(date);
    }
    let clock = read_formatted(|buffer, length| unsafe {
        GetTimeFormatEx(null(), TIME_NOSECONDS, time, null(), buffer, length)
    })?;
    Ok(format!("{clock} {date}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_and_time_share_one_local_timestamp_and_omit_seconds() {
        let mut time = SYSTEMTIME {
            wYear: 2026,
            wMonth: 10,
            wDay: 6,
            wHour: 13,
            wMinute: 4,
            wSecond: 59,
            ..Default::default()
        };
        let date = format_at(&time, false).unwrap();
        let timestamp = format_at(&time, true).unwrap();
        assert!(!date.is_empty());
        assert!(timestamp.ends_with(&format!(" {date}")));
        assert!(timestamp.len() > date.len() + 1);
        time.wSecond = 0;
        assert_eq!(format_at(&time, true).unwrap(), timestamp);
        assert_eq!(format_at(&time, false).unwrap(), date);
        assert!(!timestamp.contains(['\r', '\n', '\0']));
    }
}
