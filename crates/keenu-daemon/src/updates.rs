// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! A newer Keenu: GitHub's latest release, looked up at most once a day while the extension wants it
//! (Settings), so the island can say an update is out. Nothing is sent but the request itself.

use std::sync::Arc;
use std::time::Duration;

use log::warn;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use zbus::object_server::InterfaceRef;
use zbus::{fdo, interface};

use crate::clock::{now_ms, wait_until};
use crate::store::Store;

/// The GitHub repository releases are published in, `owner/name`, set when Keenu is built. A build
/// without one never looks: it must not ask someone else's repository what is new.
const REPOSITORY: Option<&str> = option_env!("KEENU_RELEASE_REPOSITORY");
const EVERY_MS: i64 = 24 * 3600 * 1000;
/// After a failed look (offline), the next is this much sooner.
const RETRY_MS: i64 = 3600 * 1000;
const TIMEOUT: Duration = Duration::from_secs(15);

/// When it last looked, kept across restarts so a login is no reason to ask again.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Checked {
    at_ms: i64,
    latest: String,
}

/// The Updates1 D-Bus object: the switch from Settings and the newer version found.
pub struct UpdatesService {
    enabled: bool,
    latest: String,
    wake: Arc<Notify>,
}

impl UpdatesService {
    /// Starts off; the returned `Notify` wakes `run` when Settings change.
    pub fn new() -> (Self, Arc<Notify>) {
        let wake = Arc::new(Notify::new());
        let service = Self {
            enabled: false,
            latest: String::new(),
            wake: wake.clone(),
        };
        (service, wake)
    }
}

#[interface(name = "io.github.zickrian.isle.Updates1")]
impl UpdatesService {
    /// Whether to look (Settings); off until the extension says so.
    fn set_enabled(&mut self, enabled: bool) -> fdo::Result<()> {
        self.enabled = enabled;
        self.wake.notify_one();
        Ok(())
    }

    /// The newest release's version ("0.8.0") when it is newer than this keenud; empty otherwise.
    #[zbus(property)]
    fn latest(&self) -> String {
        self.latest.clone()
    }
}

/// Looks now and then while enabled; sleeps otherwise.
pub async fn run(iface: InterfaceRef<UpdatesService>, wake: Arc<Notify>) {
    let mut store: Store<Checked> = Store::new("updates");
    let mut checked = store.load();
    loop {
        let enabled = iface.get().await.enabled;
        if !enabled {
            publish(&iface, String::new()).await;
            wake.notified().await;
            continue;
        }
        if now_ms() - checked.at_ms >= EVERY_MS {
            let looked = tokio::task::spawn_blocking(latest_release)
                .await
                .ok()
                .flatten();
            checked.at_ms = match looked {
                Some(version) => {
                    checked.latest = version;
                    now_ms()
                }
                None => now_ms() - EVERY_MS + RETRY_MS,
            };
            store.save(&checked);
        }
        let newer = is_newer(&checked.latest, env!("CARGO_PKG_VERSION"));
        publish(
            &iface,
            if newer {
                checked.latest.clone()
            } else {
                String::new()
            },
        )
        .await;
        // Until the next look is due, or Settings change.
        while !wait_until(Some(checked.at_ms + EVERY_MS), &wake).await {
            if !iface.get().await.enabled {
                break;
            }
        }
    }
}

/// Sets the `Latest` property, signalling only when it changed.
async fn publish(iface: &InterfaceRef<UpdatesService>, latest: String) {
    let mut service = iface.get_mut().await;
    if service.latest == latest {
        return;
    }
    service.latest = latest;
    if let Err(err) = service.latest_changed(iface.signal_emitter()).await {
        warn!("could not publish the latest version: {err}");
    }
}

/// The latest release's version, without the "v"; `None` if GitHub could not say.
fn latest_release() -> Option<String> {
    let repository = REPOSITORY.filter(|name| !name.is_empty())?;
    let latest = format!("https://api.github.com/repos/{repository}/releases/latest");
    let release: serde_json::Value = crate::http::get(&latest, TIMEOUT)
        .header("User-Agent", concat!("Keenu/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .and_then(|mut reply| reply.body_mut().read_json())
        .map_err(|err| warn!("could not look for a newer Keenu: {err}"))
        .ok()?;
    let tag = release["tag_name"].as_str()?;
    Some(tag.trim_start_matches('v').to_owned())
}

/// Whether version `a` ("0.8.0") is newer than `b`, by each number.
fn is_newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    !a.is_empty() && parts(a) > parts(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(is_newer("1.0.0", "0.99.0"));
        assert!(!is_newer("0.7.0", "0.7.0"));
        assert!(!is_newer("0.6.9", "0.7.0"));
        assert!(!is_newer("", "0.7.0"));
    }
}
