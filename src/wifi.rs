//! Detect the currently connected Wi-Fi network via NetworkManager over D-Bus.

use zbus::zvariant::OwnedObjectPath;

/// Return the SSID of the primary Wi-Fi connection, or `None` when the machine
/// is not connected to Wi-Fi (for example on Ethernet or with no connection).
pub async fn current_ssid() -> Option<String> {
    let connection = match zbus::Connection::system().await {
        Ok(connection) => connection,
        Err(error) => {
            log::debug!("No system bus for Wi-Fi check: {error}");
            return None;
        }
    };

    let manager = match zbus::Proxy::new(
        &connection,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )
    .await
    {
        Ok(manager) => manager,
        Err(error) => {
            log::debug!("NetworkManager proxy unavailable: {error}");
            return None;
        }
    };

    let primary: OwnedObjectPath = match manager.get_property("PrimaryConnection").await {
        Ok(primary) => primary,
        Err(error) => {
            log::debug!("No primary connection path: {error}");
            return None;
        }
    };
    if primary.as_str() == "/" {
        log::debug!("No primary NetworkManager connection");
        return None;
    }

    let active = match zbus::Proxy::new(
        &connection,
        "org.freedesktop.NetworkManager",
        primary.as_str(),
        "org.freedesktop.NetworkManager.Connection.Active",
    )
    .await
    {
        Ok(active) => active,
        Err(error) => {
            log::debug!("Active connection proxy unavailable: {error}");
            return None;
        }
    };

    let connection_type: String = match active.get_property("Type").await {
        Ok(connection_type) => connection_type,
        Err(error) => {
            log::debug!("Could not read active connection type: {error}");
            return None;
        }
    };
    if connection_type != "802-11-wireless" {
        log::debug!("Active connection is not Wi-Fi: {connection_type}");
        return None;
    }

    let access_point: OwnedObjectPath = match active.get_property("SpecificObject").await {
        Ok(access_point) => access_point,
        Err(error) => {
            log::debug!("No access point object for active connection: {error}");
            return None;
        }
    };
    let access_point = match zbus::Proxy::new(
        &connection,
        "org.freedesktop.NetworkManager",
        access_point.as_str(),
        "org.freedesktop.NetworkManager.AccessPoint",
    )
    .await
    {
        Ok(access_point) => access_point,
        Err(error) => {
            log::debug!("Access point proxy unavailable: {error}");
            return None;
        }
    };

    let ssid: Vec<u8> = match access_point.get_property("Ssid").await {
        Ok(ssid) => ssid,
        Err(error) => {
            log::debug!("Could not read SSID property: {error}");
            return None;
        }
    };
    if ssid.is_empty() {
        log::debug!("Access point reported empty SSID");
        return None;
    }
    Some(String::from_utf8_lossy(&ssid).into_owned())
}
