// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Cover art as local files, downloading remote art (e.g. Spotify's) once.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::store::cache_dir;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10);
/// Refuse anything bigger than any sane cover image.
const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Returns a local path for `art_url` if one exists without a download.
pub fn local_path(art_url: &str) -> Option<String> {
    if is_remote(art_url) {
        let path = cached_path(art_url);
        return path.exists().then(|| path.to_string_lossy().into_owned());
    }
    Some(crate::uri::to_path(art_url)?.to_string_lossy().into_owned())
}

/// Whether `art_url` is remote and needs [`download`] first.
pub fn is_remote(art_url: &str) -> bool {
    art_url.starts_with("https://") || art_url.starts_with("http://")
}

/// Downloads remote art into the cache. Blocking.
pub fn download(art_url: &str) -> anyhow::Result<String> {
    let path = cached_path(art_url);
    if !path.exists() {
        let bytes = crate::http::get(art_url, DOWNLOAD_TIMEOUT)
            .call()?
            .body_mut()
            .with_config()
            .limit(MAX_BYTES)
            .read_to_vec()?;
        fs::create_dir_all(path.parent().expect("cache path has a parent"))?;
        let temp = path.with_extension("tmp");
        fs::write(&temp, bytes)?;
        fs::rename(temp, &path)?;
    }
    Ok(path.to_string_lossy().into_owned())
}

/// Where a remote cover lives in the cache, named by a hash of its URL.
fn cached_path(art_url: &str) -> PathBuf {
    cache_dir()
        .join("art")
        .join(format!("{:016x}", fnv1a(art_url)))
}

/// FNV-1a: a stable hash for cache file names (std's hasher may change
/// between Rust releases, which would orphan the cache).
pub fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}
