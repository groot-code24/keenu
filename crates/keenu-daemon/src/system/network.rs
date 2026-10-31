// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

use std::collections::HashMap;

use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{Connection, proxy};

use super::{Network, SystemEvent, SystemService, publish, settled};
use zbus::object_server::InterfaceRef;

const DESTINATION: &str = "org.freedesktop.NetworkManager";
const INTERFACE: &str = "org.freedesktop.NetworkManager";
/// NetworkManager's NM_STATE_CONNECTED_SITE; anything above means connected.
const STATE_CONNECTED_SITE: u32 = 60;

/// One active NetworkManager connection, read for its name.
#[proxy(
    interface = "org.freedesktop.NetworkManager.Connection.Active",
    default_service = "org.freedesktop.NetworkManager",
    gen_blocking = false
)]
trait ActiveConnection {
    /// The connection's name, e.g. the Wi-Fi network.
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;
}

/// Publishes the network state, with an alert when it connects, drops or switches.
pub async fn watch(system: Connection, iface: InterfaceRef<SystemService>) -> anyhow::Result<()> {
    let properties = PropertiesProxy::builder(&system)
        .destination(DESTINATION)?
        .path("/org/freedesktop/NetworkManager")?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let mut changes = properties
        .receive_properties_changed_with_args(&[(0, INTERFACE)])
        .await?;
    let mut previous: Option<Network> = None;
    let mut sleeps = super::sleep::sleeps();

    loop {
        let current = read(&system, &properties).await?;
        // Going to sleep and waking drop the connection until it is back: no alert for that.
        if sleeps.has_changed().unwrap_or(false) {
            sleeps.mark_unchanged();
            previous = None;
        }
        let event = previous
            .as_ref()
            .filter(|before| {
                before.connected != current.connected
                    || (current.connected && before.name != current.name)
            })
            .map(|_| SystemEvent::Network(current.clone()));

        let network = current.clone();
        publish(&iface, |state| state.network = network, event).await;
        previous = Some(current);

        if !settled(&mut changes).await {
            return Ok(());
        }
    }
}

/// What NetworkManager is connected through, from one `GetAll` and the connection's name.
async fn read(system: &Connection, properties: &PropertiesProxy<'_>) -> zbus::Result<Network> {
    let all: HashMap<String, OwnedValue> = properties
        .get_all(InterfaceName::from_static_str_unchecked(INTERFACE))
        .await?;
    let state = all.get("State").and_then(|v| u32::try_from(v).ok());
    if state.unwrap_or(0) < STATE_CONNECTED_SITE {
        return Ok(Network::default());
    }
    let text = |key: &str| {
        all.get(key)
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default()
    };
    let connection = match text("PrimaryConnectionType").as_str() {
        "802-11-wireless" => "wifi",
        "802-3-ethernet" => "ethernet",
        _ => "other",
    };
    let Some(path) = all
        .get("PrimaryConnection")
        .and_then(|v| OwnedObjectPath::try_from(v.try_clone().ok()?).ok())
    else {
        return Ok(Network {
            connected: true,
            connection: connection.into(),
            name: String::new(),
        });
    };
    let name = match ActiveConnectionProxy::builder(system)
        .path(path)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
    {
        Ok(connection) => connection.id().await.unwrap_or_default(),
        Err(_) => String::new(),
    };
    Ok(Network {
        connected: true,
        connection: connection.into(),
        name,
    })
}
