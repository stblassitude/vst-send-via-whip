//! A small append-only log file shared by all streams in the process, plus
//! forwarding to the `log` facade. Tokens must never be passed in here.

use std::fmt::Display;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

static FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

/// Directory for log files, following each platform's convention.
pub fn log_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Logs/WhipSend"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("WhipSend").join("logs"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .map(|d| d.join("whip-send"))
    }
}

pub fn log_file_path() -> Option<PathBuf> {
    log_dir().map(|d| d.join("whip-send.log"))
}

fn open_file() -> Option<File> {
    let path = log_file_path()?;
    fs::create_dir_all(path.parent()?).ok()?;
    if fs::metadata(&path)
        .map(|m| m.len() > MAX_LOG_BYTES)
        .unwrap_or(false)
    {
        let _ = fs::rename(&path, path.with_extension("log.1"));
    }
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// Per-stream logger. Lines are prefixed with the stream label.
#[derive(Clone)]
pub struct EventLog {
    label: String,
    to_file: bool,
}

impl EventLog {
    pub fn new(label: impl Into<String>, to_file: bool) -> Self {
        Self {
            label: label.into(),
            to_file,
        }
    }

    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    pub fn info(&self, msg: impl Display) {
        self.write(log::Level::Info, msg);
    }

    pub fn warn(&self, msg: impl Display) {
        self.write(log::Level::Warn, msg);
    }

    fn write(&self, level: log::Level, msg: impl Display) {
        log::log!(target: "whip_send", level, "[{}] {}", self.label, msg);
        if !self.to_file {
            return;
        }
        let file = FILE.get_or_init(|| Mutex::new(open_file()));
        if let Ok(mut guard) = file.lock() {
            if let Some(f) = guard.as_mut() {
                let line = format!("{} {:5} [{}] {}\n", timestamp(), level, self.label, msg);
                let _ = f.write_all(line.as_bytes());
                if f.metadata()
                    .map(|m| m.len() > MAX_LOG_BYTES)
                    .unwrap_or(false)
                {
                    *guard = None;
                    *guard = open_file();
                }
            }
        }
    }
}

/// UTC timestamp in RFC 3339 format with milliseconds.
fn timestamp() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (y, m, day) = civil_from_days(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        s / 3600,
        s / 60 % 60,
        s % 60,
        d.subsec_millis()
    )
}

// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_747), (2026, 10, 21));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }
}
