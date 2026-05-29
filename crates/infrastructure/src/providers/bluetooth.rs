//! BlueZ Bluetooth provider.
//!
//! Subscribes to Bluetooth state via `org.bluez` service using the ObjectManager
//! interface to track adapters and connected devices.

use std::collections::HashMap;

use async_trait::async_trait;
use futures::stream::BoxStream;
use tokio_stream::StreamExt;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::Connection;

use quantum_domain::{
    Action, ActionOutcome, BluetoothDevice, BluetoothState, DomainError, Match,
    ProviderCapabilities, ProviderId, ProviderSource, Query,
};

use crate::error::InfrastructureError;

pub struct BluezProvider {
    id: ProviderId,
    conn: Option<Connection>,
    available: bool,
}

impl BluezProvider {
    /// Attempt to connect to BlueZ on the system bus.
    ///
    /// If the system bus is unavailable, returns `Ok(Self { ... available: false })`
    /// with no error — the provider degrades gracefully. If the bus is available
    /// but BlueZ is not, marks `available: false` and continues.
    pub async fn connect() -> Result<Self, InfrastructureError> {
        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(_e) => {
                return Ok(Self {
                    id: ProviderId::from("bluetooth"),
                    conn: None,
                    available: false,
                });
            }
        };

        let available = crate::providers::dbus_common::service_available(&conn, "org.bluez").await;

        Ok(Self {
            id: ProviderId::from("bluetooth"),
            conn: Some(conn),
            available,
        })
    }
}

#[async_trait]
impl ProviderSource for BluezProvider {
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
            Action::Custom { kind, payload } if kind == "bluetooth" => {
                let bt_action = parse_bluetooth_action(payload)?;
                match bt_action {
                    BluetoothAction::SetPowered(enabled) => {
                        let conn = self.conn.as_ref().ok_or_else(|| {
                            DomainError::Unsupported("bluetooth unavailable".to_string())
                        })?;

                        let proxy = zbus::Proxy::new(
                            conn,
                            "org.bluez",
                            "/org/bluez/hci0",
                            "org.bluez.Adapter1",
                        )
                        .await
                        .map_err(|e| DomainError::ActionFailed {
                            reason: format!("build adapter proxy: {e}"),
                        })?;

                        proxy.set_property("Powered", enabled).await.map_err(|e| {
                            DomainError::ActionFailed {
                                reason: format!("set powered: {e}"),
                            }
                        })?;

                        Ok(ActionOutcome {
                            message: Some(format!(
                                "bluetooth {}",
                                if enabled { "enabled" } else { "disabled" }
                            )),
                        })
                    }
                    BluetoothAction::Disconnect(addr) => {
                        let conn = self.conn.as_ref().ok_or_else(|| {
                            DomainError::Unsupported("bluetooth unavailable".to_string())
                        })?;

                        let device_path = address_to_object_path("/org/bluez/hci0", &addr);
                        let path = zbus::zvariant::ObjectPath::try_from(device_path.as_str())
                            .map_err(|e| DomainError::ActionFailed {
                                reason: format!("invalid path: {e}"),
                            })?;

                        let proxy = zbus::Proxy::new(conn, "org.bluez", path, "org.bluez.Device1")
                            .await
                            .map_err(|e| DomainError::ActionFailed {
                                reason: format!("build device proxy: {e}"),
                            })?;

                        proxy.call_method("Disconnect", &()).await.map_err(|e| {
                            DomainError::ActionFailed {
                                reason: format!("disconnect: {e}"),
                            }
                        })?;

                        Ok(ActionOutcome {
                            message: Some(format!("disconnected {}", addr)),
                        })
                    }
                }
            }
            _ => Err(DomainError::Unsupported(
                "bluetooth provider only handles custom actions with kind='bluetooth'".to_string(),
            )),
        }
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        if !self.available || self.conn.is_none() {
            return Some(crate::providers::dbus_common::unavailable_stream::<
                BluetoothState,
            >());
        }

        let conn = self.conn.as_ref().unwrap().clone();

        Some(Box::pin(async_stream::stream! {
            let mut backoff = std::time::Duration::from_secs(1);
            let max_backoff = std::time::Duration::from_secs(30);
            let mut last_emitted: Option<serde_json::Value> = None;

            loop {
                match run_bluetooth_loop(&conn, &mut last_emitted).await {
                    Ok(emissions) => {
                        for v in emissions {
                            yield v;
                        }
                        backoff = std::time::Duration::from_secs(1);
                    }
                    Err((emissions, _err)) => {
                        for v in emissions {
                            yield v;
                        }
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(max_backoff);
                    }
                }
            }
        }))
    }
}

/// Enum for parsed Bluetooth actions.
#[derive(Debug)]
pub(crate) enum BluetoothAction {
    SetPowered(bool),
    Disconnect(String),
}

/// Parse Bluetooth action payload.
///
/// Expects `command` and `value` (or `address` for disconnect).
pub(crate) fn parse_bluetooth_action(
    payload: &serde_json::Value,
) -> Result<BluetoothAction, DomainError> {
    let command = payload
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| DomainError::Unsupported("missing command".to_string()))?;

    match command {
        "set_powered" => {
            let value = payload
                .get("value")
                .and_then(|v| v.as_bool())
                .ok_or_else(|| {
                    DomainError::Unsupported("missing value for set_powered".to_string())
                })?;
            Ok(BluetoothAction::SetPowered(value))
        }
        "disconnect" => {
            let address = payload
                .get("address")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    DomainError::Unsupported("missing address for disconnect".to_string())
                })?
                .to_string();
            Ok(BluetoothAction::Disconnect(address))
        }
        _ => Err(DomainError::Unsupported(format!(
            "unknown bluetooth command: {}",
            command
        ))),
    }
}

/// Convert a Bluetooth device address (AA:BB:CC:DD:EE:FF) to a BlueZ object path.
///
/// Example: address_to_object_path("/org/bluez/hci0", "AA:BB:CC:DD:EE:FF")
///   → "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF"
pub(crate) fn address_to_object_path(adapter_path: &str, addr: &str) -> String {
    let normalized = addr.to_uppercase().replace(':', "_");
    format!("{}/dev_{}", adapter_path, normalized)
}

/// Map BlueZ managed objects to BluetoothState.
///
/// This is a pure function for testability. Iterates the object dict,
/// finds the first adapter, reads its Powered/Discovering state,
/// and collects all connected devices.
pub(crate) fn map_managed_objects(
    objects: &HashMap<
        OwnedObjectPath,
        HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>>,
    >,
) -> BluetoothState {
    let mut adapter_path: Option<String> = None;
    let mut powered = false;
    let mut discovering = false;

    let adapter_key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Adapter1").unwrap();
    let device_key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Device1").unwrap();
    let battery_key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Battery1").unwrap();

    // Find the first adapter.
    for (path, interfaces) in objects.iter() {
        if interfaces.contains_key(&adapter_key) {
            if let Some(props) = interfaces.get(&adapter_key) {
                adapter_path = Some(path.to_string());
                powered = props
                    .get("Powered")
                    .and_then(|v| bool::try_from(v).ok())
                    .unwrap_or(false);
                discovering = props
                    .get("Discovering")
                    .and_then(|v| bool::try_from(v).ok())
                    .unwrap_or(false);
            }
            break;
        }
    }

    if adapter_path.is_none() {
        return BluetoothState {
            available: false,
            powered: false,
            discovering: false,
            connected_devices: vec![],
        };
    }

    let adapter_path_str = adapter_path.unwrap();
    let mut connected_devices = Vec::new();

    // Find connected devices.
    for (path, interfaces) in objects.iter() {
        let path_str = path.to_string();
        if !path_str.starts_with(&adapter_path_str) {
            continue;
        }

        if let Some(device_props) = interfaces.get(&device_key) {
            let connected = device_props
                .get("Connected")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false);

            if !connected {
                continue;
            }

            let address = device_props
                .get("Address")
                .and_then(|v| <&str>::try_from(v).ok())
                .unwrap_or_default()
                .to_string();

            let name = device_props
                .get("Alias")
                .and_then(|v| <&str>::try_from(v).ok())
                .or_else(|| {
                    device_props
                        .get("Name")
                        .and_then(|v| <&str>::try_from(v).ok())
                })
                .unwrap_or_default()
                .to_string();

            // Try to read battery percentage from Battery1 interface if present.
            let battery_percent = interfaces.get(&battery_key).and_then(|battery_props| {
                battery_props
                    .get("Percentage")
                    .and_then(|v| u8::try_from(v).ok())
            });

            connected_devices.push(BluetoothDevice {
                address,
                name,
                battery_percent,
            });
        }
    }

    BluetoothState {
        available: true,
        powered,
        discovering,
        connected_devices,
    }
}

/// Run one Bluetooth event loop: fetch initial state, then watch for changes.
///
/// Returns (emissions, error) so the caller can flush any yielded values
/// before applying backoff.
async fn run_bluetooth_loop(
    conn: &Connection,
    last_emitted: &mut Option<serde_json::Value>,
) -> Result<Vec<serde_json::Value>, (Vec<serde_json::Value>, InfrastructureError)> {
    let mut emissions = Vec::new();

    // Open ObjectManagerProxy.
    let om_proxy = match zbus::fdo::ObjectManagerProxy::builder(conn)
        .destination("org.bluez")
        .map_err(|e| {
            (
                emissions.clone(),
                InfrastructureError::DbusTransport(e.to_string()),
            )
        })?
        .path("/")
        .map_err(|e| {
            (
                emissions.clone(),
                InfrastructureError::DbusTransport(e.to_string()),
            )
        })?
        .build()
        .await
    {
        Ok(p) => p,
        Err(e) => return Err((emissions, InfrastructureError::DbusTransport(e.to_string()))),
    };

    // Fetch and yield initial state.
    let managed_objects = match om_proxy.get_managed_objects().await {
        Ok(objs) => objs,
        Err(e) => return Err((emissions, InfrastructureError::DbusTransport(e.to_string()))),
    };

    let state = map_managed_objects(&managed_objects);
    let state_json = serde_json::to_value(&state).unwrap_or(serde_json::Value::Null);
    if last_emitted.as_ref() != Some(&state_json) {
        emissions.push(state_json.clone());
        *last_emitted = Some(state_json);
    }

    // TODO: per-device PropertiesChanged subscriptions would catch connection
    // state flips faster, but for v1 we rebuild on InterfacesAdded/Removed.
    // Devices appear/disappear from the ObjectManager view within a few
    // seconds, so the bar will update with acceptable latency.

    // Subscribe to InterfacesAdded and InterfacesRemoved.
    let mut interfaces_added = match om_proxy.receive_interfaces_added().await {
        Ok(s) => s,
        Err(e) => return Err((emissions, InfrastructureError::DbusTransport(e.to_string()))),
    };

    let mut interfaces_removed = match om_proxy.receive_interfaces_removed().await {
        Ok(s) => s,
        Err(e) => return Err((emissions, InfrastructureError::DbusTransport(e.to_string()))),
    };

    // Loop until an error occurs, listening for interface changes.
    loop {
        tokio::select! {
            _ = interfaces_added.next() => {
                // InterfacesAdded: rebuild state
            }
            _ = interfaces_removed.next() => {
                // InterfacesRemoved: rebuild state
            }
        }

        // Rebuild from current ObjectManager state.
        match om_proxy.get_managed_objects().await {
            Ok(objs) => {
                let state = map_managed_objects(&objs);
                let state_json = serde_json::to_value(&state).unwrap_or(serde_json::Value::Null);
                if last_emitted.as_ref() != Some(&state_json) {
                    emissions.push(state_json.clone());
                    *last_emitted = Some(state_json);
                }
            }
            Err(e) => return Err((emissions, InfrastructureError::DbusTransport(e.to_string()))),
        }
    }

    #[allow(unreachable_code)]
    Ok(emissions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::time::Duration;

    #[test]
    fn map_managed_objects_no_adapter() {
        let objects = HashMap::new();
        let state = map_managed_objects(&objects);
        assert!(!state.available);
        assert!(!state.powered);
        assert!(!state.discovering);
        assert!(state.connected_devices.is_empty());
    }

    #[test]
    fn map_managed_objects_adapter_no_devices() {
        let mut objects = HashMap::new();
        let mut adapter_ifaces = HashMap::new();
        let mut adapter_props = HashMap::new();
        adapter_props.insert("Powered".to_string(), OwnedValue::from(true));
        adapter_props.insert("Discovering".to_string(), OwnedValue::from(false));
        let adapter_key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Adapter1").unwrap();
        adapter_ifaces.insert(adapter_key, adapter_props);
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0").unwrap(),
            adapter_ifaces,
        );

        let state = map_managed_objects(&objects);
        assert!(state.available);
        assert!(state.powered);
        assert!(!state.discovering);
        assert!(state.connected_devices.is_empty());
    }

    #[test]
    fn map_managed_objects_adapter_one_connected_device() {
        let mut objects = HashMap::new();

        // Add adapter.
        let mut adapter_ifaces = HashMap::new();
        let mut adapter_props = HashMap::new();
        adapter_props.insert("Powered".to_string(), OwnedValue::from(true));
        adapter_props.insert("Discovering".to_string(), OwnedValue::from(false));
        let adapter_key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Adapter1").unwrap();
        adapter_ifaces.insert(adapter_key, adapter_props);
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0").unwrap(),
            adapter_ifaces,
        );

        // Add connected device.
        let mut device_ifaces = HashMap::new();
        let mut device_props = HashMap::new();
        device_props.insert("Connected".to_string(), OwnedValue::from(true));

        // For string values, use zbus::zvariant::Value first
        let addr_val: zbus::zvariant::Value = "AA:BB:CC:DD:EE:FF".into();
        device_props.insert(
            "Address".to_string(),
            OwnedValue::try_from(addr_val).unwrap(),
        );

        let alias_val: zbus::zvariant::Value = "Headphones".into();
        device_props.insert(
            "Alias".to_string(),
            OwnedValue::try_from(alias_val).unwrap(),
        );
        let device_key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Device1").unwrap();
        device_ifaces.insert(device_key, device_props);
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").unwrap(),
            device_ifaces,
        );

        let state = map_managed_objects(&objects);
        assert!(state.available);
        assert_eq!(state.connected_devices.len(), 1);
        assert_eq!(state.connected_devices[0].address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(state.connected_devices[0].name, "Headphones");
        assert_eq!(state.connected_devices[0].battery_percent, None);
    }

    #[test]
    fn map_managed_objects_skips_disconnected_devices() {
        let mut objects = HashMap::new();

        // Add adapter.
        let mut adapter_ifaces = HashMap::new();
        let mut adapter_props = HashMap::new();
        adapter_props.insert("Powered".to_string(), OwnedValue::from(true));
        adapter_props.insert("Discovering".to_string(), OwnedValue::from(false));
        let adapter_key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Adapter1").unwrap();
        adapter_ifaces.insert(adapter_key, adapter_props);
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0").unwrap(),
            adapter_ifaces,
        );

        // Add disconnected device.
        let mut device_ifaces = HashMap::new();
        let mut device_props = HashMap::new();
        device_props.insert("Connected".to_string(), OwnedValue::from(false));
        let addr_val: zbus::zvariant::Value = "AA:BB:CC:DD:EE:FF".into();
        device_props.insert(
            "Address".to_string(),
            OwnedValue::try_from(addr_val).unwrap(),
        );
        let device_key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Device1").unwrap();
        device_ifaces.insert(device_key, device_props);
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").unwrap(),
            device_ifaces,
        );

        let state = map_managed_objects(&objects);
        assert!(state.available);
        assert!(state.connected_devices.is_empty());
    }

    #[test]
    fn map_managed_objects_picks_up_battery_percent() {
        let mut objects = HashMap::new();

        // Add adapter.
        let mut adapter_ifaces = HashMap::new();
        let mut adapter_props = HashMap::new();
        adapter_props.insert("Powered".to_string(), OwnedValue::from(true));
        adapter_props.insert("Discovering".to_string(), OwnedValue::from(false));
        let adapter_key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Adapter1").unwrap();
        adapter_ifaces.insert(adapter_key, adapter_props);
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0").unwrap(),
            adapter_ifaces,
        );

        // Add device with battery.
        let mut device_ifaces = HashMap::new();
        let mut device_props = HashMap::new();
        device_props.insert("Connected".to_string(), OwnedValue::from(true));
        let addr_val: zbus::zvariant::Value = "AA:BB:CC:DD:EE:FF".into();
        device_props.insert(
            "Address".to_string(),
            OwnedValue::try_from(addr_val).unwrap(),
        );
        let alias_val: zbus::zvariant::Value = "Headphones".into();
        device_props.insert(
            "Alias".to_string(),
            OwnedValue::try_from(alias_val).unwrap(),
        );
        let device_key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Device1").unwrap();
        device_ifaces.insert(device_key, device_props);

        let mut battery_props = HashMap::new();
        battery_props.insert("Percentage".to_string(), OwnedValue::from(60_u8));
        let battery_key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Battery1").unwrap();
        device_ifaces.insert(battery_key, battery_props);

        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").unwrap(),
            device_ifaces,
        );

        let state = map_managed_objects(&objects);
        assert!(state.available);
        assert_eq!(state.connected_devices.len(), 1);
        assert_eq!(state.connected_devices[0].battery_percent, Some(60));
    }

    #[test]
    fn address_to_object_path_uppercases_lowercase() {
        let path = address_to_object_path("/org/bluez/hci0", "aa:bb:cc:dd:ee:ff");
        assert_eq!(path, "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF");
    }

    #[test]
    fn address_to_object_path_uppercases_mixed_case() {
        let path = address_to_object_path("/org/bluez/hci0", "Aa:Bb:Cc:Dd:Ee:Ff");
        assert_eq!(path, "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF");
    }

    #[test]
    fn parses_set_powered_true() {
        let payload = serde_json::json!({"command": "set_powered", "value": true});
        let action = parse_bluetooth_action(&payload);
        assert!(matches!(action, Ok(BluetoothAction::SetPowered(true))));
    }

    #[test]
    fn parses_set_powered_false() {
        let payload = serde_json::json!({"command": "set_powered", "value": false});
        let action = parse_bluetooth_action(&payload);
        assert!(matches!(action, Ok(BluetoothAction::SetPowered(false))));
    }

    #[test]
    fn parses_disconnect() {
        let payload = serde_json::json!({"command": "disconnect", "address": "AA:BB:CC:DD:EE:FF"});
        let action = parse_bluetooth_action(&payload);
        assert!(matches!(
            action,
            Ok(BluetoothAction::Disconnect(ref addr)) if addr == "AA:BB:CC:DD:EE:FF"
        ));
    }

    #[test]
    fn rejects_unknown_command() {
        let payload = serde_json::json!({"command": "unknown"});
        let action = parse_bluetooth_action(&payload);
        assert!(action.is_err());
    }

    #[test]
    fn rejects_set_powered_missing_value() {
        let payload = serde_json::json!({"command": "set_powered"});
        let action = parse_bluetooth_action(&payload);
        assert!(action.is_err());
    }

    #[test]
    fn rejects_disconnect_missing_address() {
        let payload = serde_json::json!({"command": "disconnect"});
        let action = parse_bluetooth_action(&payload);
        assert!(action.is_err());
    }

    #[tokio::test]
    #[ignore = "requires real BlueZ"]
    async fn yields_initial_state_within_2s() {
        let p = BluezProvider::connect().await.expect("connect");
        if let Some(mut s) = p.subscribe() {
            let v = tokio::time::timeout(Duration::from_secs(2), s.next())
                .await
                .expect("first state within 2s")
                .expect("Some");
            let _state: BluetoothState = serde_json::from_value(v).expect("BluetoothState");
        }
    }
}
