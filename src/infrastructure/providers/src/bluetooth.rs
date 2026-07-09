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
    Action, ActionOutcome, BluetoothDevice, BluetoothState, DomainError, Match, ProviderId,
    ProviderSource, Query,
};

use crate::error::ProvidersError;

pub struct BluezProvider {
    id: ProviderId,
    conn: Option<Connection>,
}

impl BluezProvider {
    /// Attempt to connect to BlueZ on the system bus.
    ///
    /// If the system bus is unavailable, returns `Ok(Self { conn: None })` with no
    /// error -- the provider degrades gracefully. When the bus is available but
    /// BlueZ is not, `service_lifecycle_stream` polls availability and switches to
    /// a real subscription once BlueZ appears.
    pub async fn connect() -> Result<Self, ProvidersError> {
        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(_e) => {
                return Ok(Self {
                    id: ProviderId::from("bluetooth"),
                    conn: None,
                });
            }
        };

        Ok(Self {
            id: ProviderId::from("bluetooth"),
            conn: Some(conn),
        })
    }
}

#[async_trait]
impl ProviderSource for BluezProvider {
    fn id(&self) -> &ProviderId {
        &self.id
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
        let conn = match self.conn.as_ref() {
            Some(c) => c.clone(),
            None => {
                #[allow(deprecated)]
                return Some(quantum_dbus::common::unavailable_stream::<BluetoothState>());
            }
        };

        Some(quantum_dbus::common::service_lifecycle_stream::<
            BluetoothState,
            _,
        >(conn, "org.bluez", |conn: Connection| {
            bluez_managed_objects_stream(conn)
        }))
    }
}

/// Build the inner ObjectManager-driven stream for BlueZ.
///
/// This is the original property-subscription-shaped loop that watches
/// `InterfacesAdded` / `InterfacesRemoved` on `org.bluez` root. It is
/// wrapped by `service_lifecycle_stream` so that when `org.bluez`
/// disappears (or was never present at startup) the outer loop
/// recovers automatically.
fn bluez_managed_objects_stream(conn: Connection) -> BoxStream<'static, serde_json::Value> {
    Box::pin(async_stream::stream! {
        let mut backoff = std::time::Duration::from_secs(1);
        let max_backoff = std::time::Duration::from_secs(30);
        let mut last_emitted: Option<serde_json::Value> = None;

        loop {
            // Open ObjectManagerProxy at org.bluez root.
            let om_proxy = match zbus::fdo::ObjectManagerProxy::builder(&conn)
                .destination("org.bluez")
                .and_then(|b| b.path("/"))
                .map(|b| b.build())
            {
                Ok(fut) => match fut.await {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(error = %e, "bluez ObjectManagerProxy build failed");
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(max_backoff);
                        continue;
                    }
                },
                Err(e) => {
                    tracing::warn!(error = %e, "bluez ObjectManagerProxy builder failed");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                    continue;
                }
            };

            // Initial state.
            match om_proxy.get_managed_objects().await {
                Ok(objs) => {
                    let state = map_managed_objects(&objs);
                    let v = serde_json::to_value(&state).unwrap_or(serde_json::Value::Null);
                    if last_emitted.as_ref() != Some(&v) {
                        last_emitted = Some(v.clone());
                        yield v;
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "bluez get_managed_objects failed");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                    continue;
                }
            }

            // Subscribe to InterfacesAdded / InterfacesRemoved.
            let mut interfaces_added = match om_proxy.receive_interfaces_added().await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "bluez receive_interfaces_added failed");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                    continue;
                }
            };
            let mut interfaces_removed = match om_proxy.receive_interfaces_removed().await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "bluez receive_interfaces_removed failed");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                    continue;
                }
            };

            backoff = std::time::Duration::from_secs(1);

            // Yield deduped rebuilds on each event.
            let mut clean = true;
            loop {
                tokio::select! {
                    next = interfaces_added.next() => {
                        if next.is_none() { break; }
                    }
                    next = interfaces_removed.next() => {
                        if next.is_none() { break; }
                    }
                }
                match om_proxy.get_managed_objects().await {
                    Ok(objs) => {
                        let state = map_managed_objects(&objs);
                        let v = serde_json::to_value(&state).unwrap_or(serde_json::Value::Null);
                        if last_emitted.as_ref() != Some(&v) {
                            last_emitted = Some(v.clone());
                            yield v;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "bluez get_managed_objects failed during loop");
                        clean = false;
                        break;
                    }
                }
            }

            if !clean {
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
        }
    })
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

    // Pick the adapter with the lexicographically smallest object path so
    // that hosts with multiple adapters (for example hci0 and hci1) produce
    // a stable BluetoothState across rebuilds. HashMap iteration order is
    // unspecified, so iterating with break would otherwise flap.
    if let Some((path, interfaces)) = objects
        .iter()
        .filter(|(_, ifaces)| ifaces.contains_key(&adapter_key))
        .min_by_key(|(path, _)| path.as_str().to_string())
    {
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
    }

    let Some(adapter_path_str) = adapter_path else {
        return BluetoothState {
            available: false,
            powered: false,
            discovering: false,
            devices: vec![],
            adapter_path: String::new(),
        };
    };
    let mut devices = Vec::new();

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

            devices.push(BluetoothDevice {
                address,
                name,
                battery_percent,
                paired: false,
                trusted: false,
                connected: true,
                icon: None,
                rssi: None,
            });
        }
    }

    BluetoothState {
        available: true,
        powered,
        discovering,
        devices,
        adapter_path: adapter_path_str,
    }
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
        assert!(state.devices.is_empty());
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
        assert!(state.devices.is_empty());
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
        assert_eq!(state.devices.len(), 1);
        assert_eq!(state.devices[0].address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(state.devices[0].name, "Headphones");
        assert_eq!(state.devices[0].battery_percent, None);
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
        assert!(state.devices.is_empty());
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
        assert_eq!(state.devices.len(), 1);
        assert_eq!(state.devices[0].battery_percent, Some(60));
    }

    #[test]
    fn selects_adapter_with_lowest_path() {
        // Build a two-adapter HashMap many times so HashMap iteration order
        // varies, and confirm the result never depends on iteration order.
        // hci0 is powered=true/discovering=false; hci1 is the opposite. If
        // selection is non-deterministic, at least one iteration will pick
        // hci1 and flip the asserted values. OwnedValue is not Clone, so the
        // map is rebuilt from scratch each loop iteration.
        for _ in 0..64 {
            let adapter_key =
                zbus::names::OwnedInterfaceName::try_from("org.bluez.Adapter1").unwrap();

            let mut hci1_ifaces = HashMap::new();
            let mut hci1_props = HashMap::new();
            hci1_props.insert("Powered".to_string(), OwnedValue::from(false));
            hci1_props.insert("Discovering".to_string(), OwnedValue::from(true));
            hci1_ifaces.insert(adapter_key.clone(), hci1_props);

            let mut hci0_ifaces = HashMap::new();
            let mut hci0_props = HashMap::new();
            hci0_props.insert("Powered".to_string(), OwnedValue::from(true));
            hci0_props.insert("Discovering".to_string(), OwnedValue::from(false));
            hci0_ifaces.insert(adapter_key, hci0_props);

            let mut objects = HashMap::new();
            objects.insert(
                OwnedObjectPath::try_from("/org/bluez/hci1").unwrap(),
                hci1_ifaces,
            );
            objects.insert(
                OwnedObjectPath::try_from("/org/bluez/hci0").unwrap(),
                hci0_ifaces,
            );

            let state = map_managed_objects(&objects);
            assert!(state.available);
            assert!(state.powered, "selected wrong adapter (got hci1)");
            assert!(!state.discovering, "selected wrong adapter (got hci1)");
        }
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
