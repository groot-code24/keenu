// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Waking from suspend, from logind, so the clock's waits can sleep to their deadline (see `clock::resumed`),
//! and whether the screen is locked, for work nobody would see meanwhile (see `locked`).

use std::sync::LazyLock;

use futures_util::StreamExt;
use log::debug;
use tokio::sync::watch;
use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, MatchRule, MessageStream};

const LOGIND: &str = "org.freedesktop.login1";
const SESSION: &str = "org.freedesktop.login1.Session";

/// Whether the session's screen is locked; `false` when unknown.
static LOCKED: LazyLock<watch::Sender<bool>> = LazyLock::new(|| watch::Sender::new(false));

/// Whether the screen is locked, told again each time that changes.
pub fn locked() -> watch::Receiver<bool> {
    LOCKED.subscribe()
}

/// How often the computer went to sleep or woke, so a watcher can tell what came with that.
static SLEEPS: LazyLock<watch::Sender<u64>> = LazyLock::new(|| watch::Sender::new(0));

/// Changes as the computer goes to sleep and as it wakes.
pub fn sleeps() -> watch::Receiver<u64> {
    SLEEPS.subscribe()
}

/// Counts logind's sleeps and wakes, and has the clock re-check its waits on each wake.
pub async fn watch(bus: Connection) -> anyhow::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(LOGIND)?
        .interface("org.freedesktop.login1.Manager")?
        .member("PrepareForSleep")?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, &bus, None).await?;
    while let Some(message) = signals.next().await {
        let going = message?.body().deserialize::<bool>();
        SLEEPS.send_modify(|count| *count += 1);
        // `false`: waking up, not going to sleep.
        if going == Ok(false) {
            crate::clock::resumed();
        }
    }
    Ok(())
}

/// Follows the session's `LockedHint`, which GNOME sets while the screen is locked.
pub async fn watch_lock(bus: Connection) -> anyhow::Result<()> {
    let result = follow_lock(&bus).await;
    // Unknown again: nothing waits on a lock that may be gone.
    set_locked(false);
    result
}

/// Follows `LockedHint` on the user's graphical session; with no session, waits forever.
async fn follow_lock(bus: &Connection) -> anyhow::Result<()> {
    // keenud runs as a user service, outside the session: "auto" is then the user's graphical one.
    let reply = bus
        .call_method(
            Some(LOGIND),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "GetSession",
            &("auto",),
        )
        .await;
    let session: OwnedObjectPath = match reply.and_then(|reply| reply.body().deserialize()) {
        Ok(path) => path,
        Err(err) => {
            // No session to follow (a test machine): it never counts as locked.
            debug!("no login session, so the screen lock is not followed: {err}");
            std::future::pending::<()>().await;
            return Ok(());
        }
    };
    let properties = PropertiesProxy::builder(bus)
        .destination(LOGIND)?
        .path(session)?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let mut changes = properties
        .receive_properties_changed_with_args(&[(0, SESSION)])
        .await?;
    let now = properties
        .get(
            InterfaceName::from_static_str_unchecked(SESSION),
            "LockedHint",
        )
        .await?;
    set_locked(bool::try_from(now).unwrap_or(false));
    while let Some(signal) = changes.next().await {
        let Ok(args) = signal.args() else { continue };
        if let Some(locked) = args.changed_properties().get("LockedHint") {
            set_locked(bool::try_from(locked).unwrap_or(false));
        }
    }
    Ok(())
}

/// Publishes the lock state, waking watchers only when it changes.
fn set_locked(locked: bool) {
    LOCKED.send_if_modified(|was| std::mem::replace(was, locked) != locked);
}
