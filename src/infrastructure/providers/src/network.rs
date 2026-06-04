//! NetworkManager network state provider.
//!
//! Subscribes to network state via `org.freedesktop.NetworkManager` service,
//! reading connectivity, primary connection type, and WiFi state.

use async_trait::async_trait;
use futures::stream::BoxStream;
use zbus::Connection;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, NetworkConnection, NetworkConnectivity, NetworkKind,
    NetworkState, ProviderCapabilities, ProviderId, ProviderSource, Query,
};

use quantum_dbus::DbusError;

use crate::error::ProvidersError;

pub struct NetworkManagerProvider {
    id: ProviderId,
    conn: Option<Connection>,
}

impl NetworkManagerProvider {
    /// Attempt to connect to NetworkManager on the system bus.
    ///
    /// If the system bus is unavailable, returns `Ok(Self { conn: None })` with no
    /// error -- the provider degrades gracefully. When the bus is available but
    /// NetworkManager is not, `service_lifecycle_stream` polls availability and
    /// switches to a real subscription once NetworkManager appears.
    pub async fn connect() -> Result<Self, ProvidersError> {
        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(_e) => {
                return Ok(Self {
                    id: ProviderId::from("network"),
                    conn: None,
                });
            }
        };

        Ok(Self {
            id: ProviderId::from("network"),
            conn: Some(conn),
        })
    }
}

#[async_trait]
impl ProviderSource for NetworkManagerProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            searchable: false,
            streamable: true,
        }
    }

    async fn search(&self, _: &Query) -> Result<Vec<Match>, DomainError> {
        Ok(vec![])
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Custom { kind, payload } if kind == "network" => {
                let network_action = parse_network_action(payload)?;
                match network_action {
                    NetworkAction::SetWifiEnabled(enabled) => {
                        let conn = self.conn.as_ref().ok_or_else(|| {
                            DomainError::Unsupported("network manager not available".to_string())
                        })?;

                        let proxy = zbus::Proxy::new(
                            conn,
                            "org.freedesktop.NetworkManager",
                            "/org/freedesktop/NetworkManager",
                            "org.freedesktop.NetworkManager",
                        )
                        .await
                        .map_err(|e| DomainError::ActionFailed {
                            reason: format!("build proxy: {e}"),
                        })?;

                        proxy
                            .set_property("WirelessEnabled", enabled)
                            .await
                            .map_err(|e| DomainError::ActionFailed {
                                reason: format!("set wifi enabled: {e}"),
                            })?;

                        Ok(ActionOutcome {
                            message: Some(format!(
                                "set wifi {}",
                                if enabled { "enabled" } else { "disabled" }
                            )),
                        })
                    }
                }
            }
            _ => Err(DomainError::Unsupported(
                "network provider only handles custom actions with kind='network'".to_string(),
            )),
        }
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        let conn = match self.conn.as_ref() {
            Some(c) => c.clone(),
            None => {
                #[allow(deprecated)]
                return Some(quantum_dbus::common::unavailable_stream::<NetworkState>());
            }
        };

        Some(quantum_dbus::common::service_lifecycle_stream::<
            NetworkState,
            _,
        >(
            conn,
            "org.freedesktop.NetworkManager",
            |conn: Connection| {
                let build: quantum_dbus::common::BuildFn<NetworkState> =
                    Box::new(|conn: &Connection| {
                        Box::pin(async {
                            let proxy = zbus::Proxy::new(
                                conn,
                                "org.freedesktop.NetworkManager",
                                "/org/freedesktop/NetworkManager",
                                "org.freedesktop.NetworkManager",
                            )
                            .await
                            .map_err(|e| DbusError::Transport(e.to_string()))?;

                            let connectivity: u32 =
                                proxy.get_property("Connectivity").await.unwrap_or(0);
                            let wifi_enabled: bool =
                                proxy.get_property("WirelessEnabled").await.unwrap_or(false);
                            let primary_path: zbus::zvariant::OwnedObjectPath = proxy
                                .get_property("PrimaryConnection")
                                .await
                                .unwrap_or_else(|_| {
                                    zbus::zvariant::OwnedObjectPath::from(
                                        zbus::zvariant::ObjectPath::try_from("/").unwrap(),
                                    )
                                });

                            let (primary, wifi_signal_percent) = if primary_path.as_str() != "/" {
                                match build_primary_connection(conn, &primary_path).await {
                                    Ok((conn_info, strength)) => (Some(conn_info), strength),
                                    Err(_) => (None, None),
                                }
                            } else {
                                (None, None)
                            };

                            Ok(NetworkState {
                                available: true,
                                connectivity: map_connectivity(connectivity),
                                primary,
                                wifi_enabled,
                                wifi_signal_percent,
                            })
                        })
                    });

                quantum_dbus::common::property_subscription_stream(
                    conn,
                    "org.freedesktop.NetworkManager",
                    "/org/freedesktop/NetworkManager",
                    "org.freedesktop.NetworkManager",
                    build,
                )
            },
        ))
    }
}

/// Build primary connection details from a Connection.Active object
/// path. Returns the connection and (for wifi) the signal strength.
async fn build_primary_connection(
    conn: &Connection,
    primary_path: &zbus::zvariant::OwnedObjectPath,
) -> Result<(NetworkConnection, Option<u8>), DbusError> {
    let proxy = zbus::Proxy::new(
        conn,
        "org.freedesktop.NetworkManager",
        primary_path.clone(),
        "org.freedesktop.NetworkManager.Connection.Active",
    )
    .await
    .map_err(|e| DbusError::Transport(e.to_string()))?;

    let connection_type: String = proxy
        .get_property("Type")
        .await
        .unwrap_or_else(|_| "other".to_string());
    let id: String = proxy
        .get_property("Id")
        .await
        .unwrap_or_else(|_| "unknown".to_string());

    let kind = map_nm_connection_type(&connection_type);

    let (ssid, strength) = if kind == NetworkKind::Wifi {
        match read_wifi_access_point(conn, &proxy).await {
            Ok((s, st)) => (Some(s), st),
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };

    Ok((NetworkConnection { kind, id, ssid }, strength))
}

/// Read SSID and signal strength from the WiFi access point associated
/// with this active connection.
async fn read_wifi_access_point(
    conn: &Connection,
    active_proxy: &zbus::Proxy<'_>,
) -> Result<(String, Option<u8>), DbusError> {
    let specific_object: zbus::zvariant::OwnedObjectPath = active_proxy
        .get_property("SpecificObject")
        .await
        .map_err(|e| DbusError::Transport(e.to_string()))?;

    if specific_object.as_str() == "/" {
        return Err(DbusError::Transport(
            "no specific object for wifi".to_string(),
        ));
    }

    let ap_proxy = zbus::Proxy::new(
        conn,
        "org.freedesktop.NetworkManager",
        specific_object,
        "org.freedesktop.NetworkManager.AccessPoint",
    )
    .await
    .map_err(|e| DbusError::Transport(e.to_string()))?;

    let ssid_bytes: Vec<u8> = ap_proxy
        .get_property("Ssid")
        .await
        .map_err(|e| DbusError::Transport(e.to_string()))?;

    let ssid = String::from_utf8(ssid_bytes).map_err(|e| DbusError::Transport(e.to_string()))?;

    let strength: Option<u8> = ap_proxy.get_property("Strength").await.ok();

    Ok((ssid, strength))
}

/// Map NetworkManager connectivity u32 to NetworkConnectivity enum.
///
/// Per NM documentation: 1=None, 2=Portal, 3=Limited, 4=Full, else=Unknown.
pub(crate) fn map_connectivity(v: u32) -> NetworkConnectivity {
    match v {
        1 => NetworkConnectivity::None,
        2 => NetworkConnectivity::Portal,
        3 => NetworkConnectivity::Limited,
        4 => NetworkConnectivity::Full,
        _ => NetworkConnectivity::Unknown,
    }
}

/// Map NetworkManager connection Type string to NetworkKind.
///
/// "802-3-ethernet" → Ethernet
/// "802-11-wireless" → Wifi
/// "gsm", "cdma" → Cellular
/// "vpn", "wireguard", "openvpn", "tun" → Vpn
/// Anything else → Other
pub(crate) fn map_nm_connection_type(s: &str) -> NetworkKind {
    match s {
        "802-3-ethernet" => NetworkKind::Ethernet,
        "802-11-wireless" => NetworkKind::Wifi,
        "gsm" | "cdma" => NetworkKind::Cellular,
        "vpn" | "wireguard" | "openvpn" | "tun" => NetworkKind::Vpn,
        _ => NetworkKind::Other,
    }
}

/// Parsed network action from JSON payload.
pub(crate) enum NetworkAction {
    SetWifiEnabled(bool),
}

/// Parse a network action from a JSON payload.
///
/// Expects `{"command": "set_wifi_enabled", "value": bool}`.
pub(crate) fn parse_network_action(
    payload: &serde_json::Value,
) -> Result<NetworkAction, DomainError> {
    let command = payload
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            DomainError::Unsupported("missing or non-string command in network action".to_string())
        })?;

    match command {
        "set_wifi_enabled" => {
            let value = payload
                .get("value")
                .and_then(|v| v.as_bool())
                .ok_or_else(|| {
                    DomainError::Unsupported(
                        "missing or non-bool value for set_wifi_enabled".to_string(),
                    )
                })?;
            Ok(NetworkAction::SetWifiEnabled(value))
        }
        _ => Err(DomainError::Unsupported(format!(
            "unknown network command: {}",
            command
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_connectivity_known_values() {
        assert_eq!(map_connectivity(1), NetworkConnectivity::None);
        assert_eq!(map_connectivity(2), NetworkConnectivity::Portal);
        assert_eq!(map_connectivity(3), NetworkConnectivity::Limited);
        assert_eq!(map_connectivity(4), NetworkConnectivity::Full);
        assert_eq!(map_connectivity(99), NetworkConnectivity::Unknown);
    }

    #[test]
    fn map_nm_connection_type_categorises() {
        assert_eq!(
            map_nm_connection_type("802-3-ethernet"),
            NetworkKind::Ethernet
        );
        assert_eq!(map_nm_connection_type("802-11-wireless"), NetworkKind::Wifi);
        assert_eq!(map_nm_connection_type("gsm"), NetworkKind::Cellular);
        assert_eq!(map_nm_connection_type("cdma"), NetworkKind::Cellular);
        assert_eq!(map_nm_connection_type("vpn"), NetworkKind::Vpn);
        assert_eq!(map_nm_connection_type("wireguard"), NetworkKind::Vpn);
        assert_eq!(map_nm_connection_type("openvpn"), NetworkKind::Vpn);
        assert_eq!(map_nm_connection_type("tun"), NetworkKind::Vpn);
        assert_eq!(map_nm_connection_type("garbage"), NetworkKind::Other);
    }

    #[test]
    fn parses_set_wifi_enabled_true() {
        let p = serde_json::json!({"command":"set_wifi_enabled","value":true});
        match parse_network_action(&p) {
            Ok(NetworkAction::SetWifiEnabled(true)) => {}
            _ => panic!("expected SetWifiEnabled(true)"),
        }
    }

    #[test]
    fn parses_set_wifi_enabled_false() {
        let p = serde_json::json!({"command":"set_wifi_enabled","value":false});
        match parse_network_action(&p) {
            Ok(NetworkAction::SetWifiEnabled(false)) => {}
            _ => panic!("expected SetWifiEnabled(false)"),
        }
    }

    #[test]
    fn rejects_unknown_command() {
        let p = serde_json::json!({"command":"reticulate_splines"});
        assert!(parse_network_action(&p).is_err());
    }

    #[test]
    fn rejects_missing_command() {
        let p = serde_json::json!({});
        assert!(parse_network_action(&p).is_err());
    }

    #[test]
    fn rejects_non_bool_value() {
        let p = serde_json::json!({"command":"set_wifi_enabled","value":"yes"});
        assert!(parse_network_action(&p).is_err());
    }

    #[tokio::test]
    #[ignore = "requires real NetworkManager"]
    async fn yields_initial_state_within_2s() {
        use futures::StreamExt;
        use std::time::Duration;

        let p = NetworkManagerProvider::connect().await.expect("connect");
        let mut stream = p.subscribe().expect("subscribe");
        let v = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("first state within 2s")
            .expect("Some");
        let _state: NetworkState = serde_json::from_value(v).expect("NetworkState");
    }
}
