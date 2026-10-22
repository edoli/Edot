use std::{io, mem::MaybeUninit, ptr};

pub fn current(include_time: bool) -> io::Result<String> {
    // SAFETY: A null argument asks time() to return the timestamp without writing.
    let timestamp = unsafe { libc::time(ptr::null_mut()) };
    if timestamp == -1 {
        return Err(io::Error::last_os_error());
    }
    format_at(timestamp, include_time)
}

fn format_at(timestamp: libc::time_t, include_time: bool) -> io::Result<String> {
    let mut local = MaybeUninit::<libc::tm>::uninit();
    // SAFETY: The input is valid and the output points to writable tm storage.
    // localtime_r uses caller-owned storage rather than a shared static buffer.
    if unsafe { libc::localtime_r(&timestamp, local.as_mut_ptr()) }.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: A successful localtime_r initialized the complete tm value.
    let local = unsafe { local.assume_init() };
    let date = format!(
        "{:04}-{:02}-{:02}",
        i64::from(local.tm_year) + 1900,
        local.tm_mon + 1,
        local.tm_mday
    );
    if include_time {
        Ok(format!("{:02}:{:02} {date}", local.tm_hour, local.tm_min))
    } else {
        Ok(date)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn converts_timezone_and_dst_without_changing_parent_environment() {
        for timezone in ["UTC0", "KST-9", "EST5EDT,M3.2.0,M11.1.0"] {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "datetime::unix::tests::timezone_probe",
                    "--nocapture",
                ])
                .env("TZ", timezone)
                .env("EDOT_TEST_DATETIME_TZ", timezone)
                .output()
                .unwrap();
            assert!(output.status.success(), "{timezone}: {output:?}");
        }
    }

    #[test]
    fn timezone_probe() {
        let Ok(timezone) = std::env::var("EDOT_TEST_DATETIME_TZ") else {
            return;
        };
        // 2024-02-29 23:45:59 UTC, including a local date rollover in Korea.
        let leap_day = 1_709_250_359;
        let expected = match timezone.as_str() {
            "UTC0" => "23:45 2024-02-29",
            "KST-9" => "08:45 2024-03-01",
            _ => "18:45 2024-02-29",
        };
        assert_eq!(format_at(leap_day, true).unwrap(), expected);
        assert_eq!(format_at(leap_day, false).unwrap(), &expected[6..]);
        assert_eq!(format_at(leap_day - 59, true).unwrap(), expected);
        // US daylight-saving transition: 2024-03-10 07:00 UTC.
        if timezone.starts_with("EST") {
            assert_eq!(format_at(1_710_053_999, true).unwrap(), "01:59 2024-03-10");
            assert_eq!(format_at(1_710_054_000, true).unwrap(), "03:00 2024-03-10");
        }
        assert!(!current(true).unwrap().is_empty());
    }
}
