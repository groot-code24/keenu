// SPDX-License-Identifier: GPL-3.0-or-later

//! The product's identity and where its files live on each operating system.

use std::path::PathBuf;

/// The product's name, as people see it.
pub const PRODUCT: &str = "Keenu";
/// Windows' AppUserModelID: toasts, the taskbar and the Start Menu shortcut all use this.
pub const APP_USER_MODEL_ID: &str = "Keenu.Desktop";
/// Overrides the data folder: tests, portable use and support.
pub const DATA_DIR_ENV: &str = "KEENU_DATA_DIR";

/// Keenu's folder name: lower case where the platform's own folders are, as on Linux.
fn folder_name() -> &'static str {
    if cfg!(target_os = "linux") {
        "keenu"
    } else {
        PRODUCT
    }
}

/// Keenu's data folder: `%LOCALAPPDATA%\Keenu`, `~/Library/Application Support/Keenu` or
/// `~/.local/share/keenu`.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(folder_name())
}

/// Keenu's cache folder, for what can be fetched again.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir).join("cache");
    }
    if cfg!(windows) {
        return data_dir().join("cache");
    }
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(folder_name())
}

/// The folders earlier names of this project kept their data in, oldest name last. Empty when
/// [`DATA_DIR_ENV`] is set: an overridden folder is never filled from the real one.
pub fn legacy_data_dirs() -> Vec<PathBuf> {
    if std::env::var_os(DATA_DIR_ENV).is_some_and(|dir| !dir.is_empty()) {
        return Vec::new();
    }
    let Some(base) = dirs::data_local_dir() else {
        return Vec::new();
    };
    if cfg!(windows) {
        vec![base.join("Neeru")]
    } else if cfg!(target_os = "linux") {
        vec![base.join("isle")]
    } else {
        Vec::new()
    }
}
