// SPDX-License-Identifier: GPL-3.0-or-later

//! The action log: everything the assistant did, was refused or was told not to do, a line per action, on this
//! computer only. Secrets are masked before a line is written, so the log can be read or shared without worry.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::level::Level;

/// The log starts over, keeping its newer half, once it passes this.
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// No one line is longer than this.
const MAX_TEXT: usize = 2_000;

/// How an action went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Done,
    Failed,
    Denied,
    Refused,
    Approved,
}

/// One line of the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub at_ms: i64,
    pub request: String,
    pub level: Level,
    /// What kind of action it was, as the assistant names it (`open_app`); empty in lines from before it was kept.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    pub action: String,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// The log's file.
#[derive(Debug, Clone)]
pub struct ActionLog {
    path: PathBuf,
}

/// `text` with secrets masked, and cut to a size.
fn clean(text: &str) -> String {
    let masked = keenu_core::redact::redact(text);
    if masked.chars().count() <= MAX_TEXT {
        return masked;
    }
    let mut cut: String = masked.chars().take(MAX_TEXT).collect();
    cut.push('…');
    cut
}

impl ActionLog {
    /// The log at `path`, made when first written.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Where it is, to show the user.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Adds `entry`. A log that cannot be written stops the action: nothing runs unrecorded.
    pub fn add(&self, entry: &Entry) -> Result<(), String> {
        let entry = Entry {
            request: clean(&entry.request),
            action: clean(&entry.action),
            detail: clean(&entry.detail),
            ..entry.clone()
        };
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        self.trim().map_err(|err| err.to_string())?;
        let mut line = serde_json::to_string(&entry).map_err(|err| err.to_string())?;
        line.push('\n');
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut file| file.write_all(line.as_bytes()))
            .map_err(|err| format!("the action log could not be written: {err}"))
    }

    /// The newest `count` entries, newest first. Lines that cannot be read are skipped.
    pub fn newest(&self, count: usize) -> Vec<Entry> {
        let Ok(file) = File::open(&self.path) else {
            return Vec::new();
        };
        let mut entries: Vec<Entry> = BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| serde_json::from_str(&line).ok())
            .collect();
        entries.reverse();
        entries.truncate(count);
        entries
    }

    /// Empties the log.
    pub fn clear(&self) -> Result<(), String> {
        match fs::remove_file(&self.path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.to_string()),
            _ => Ok(()),
        }
    }

    /// Keeps the newer half once the log has grown too long.
    fn trim(&self) -> std::io::Result<()> {
        let Ok(metadata) = fs::metadata(&self.path) else {
            return Ok(());
        };
        if metadata.len() < MAX_BYTES {
            return Ok(());
        }
        let text = fs::read_to_string(&self.path)?;
        let lines: Vec<&str> = text.lines().collect();
        let kept = lines[lines.len() / 2..].join("\n");
        let temp = self.path.with_extension("tmp");
        fs::write(&temp, format!("{kept}\n"))?;
        fs::rename(&temp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(action: &str) -> Entry {
        Entry {
            at_ms: 1,
            request: "send it".into(),
            level: Level::Change,
            kind: "click".into(),
            action: action.into(),
            outcome: Outcome::Done,
            detail: String::new(),
        }
    }

    fn scratch(name: &str) -> ActionLog {
        let dir = std::env::temp_dir().join(format!("keenu-guard-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        ActionLog::new(dir.join("actions.log"))
    }

    #[test]
    fn keeps_what_happened_newest_first() {
        let log = scratch("order");
        assert!(log.newest(5).is_empty());
        log.add(&entry("first")).unwrap();
        log.add(&entry("second")).unwrap();
        let newest = log.newest(5);
        assert_eq!(newest[0].action, "second");
        assert_eq!(newest[1].action, "first");
        assert_eq!(log.newest(1).len(), 1);
        log.clear().unwrap();
        log.clear().unwrap();
        assert!(log.newest(5).is_empty());
    }

    #[test]
    fn never_writes_a_secret_down() {
        let log = scratch("secret");
        log.add(&entry(
            "Run: curl -H 'Authorization: Bearer abc.def' https://x",
        ))
        .unwrap();
        let text = fs::read_to_string(log.path()).unwrap();
        assert!(!text.contains("abc.def"), "{text}");
    }

    #[test]
    fn long_text_is_cut_and_a_full_log_keeps_its_newer_half() {
        let log = scratch("long");
        let long = "x".repeat(MAX_TEXT * 2);
        log.add(&entry(&long)).unwrap();
        assert_eq!(log.newest(1)[0].action.chars().count(), MAX_TEXT + 1);
        let line = serde_json::to_string(&entry(&"y".repeat(1000))).unwrap();
        let lines = usize::try_from(MAX_BYTES).unwrap() / line.len() + 2;
        fs::write(log.path(), format!("{line}\n").repeat(lines)).unwrap();
        log.add(&entry("after")).unwrap();
        assert!(fs::metadata(log.path()).unwrap().len() < MAX_BYTES);
        assert_eq!(log.newest(1)[0].action, "after");
    }

    #[test]
    fn lines_from_before_the_kind_was_kept_still_read() {
        let log = scratch("old");
        fs::create_dir_all(log.path().parent().unwrap()).unwrap();
        fs::write(
            log.path(),
            "{\"at_ms\":1,\"request\":\"r\",\"level\":\"look\",\"action\":\"a\",\"outcome\":\"done\"}\n",
        )
        .unwrap();
        let read = log.newest(1);
        assert_eq!(read.len(), 1);
        assert!(read[0].kind.is_empty());
        log.add(&entry("new")).unwrap();
        assert_eq!(log.newest(1)[0].kind, "click");
    }

    #[test]
    fn a_damaged_line_does_not_lose_the_others() {
        let log = scratch("damaged");
        log.add(&entry("good")).unwrap();
        let mut file = OpenOptions::new().append(true).open(log.path()).unwrap();
        writeln!(file, "not json").unwrap();
        assert_eq!(log.newest(5).len(), 1);
    }

    #[test]
    fn a_log_that_cannot_be_written_says_so() {
        // Its folder is a file.
        let dir = std::env::temp_dir().join(format!("keenu-guard-{}-file", std::process::id()));
        fs::write(&dir, "x").unwrap();
        assert!(
            ActionLog::new(dir.join("actions.log"))
                .add(&entry("a"))
                .is_err()
        );
        fs::remove_file(dir).unwrap();
    }
}
