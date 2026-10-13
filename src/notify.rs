//! Best-effort desktop notifications for command failures.
//!
//! Uses `org.freedesktop.Notifications` on the session bus (provided by
//! Plasma). Everything is swallowed: a missing notification service must
//! never break controlling the ACs.

use std::collections::HashMap;

/// Tell the user that a command failed. Returns immediately after trying.
pub async fn command_failed(body: &str) {
    let Ok(connection) = zbus::Connection::session().await else {
        return;
    };
    let Ok(proxy) = zbus::Proxy::new(
        &connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .await
    else {
        return;
    };
    let hints = HashMap::<String, zbus::zvariant::Value>::new();
    let _: Result<u32, _> = proxy
        .call(
            "Notify",
            &(
                "gree-ac-plasma",
                0u32,
                "temperature-normal",
                "Gree AC command failed",
                body,
                Vec::<String>::new(),
                hints,
                8000i32,
            ),
        )
        .await;
}
