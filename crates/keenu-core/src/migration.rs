// SPDX-License-Identifier: GPL-3.0-or-later

//! Brings data from a folder under an earlier name of the project into Keenu's.
//!
//! Nothing is lost whatever happens: the old folder is left as it was, a file already in the new
//! folder is never replaced, and a run that fails part way is simply run again at the next start.
//! Only re-downloadable bulk (see `moved`) is moved rather than copied.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Written last, into the new folder: its presence means that source is done with.
const MARKER_PREFIX: &str = ".migrated-from-";
/// A line per migration and per thing it had to skip or could not do.
pub const LOG_FILE: &str = "migration.log";
/// A file being copied, until it is complete.
const PARTIAL: &str = ".keenu-migrating";

/// What one migration did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// The source was already migrated, or does not exist: nothing was done.
    pub skipped: bool,
    pub copied: u64,
    pub moved: u64,
    /// Files left alone because the new folder already has them.
    pub kept_existing: u64,
    /// What could not be brought over; the run stays unmarked and is tried again.
    pub failed: Vec<String>,
}

impl Report {
    /// Whether everything that could be brought over was.
    pub fn complete(&self) -> bool {
        self.failed.is_empty()
    }
}

/// The marker file for `from` inside `to`.
fn marker(from: &Path, to: &Path) -> PathBuf {
    let name = from
        .file_name()
        .map_or_else(|| "unknown".into(), |name| name.to_string_lossy());
    to.join(format!("{MARKER_PREFIX}{name}"))
}

/// Brings `from` into `to`. Subfolders named in `moved` are renamed across when the new folder has
/// none, so gigabytes of speech models are not copied; everything else is copied.
pub fn migrate(from: &Path, to: &Path, moved: &[&str]) -> io::Result<Report> {
    let mut report = Report::default();
    if !from.is_dir() || marker(from, to).exists() || same_folder(from, to) {
        report.skipped = true;
        return Ok(report);
    }
    fs::create_dir_all(to)?;
    log_line(
        to,
        &format!("start: {} -> {}", from.display(), to.display()),
    );
    for name in moved {
        let (old, new) = (from.join(name), to.join(name));
        if old.is_dir() && !new.exists() {
            match fs::rename(&old, &new) {
                Ok(()) => {
                    report.moved += 1;
                    log_line(to, &format!("moved: {name}"));
                }
                // Another drive, say: an ordinary copy below takes over.
                Err(err) => log_line(to, &format!("could not move {name}: {err}; copying it")),
            }
        }
    }
    copy_tree(from, to, to, &mut report);
    if report.complete() {
        let mut file = fs::File::create(marker(from, to))?;
        writeln!(file, "{}", from.display())?;
        file.sync_all()?;
    }
    log_line(
        to,
        &format!(
            "end: {} copied, {} moved, {} kept, {} failed",
            report.copied,
            report.moved,
            report.kept_existing,
            report.failed.len()
        ),
    );
    Ok(report)
}

/// Sets the new folder aside (nothing is deleted), so the next start migrates from the old one again.
/// Returns where it went, or `None` when there was nothing to set aside.
pub fn roll_back(to: &Path) -> io::Result<Option<PathBuf>> {
    if !to.exists() {
        return Ok(None);
    }
    let name = to
        .file_name()
        .map_or_else(|| "keenu".into(), |name| name.to_string_lossy());
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let aside = to.with_file_name(format!("{name}.rolled-back-{stamp}"));
    fs::rename(to, &aside)?;
    Ok(Some(aside))
}

fn same_folder(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

fn copy_tree(from: &Path, to: &Path, root: &Path, report: &mut Report) {
    let entries = match fs::read_dir(from) {
        Ok(entries) => entries,
        Err(err) => {
            fail(root, report, from, &err.to_string());
            return;
        }
    };
    if let Err(err) = fs::create_dir_all(to) {
        fail(root, report, to, &err.to_string());
        return;
    }
    match_permissions(from, to);
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                fail(root, report, from, &err.to_string());
                continue;
            }
        };
        let (old, new) = (entry.path(), to.join(entry.file_name()));
        let Ok(kind) = entry.file_type() else {
            fail(root, report, &old, "could not tell what it is");
            continue;
        };
        if kind.is_dir() {
            copy_tree(&old, &new, root, report);
        } else if new.symlink_metadata().is_ok() {
            report.kept_existing += 1;
            log_line(root, &format!("kept existing: {}", new.display()));
        } else if kind.is_symlink() {
            copy_link(&old, &new, root, report);
        } else if let Err(err) = copy_file(&old, &new) {
            fail(root, report, &old, &err.to_string());
        } else {
            report.copied += 1;
        }
    }
}

/// Complete or not there at all: a stopped copy leaves only the partial file, replaced next time.
fn copy_file(old: &Path, new: &Path) -> io::Result<()> {
    let mut partial = new.as_os_str().to_owned();
    partial.push(PARTIAL);
    let partial = PathBuf::from(partial);
    fs::copy(old, &partial)?;
    fs::rename(&partial, new)
}

#[cfg(unix)]
fn copy_link(old: &Path, new: &Path, root: &Path, report: &mut Report) {
    let done = fs::read_link(old).and_then(|target| std::os::unix::fs::symlink(target, new));
    match done {
        Ok(()) => report.copied += 1,
        Err(err) => fail(root, report, old, &err.to_string()),
    }
}

#[cfg(not(unix))]
fn copy_link(old: &Path, _new: &Path, root: &Path, report: &mut Report) {
    // Keenu's Windows data never had links; one is noted, not followed out of the folder.
    log_line(root, &format!("left a link behind: {}", old.display()));
    report.kept_existing += 1;
}

/// A private folder stays private.
#[cfg(unix)]
fn match_permissions(from: &Path, to: &Path) {
    if let Ok(metadata) = fs::metadata(from) {
        let _ = fs::set_permissions(to, metadata.permissions());
    }
}

#[cfg(not(unix))]
fn match_permissions(_from: &Path, _to: &Path) {}

fn fail(root: &Path, report: &mut Report, path: &Path, why: &str) {
    log_line(root, &format!("failed: {}: {why}", path.display()));
    report.failed.push(format!("{}: {why}", path.display()));
}

fn log_line(root: &Path, line: &str) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(LOG_FILE))
    {
        let _ = writeln!(file, "{stamp} {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("keenu-migration-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copies_everything_and_leaves_the_old_folder_as_it_was() {
        let root = temp("copy");
        let (old, new) = (root.join("isle"), root.join("keenu"));
        fs::create_dir_all(old.join("clips")).unwrap();
        fs::write(old.join("todo.json"), "[1]").unwrap();
        fs::write(old.join("clips/a.png"), "png").unwrap();

        let report = migrate(&old, &new, &[]).unwrap();

        assert_eq!((report.copied, report.complete()), (2, true));
        assert_eq!(fs::read_to_string(new.join("todo.json")).unwrap(), "[1]");
        assert_eq!(fs::read_to_string(new.join("clips/a.png")).unwrap(), "png");
        assert_eq!(fs::read_to_string(old.join("todo.json")).unwrap(), "[1]");
        assert!(new.join(LOG_FILE).exists());
    }

    #[test]
    fn never_replaces_a_file_the_new_folder_has() {
        let root = temp("keep");
        let (old, new) = (root.join("isle"), root.join("keenu"));
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(old.join("settings.json"), "old").unwrap();
        fs::write(new.join("settings.json"), "new").unwrap();

        let report = migrate(&old, &new, &[]).unwrap();

        assert_eq!(report.kept_existing, 1);
        assert_eq!(
            fs::read_to_string(new.join("settings.json")).unwrap(),
            "new"
        );
    }

    #[test]
    fn runs_once() {
        let root = temp("once");
        let (old, new) = (root.join("isle"), root.join("keenu"));
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("a"), "1").unwrap();
        assert!(!migrate(&old, &new, &[]).unwrap().skipped);
        // Deleted on purpose in the new folder: not brought back on the next start.
        fs::remove_file(new.join("a")).unwrap();
        assert!(migrate(&old, &new, &[]).unwrap().skipped);
        assert!(!new.join("a").exists());
    }

    #[test]
    fn moves_named_bulk_instead_of_copying_it() {
        let root = temp("move");
        let (old, new) = (root.join("isle"), root.join("keenu"));
        fs::create_dir_all(old.join("models")).unwrap();
        fs::write(old.join("models/big.bin"), "weights").unwrap();

        let report = migrate(&old, &new, &["models"]).unwrap();

        assert_eq!(report.moved, 1);
        assert!(!old.join("models").exists());
        assert_eq!(
            fs::read_to_string(new.join("models/big.bin")).unwrap(),
            "weights"
        );
    }

    #[test]
    fn a_missing_source_is_nothing_to_do() {
        let root = temp("none");
        let report = migrate(&root.join("nope"), &root.join("keenu"), &[]).unwrap();
        assert!(report.skipped);
        assert!(!root.join("keenu").exists());
    }

    #[test]
    fn roll_back_sets_the_new_folder_aside_and_migration_runs_again() {
        let root = temp("rollback");
        let (old, new) = (root.join("isle"), root.join("keenu"));
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("a"), "1").unwrap();
        migrate(&old, &new, &[]).unwrap();

        let aside = roll_back(&new).unwrap().unwrap();

        assert!(aside.join("a").exists(), "nothing is deleted");
        assert!(!new.exists());
        assert!(!migrate(&old, &new, &[]).unwrap().skipped);
        assert!(new.join("a").exists());
    }

    #[test]
    fn a_stale_partial_copy_is_replaced() {
        let root = temp("partial");
        let (old, new) = (root.join("isle"), root.join("keenu"));
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(old.join("a"), "whole").unwrap();
        fs::write(new.join(format!("a{PARTIAL}")), "ha").unwrap();
        migrate(&old, &new, &[]).unwrap();
        assert_eq!(fs::read_to_string(new.join("a")).unwrap(), "whole");
    }
}
