// SPDX-License-Identifier: GPL-3.0-or-later

//! JSON files saved whole or not at all, and private to the user.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Loads `path`; a missing file is the default. A corrupt one is moved aside to `<name>.corrupt` and
/// the default returned, so it is kept for recovery and never overwritten. Any other error is returned
/// and the caller must not save, or it would replace data it never saw.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> io::Result<T> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(T::default()),
        Err(err) => return Err(err),
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(value),
        Err(_) => {
            fs::rename(path, sibling(path, ".corrupt"))?;
            Ok(T::default())
        }
    }
}

/// Saves `value` to `path` atomically: readers see the old file or the new one, never a torn one.
pub fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    write_private(path, &bytes)
}

/// Writes `bytes` to `path` atomically, readable only by this user on Unix. On Windows the file
/// inherits `%LOCALAPPDATA%`'s access list, which already excludes other users.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    let temp = sibling(path, ".tmp");
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)
}

/// Creates `dir` and its parents; on Unix a folder made here is only for this user.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    fs::create_dir_all(dir)
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keenu-json-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("file.json")
    }

    #[test]
    fn round_trips_and_a_missing_file_is_the_default() {
        let path = temp("round");
        assert_eq!(load::<Vec<u32>>(&path).unwrap(), Vec::<u32>::new());
        save(&path, &vec![1, 2]).unwrap();
        assert_eq!(load::<Vec<u32>>(&path).unwrap(), [1, 2]);
    }

    #[test]
    fn a_corrupt_file_is_kept_aside() {
        let path = temp("corrupt");
        fs::write(&path, "{not json").unwrap();
        assert_eq!(load::<Vec<u32>>(&path).unwrap(), Vec::<u32>::new());
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(sibling(&path, ".corrupt")).unwrap(),
            "{not json"
        );
    }

    #[cfg(unix)]
    #[test]
    fn saved_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp("private");
        save(&path, &1).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
