// SPDX-License-Identifier: GPL-3.0-or-later

//! The file shelf on hosts that keep it themselves: references to files, with what is worth showing
//! about each. It never copies, moves or changes the files it refers to.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "shelf.json";
/// Files kept; pinned ones do not count against it until nothing else is left to drop.
pub const CAPACITY: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Absolute and normalised: the key that finds duplicates.
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub modified_ms: i64,
    pub added_ms: i64,
    pub is_dir: bool,
    pub pinned: bool,
    /// Set when the file was last looked for and not found. Kept: a drive may come back.
    #[serde(default)]
    pub missing: bool,
}

/// Why a path was not put on the shelf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddError {
    /// Not an absolute path, or it has `..` in it.
    Unsafe,
    NotFound,
    /// Already there; it moved to the front instead.
    Duplicate,
}

/// Newest first, pinned or not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileShelf {
    entries: Vec<Entry>,
}

/// A path from outside (a drop, a saved file, the command line) as one safe to act on: absolute and
/// with no `..`, so it cannot walk somewhere other than what it names.
pub fn validate_path(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut clean = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => return None,
            Component::CurDir => {}
            part => clean.push(part.as_os_str()),
        }
    }
    Some(clean)
}

impl FileShelf {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Adds the file at `path`, reading what is shown about it from the disk.
    pub fn add(&mut self, path: &Path, now_ms: i64) -> Result<(), AddError> {
        let clean = validate_path(path).ok_or(AddError::Unsafe)?;
        let metadata = std::fs::metadata(&clean).map_err(|_| AddError::NotFound)?;
        if let Some(at) = self
            .entries
            .iter()
            .position(|entry| same_path(&entry.path, &clean))
        {
            let mut entry = self.entries.remove(at);
            entry.added_ms = now_ms;
            entry.missing = false;
            self.entries.insert(0, entry);
            return Err(AddError::Duplicate);
        }
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |time| time.as_millis() as i64);
        let name = clean.file_name().map_or_else(
            || clean.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        self.entries.insert(
            0,
            Entry {
                path: clean,
                name,
                size: metadata.len(),
                modified_ms,
                added_ms: now_ms,
                is_dir: metadata.is_dir(),
                pinned: false,
                missing: false,
            },
        );
        self.prune();
        Ok(())
    }

    pub fn set_pinned(&mut self, path: &Path, pinned: bool) -> bool {
        match self
            .entries
            .iter_mut()
            .find(|entry| same_path(&entry.path, path))
        {
            Some(entry) => {
                entry.pinned = pinned;
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, path: &Path) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| !same_path(&entry.path, path));
        self.entries.len() != before
    }

    /// Empties the shelf except for what is pinned; the files themselves are not touched.
    pub fn clear_unpinned(&mut self) {
        self.entries.retain(|entry| entry.pinned);
    }

    /// Looks each file up again; true if any changed state.
    pub fn refresh_missing(&mut self) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            let missing = !entry.path.exists();
            changed |= missing != entry.missing;
            entry.missing = missing;
        }
        changed
    }

    fn prune(&mut self) {
        while self.entries.len() > CAPACITY {
            let drop = self
                .entries
                .iter()
                .rposition(|entry| !entry.pinned)
                .unwrap_or(self.entries.len() - 1);
            self.entries.remove(drop);
        }
    }
}

/// Windows paths differ only in case when they are the same file.
fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
    } else {
        a == b
    }
}

/// A file size as people read it.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keenu-shelf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, b"12345").unwrap();
        path
    }

    #[test]
    fn keeps_a_reference_and_what_is_shown_about_it() {
        let path = temp_file("a.txt");
        let mut shelf = FileShelf::default();
        shelf.add(&path, 7).unwrap();
        let entry = &shelf.entries()[0];
        assert_eq!(
            (entry.name.as_str(), entry.size, entry.added_ms),
            ("a.txt", 5, 7)
        );
        assert!(path.exists(), "the file stays where it was");
    }

    #[test]
    fn refuses_unsafe_paths_and_missing_files() {
        let mut shelf = FileShelf::default();
        assert_eq!(
            shelf.add(Path::new("relative.txt"), 0),
            Err(AddError::Unsafe)
        );
        let walk = std::env::temp_dir().join("..").join("x");
        assert_eq!(shelf.add(&walk, 0), Err(AddError::Unsafe));
        let gone = std::env::temp_dir().join("keenu-does-not-exist-really");
        assert_eq!(shelf.add(&gone, 0), Err(AddError::NotFound));
        assert!(shelf.entries().is_empty());
    }

    #[test]
    fn a_duplicate_moves_to_the_front() {
        let (a, b) = (temp_file("dup-a"), temp_file("dup-b"));
        let mut shelf = FileShelf::default();
        shelf.add(&a, 1).unwrap();
        shelf.add(&b, 2).unwrap();
        assert_eq!(shelf.add(&a, 3), Err(AddError::Duplicate));
        assert_eq!(shelf.entries().len(), 2);
        assert_eq!(shelf.entries()[0].path, a);
    }

    #[test]
    fn notices_a_file_going_missing_and_coming_back() {
        let path = temp_file("goes-away");
        let mut shelf = FileShelf::default();
        shelf.add(&path, 1).unwrap();
        assert!(!shelf.refresh_missing());
        std::fs::remove_file(&path).unwrap();
        assert!(shelf.refresh_missing());
        assert!(shelf.entries()[0].missing);
        std::fs::write(&path, b"x").unwrap();
        assert!(shelf.refresh_missing());
        assert!(!shelf.entries()[0].missing);
    }

    #[test]
    fn clearing_keeps_what_is_pinned_and_the_files() {
        let (a, b) = (temp_file("pin-a"), temp_file("pin-b"));
        let mut shelf = FileShelf::default();
        shelf.add(&a, 1).unwrap();
        shelf.add(&b, 2).unwrap();
        assert!(shelf.set_pinned(&a, true));
        shelf.clear_unpinned();
        assert_eq!(shelf.entries().len(), 1);
        assert_eq!(shelf.entries()[0].path, a);
        assert!(shelf.remove(&a));
        assert!(a.exists() && b.exists());
    }

    #[test]
    fn a_full_shelf_drops_the_oldest_that_is_not_pinned() {
        let mut shelf = FileShelf::default();
        let oldest = temp_file("oldest");
        shelf.add(&oldest, 0).unwrap();
        shelf.set_pinned(&oldest, true);
        for i in 0..CAPACITY {
            shelf
                .add(&temp_file(&format!("many-{i}")), i as i64 + 1)
                .unwrap();
        }
        assert_eq!(shelf.entries().len(), CAPACITY);
        assert!(shelf.entries().iter().any(|entry| entry.path == oldest));
    }

    #[test]
    fn sizes_read_as_people_expect() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }
}
