// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Who may call what: the methods that read the user's clipboard or steer the assistant (which can type
//! and open things) answer GNOME Shell, where the extension runs, and no other program on the bus.

use zbus::message::Header;
use zbus::names::BusName;
use zbus::proxy::CacheProperties;
use zbus::{Connection, fdo};

const SHELL: &str = "org.gnome.Shell";

/// Refuses the call unless it comes from GNOME Shell.
pub async fn shell_only(connection: &Connection, header: &Header<'_>) -> fdo::Result<()> {
    let sender = header.sender().map(|s| s.as_str().to_owned());
    let shell = shell_owner(connection).await;
    if allowed(sender.as_deref(), shell.as_deref()) {
        Ok(())
    } else {
        Err(fdo::Error::AccessDenied(
            "only GNOME Shell may call this".into(),
        ))
    }
}

/// The unique name GNOME Shell has on the bus, if it runs.
async fn shell_owner(connection: &Connection) -> Option<String> {
    let dbus = fdo::DBusProxy::builder(connection)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .ok()?;
    let owner = dbus
        .get_name_owner(BusName::try_from(SHELL).ok()?)
        .await
        .ok()?;
    Some(owner.as_str().to_owned())
}

/// True when the sender is GNOME Shell's own bus name; no shell or no sender never passes.
fn allowed(sender: Option<&str>, shell: Option<&str>) -> bool {
    matches!((sender, shell), (Some(sender), Some(shell)) if sender == shell)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_shell_gets_in() {
        assert!(allowed(Some(":1.12"), Some(":1.12")));
        assert!(!allowed(Some(":1.40"), Some(":1.12")));
        // No shell (a test session), or a sender the bus did not name: no.
        assert!(!allowed(Some(":1.40"), None));
        assert!(!allowed(None, Some(":1.12")));
    }
}
