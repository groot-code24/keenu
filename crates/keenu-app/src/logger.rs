// SPDX-License-Identifier: GPL-3.0-or-later

//! Keenu's log: a small file in its data folder, for bug reports. Each line goes through the redactor first,
//! and what the user copies, says or is asked about is never logged in the first place.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

pub const FILE_NAME: &str = "keenu.log";
/// Past this the file is started again, with one older file kept.
const MAX_BYTES: u64 = 1024 * 1024;

struct FileLogger {
    file: Mutex<Option<(File, u64)>>,
    path: PathBuf,
    level: LevelFilter,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level && metadata.target().starts_with("keenu")
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let line = keenu_core::redact::redact(&format!(
            "{stamp} {} {}\n",
            level_name(record.level()),
            record.args()
        ));
        if let Some((file, written)) = guard.as_mut() {
            if file.write_all(line.as_bytes()).is_ok() {
                *written += line.len() as u64;
            }
            if *written > MAX_BYTES {
                let _ = fs::rename(&self.path, self.path.with_extension("log.old"));
                *guard = open(&self.path);
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock()
            && let Some((file, _)) = guard.as_mut()
        {
            let _ = file.flush();
        }
    }
}

fn level_name(level: Level) -> &'static str {
    match level {
        Level::Error => "ERROR",
        Level::Warn => "WARN",
        Level::Info => "INFO",
        Level::Debug => "DEBUG",
        Level::Trace => "TRACE",
    }
}

fn open(path: &Path) -> Option<(File, u64)> {
    if let Some(parent) = path.parent() {
        keenu_core::json_file::create_private_dir(parent).ok()?;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).ok()?;
    let written = file.metadata().map_or(0, |metadata| metadata.len());
    Some((file, written))
}

/// Starts logging to `dir`. `KEENU_LOG` chooses how much: `error`, `warn` (the default), `info` or `debug`.
pub fn init(dir: &Path) {
    let level = match std::env::var("KEENU_LOG").as_deref() {
        Ok("error") => LevelFilter::Error,
        Ok("info") => LevelFilter::Info,
        Ok("debug") => LevelFilter::Debug,
        Ok("off") => LevelFilter::Off,
        _ => LevelFilter::Warn,
    };
    let path = dir.join(FILE_NAME);
    let logger = FileLogger {
        file: Mutex::new(open(&path)),
        path,
        level,
    };
    // Lives as long as the program; this runs once.
    if log::set_logger(Box::leak(Box::new(logger))).is_ok() {
        log::set_max_level(level);
    }
}
