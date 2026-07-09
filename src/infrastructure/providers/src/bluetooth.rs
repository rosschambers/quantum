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

use std::sync::Arc;

use crate::bluetooth_agent::{
    BluezPairingAgent, PairingReply, PendingPairingMap, AGENT_OBJECT_PATH,
};
use crate::error::ProvidersError;

pub struct BluezProvider {
    id: ProviderId,
    conn: Option<Connection>,
    pairing: Arc<PendingPairingMap>,
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
                    pairing: Arc::new(PendingPairingMap::new()),
                });
            }
        };

        Ok(Self {
            id: ProviderId::from("bluetooth"),
            conn: Some(conn),
            pairing: Arc::new(PendingPairingMap::new()),
        })
    }
}

/// Discovery start/stop failures that mean "already in the desired state".
/// Treated as success so open_session/close_session are idempotent.
pub(crate) fn discovery_error_is_ignorable(message: &str) -> bool {
    message.contains("org.bluez.Error.InProgress") || message.contains("No discovery started")
}

impl BluezProvider {
    fn connection(&self) -> Result<&Connection, DomainError> {
        self.conn
            .as_ref()
            .ok_or_else(|| DomainError::Unsupported("bluetooth unavailable".to_string()))
    }

    /// Resolve the adapter object path from live managed objects, replacing
    /// the old hard-coded `/org/bluez/hci0`.
    async fn resolve_adapter_path(&self) -> Result<String, DomainError> {
        let connection = self.connection()?;
        let object_manager = zbus::fdo::ObjectManagerProxy::builder(connection)
            .destination("org.bluez")
            .and_then(|builder| builder.path("/"))
            .map_err(|error| DomainError::ActionFailed {
                reason: format!("build object manager proxy: {error}"),
            })?
            .build()
            .await
            .map_err(|error| DomainError::ActionFailed {
                reason: format!("connect object manager proxy: {error}"),
            })?;
        let objects = object_manager
            .get_managed_objects()
            .await
            .map_err(|error| DomainError::ActionFailed {
                reason: format!("get managed objects: {error}"),
            })?;
        first_adapter_path(&objects)
            .ok_or_else(|| DomainError::Unsupported("no bluetooth adapter present".to_string()))
    }

    async fn adapter_proxy(&self) -> Result<zbus::Proxy<'_>, DomainError> {
        let connection = self.connection()?;
        let adapter_path = self.resolve_adapter_path().await?;
        zbus::Proxy::new(connection, "org.bluez", adapter_path, "org.bluez.Adapter1")
            .await
            .map_err(|error| DomainError::ActionFailed {
                reason: format!("build adapter proxy: {error}"),
            })
    }

    async fn device_proxy(&self, address: &str) -> Result<zbus::Proxy<'_>, DomainError> {
        let connection = self.connection()?;
        let adapter_path = self.resolve_adapter_path().await?;
        let device_path = address_to_object_path(&adapter_path, address);
        zbus::Proxy::new(connection, "org.bluez", device_path, "org.bluez.Device1")
            .await
            .map_err(|error| DomainError::ActionFailed {
                reason: format!("build device proxy: {error}"),
            })
    }

    async fn call_device_method(
        &self,
        address: &str,
        method: &'static str,
    ) -> Result<(), DomainError> {
        let proxy = self.device_proxy(address).await?;
        proxy
            .call_method(method, &())
            .await
            .map_err(|error| DomainError::ActionFailed {
                reason: format!("{method} on {address}: {error}"),
            })?;
        Ok(())
    }

    async fn set_discovery(&self, running: bool) -> Result<(), DomainError> {
        let proxy = self.adapter_proxy().await?;
        let method = if running {
            "StartDiscovery"
        } else {
            "StopDiscovery"
        };
        match proxy.call_method(method, &()).await {
            Ok(_) => Ok(()),
            Err(error) if discovery_error_is_ignorable(&error.to_string()) => Ok(()),
            Err(error) => Err(DomainError::ActionFailed {
                reason: format!("{method}: {error}"),
            }),
        }
    }

    /// Serve the pairing agent on the system bus and register it as the
    /// default agent with BlueZ. Called once from daemon startup, after
    /// provider registration. Failure is non-fatal: pairing prompts simply
    /// will not work until the daemon restarts with BlueZ present.
    pub async fn start_pairing_agent(
        &self,
        event_bus: std::sync::Arc<dyn quantum_domain::EventBus>,
    ) -> Result<(), ProvidersError> {
        let connection = self.conn.as_ref().ok_or_else(|| {
            ProvidersError::ServiceUnavailable("no system bus for pairing agent".to_string())
        })?;
        let agent = BluezPairingAgent::new(self.pairing.clone(), event_bus);
        connection
            .object_server()
            .at(AGENT_OBJECT_PATH, agent)
            .await
            .map_err(|error| {
                ProvidersError::ServiceUnavailable(format!("serve pairing agent: {error}"))
            })?;
        let manager = zbus::Proxy::new(
            connection,
            "org.bluez",
            "/org/bluez",
            "org.bluez.AgentManager1",
        )
        .await
        .map_err(|error| {
            ProvidersError::ServiceUnavailable(format!("agent manager proxy: {error}"))
        })?;
        let agent_path = zbus::zvariant::ObjectPath::try_from(AGENT_OBJECT_PATH)
            .map_err(|error| ProvidersError::ServiceUnavailable(format!("agent path: {error}")))?;
        manager
            .call_method("RegisterAgent", &(&agent_path, "KeyboardDisplay"))
            .await
            .map_err(|error| {
                ProvidersError::ServiceUnavailable(format!("RegisterAgent: {error}"))
            })?;
        manager
            .call_method("RequestDefaultAgent", &(&agent_path,))
            .await
            .map_err(|error| {
                ProvidersError::ServiceUnavailable(format!("RequestDefaultAgent: {error}"))
            })?;
        tracing::info!("bluetooth pairing agent registered (KeyboardDisplay)");
        Ok(())
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
                        let proxy = self.adapter_proxy().await?;
                        proxy
                            .set_property("Powered", enabled)
                            .await
                            .map_err(|error| DomainError::ActionFailed {
                                reason: format!("set powered: {error}"),
                            })?;
                        Ok(ActionOutcome {
                            message: Some(format!(
                                "bluetooth {}",
                                if enabled { "enabled" } else { "disabled" }
                            )),
                        })
                    }
                    BluetoothAction::Disconnect(address) => {
                        self.call_device_method(&address, "Disconnect").await?;
                        Ok(ActionOutcome {
                            message: Some(format!("disconnected {address}")),
                        })
                    }
                    BluetoothAction::StartDiscovery | BluetoothAction::OpenSession => {
                        self.set_discovery(true).await?;
                        Ok(ActionOutcome { message: None })
                    }
                    BluetoothAction::StopDiscovery | BluetoothAction::CloseSession => {
                        self.set_discovery(false).await?;
                        Ok(ActionOutcome { message: None })
                    }
                    BluetoothAction::Connect { address } => {
                        self.call_device_method(&address, "Connect").await?;
                        Ok(ActionOutcome { message: None })
                    }
                    BluetoothAction::Pair { address } => {
                        self.call_device_method(&address, "Pair").await?;
                        Ok(ActionOutcome { message: None })
                    }
                    BluetoothAction::SetTrusted { address, value } => {
                        let proxy = self.device_proxy(&address).await?;
                        proxy
                            .set_property("Trusted", value)
                            .await
                            .map_err(|error| DomainError::ActionFailed {
                                reason: format!("set trusted: {error}"),
                            })?;
                        Ok(ActionOutcome { message: None })
                    }
                    BluetoothAction::Remove { address } => {
                        let adapter_path = self.resolve_adapter_path().await?;
                        let device_path = address_to_object_path(&adapter_path, &address);
                        let object_path = zbus::zvariant::ObjectPath::try_from(
                            device_path.as_str(),
                        )
                        .map_err(|error| DomainError::ActionFailed {
                            reason: format!("invalid device path: {error}"),
                        })?;
                        let proxy = self.adapter_proxy().await?;
                        proxy
                            .call_method("RemoveDevice", &(object_path,))
                            .await
                            .map_err(|error| DomainError::ActionFailed {
                                reason: format!("remove device: {error}"),
                            })?;
                        Ok(ActionOutcome { message: None })
                    }
                    BluetoothAction::PairingResponse {
                        address,
                        accept,
                        passkey,
                        pin,
                    } => {
                        let reply = if !accept {
                            PairingReply::Reject
                        } else if let Some(pin) = pin {
                            PairingReply::PinCode(pin)
                        } else if let Some(passkey) = passkey {
                            PairingReply::Passkey(passkey)
                        } else {
                            PairingReply::Confirm
                        };
                        if self.pairing.resolve_by_address(&address, reply).await {
                            Ok(ActionOutcome { message: None })
                        } else {
                            Err(DomainError::ActionFailed {
                                reason: format!("no pending pairing request for {address}"),
                            })
                        }
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

/// Match rule for `PropertiesChanged` signals emitted by the BlueZ name owner.
///
/// The rule matches on the resolved UNIQUE name, not the well-known
/// "org.bluez": signal headers carry the unique sender, and matching on the
/// unique name works for both the bus daemon and zbus's client-side filter.
pub(crate) fn bluez_properties_changed_rule(
    sender_unique_name: &str,
) -> Result<zbus::OwnedMatchRule, zbus::Error> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(sender_unique_name)?
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .build();
    Ok(rule.into())
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

            // Resolve the unique owner of org.bluez and subscribe to its
            // PropertiesChanged signals so connect/disconnect/pairing property
            // flips refresh state live (an interface add/remove never fires
            // for a plain property change). If org.bluez restarts, these
            // streams end and the outer loop resubscribes to the new owner.
            let owner = match zbus::fdo::DBusProxy::new(&conn).await {
                Ok(dbus_proxy) => {
                    match zbus::names::BusName::try_from("org.bluez") {
                        Ok(bus_name) => match dbus_proxy.get_name_owner(bus_name).await {
                            Ok(owner) => owner,
                            Err(e) => {
                                tracing::warn!(error = %e, "bluez get_name_owner failed");
                                tokio::time::sleep(backoff).await;
                                backoff = (backoff * 2).min(max_backoff);
                                continue;
                            }
                        },
                        Err(e) => {
                            tracing::warn!(error = %e, "bluez bus name invalid");
                            tokio::time::sleep(backoff).await;
                            backoff = (backoff * 2).min(max_backoff);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "DBusProxy build failed");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                    continue;
                }
            };
            let properties_rule = match bluez_properties_changed_rule(owner.as_str()) {
                Ok(rule) => rule,
                Err(e) => {
                    tracing::warn!(error = %e, "bluez properties match rule build failed");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                    continue;
                }
            };
            let mut properties_changed =
                match zbus::MessageStream::for_match_rule(properties_rule, &conn, None).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        tracing::warn!(error = %e, "bluez PropertiesChanged subscribe failed");
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
                    next = properties_changed.next() => {
                        // A malformed message (Some(Err)) still triggers a
                        // rebuild; dedupe suppresses no-op emissions.
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

/// A typed Bluetooth command parsed from the JSON payload the frontend sends
/// through `action.invoke`. Each variant maps to a snake_case `command` string.
#[derive(Debug)]
pub(crate) enum BluetoothAction {
    SetPowered(bool),
    Disconnect(String),
    StartDiscovery,
    StopDiscovery,
    OpenSession,
    CloseSession,
    Connect {
        address: String,
    },
    Pair {
        address: String,
    },
    SetTrusted {
        address: String,
        value: bool,
    },
    Remove {
        address: String,
    },
    PairingResponse {
        address: String,
        accept: bool,
        passkey: Option<u32>,
        pin: Option<String>,
    },
}

fn required_string_field(payload: &serde_json::Value, key: &str) -> Result<String, DomainError> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| {
            DomainError::Unsupported(format!("missing or non-string '{key}' in bluetooth action"))
        })
}

fn required_bool_field(payload: &serde_json::Value, key: &str) -> Result<bool, DomainError> {
    payload
        .get(key)
        .and_then(|value| value.as_bool())
        .ok_or_else(|| {
            DomainError::Unsupported(format!("missing or non-bool '{key}' in bluetooth action"))
        })
}

fn optional_string_field(payload: &serde_json::Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

/// BlueZ passkeys are six decimal digits. Absent or null means "no passkey".
fn optional_passkey_field(payload: &serde_json::Value) -> Result<Option<u32>, DomainError> {
    match payload.get("passkey") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|number| *number <= 999_999)
            .map(|number| Some(number as u32))
            .ok_or_else(|| {
                DomainError::Unsupported(
                    "passkey must be an integer in 0..=999999 in bluetooth action".to_string(),
                )
            }),
    }
}

/// Parse a Bluetooth action from a JSON payload.
pub(crate) fn parse_bluetooth_action(
    payload: &serde_json::Value,
) -> Result<BluetoothAction, DomainError> {
    let command = payload
        .get("command")
        .and_then(|value| value.as_str())
        .ok_or_else(|| DomainError::Unsupported("missing command".to_string()))?;

    match command {
        "set_powered" => Ok(BluetoothAction::SetPowered(required_bool_field(
            payload, "value",
        )?)),
        "disconnect" => Ok(BluetoothAction::Disconnect(required_string_field(
            payload, "address",
        )?)),
        "start_discovery" => Ok(BluetoothAction::StartDiscovery),
        "stop_discovery" => Ok(BluetoothAction::StopDiscovery),
        "open_session" => Ok(BluetoothAction::OpenSession),
        "close_session" => Ok(BluetoothAction::CloseSession),
        "connect" => Ok(BluetoothAction::Connect {
            address: required_string_field(payload, "address")?,
        }),
        "pair" => Ok(BluetoothAction::Pair {
            address: required_string_field(payload, "address")?,
        }),
        "set_trusted" => Ok(BluetoothAction::SetTrusted {
            address: required_string_field(payload, "address")?,
            value: required_bool_field(payload, "value")?,
        }),
        "remove" => Ok(BluetoothAction::Remove {
            address: required_string_field(payload, "address")?,
        }),
        "pairing_response" => Ok(BluetoothAction::PairingResponse {
            address: required_string_field(payload, "address")?,
            accept: required_bool_field(payload, "accept")?,
            passkey: optional_passkey_field(payload)?,
            pin: optional_string_field(payload, "pin"),
        }),
        other => Err(DomainError::Unsupported(format!(
            "unknown bluetooth command: {other}"
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

/// Find the properties map for a named interface without constructing an
/// `OwnedInterfaceName` (whose `try_from` would need an `unwrap`).
fn interface_properties<'a>(
    interfaces: &'a HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>>,
    name: &str,
) -> Option<&'a HashMap<String, OwnedValue>> {
    interfaces
        .iter()
        .find(|(key, _)| key.as_str() == name)
        .map(|(_, value)| value)
}

/// The lexicographically smallest adapter object path, if any adapter exists.
/// Smallest-path selection keeps multi-adapter hosts stable across rebuilds
/// (HashMap iteration order is unspecified).
pub(crate) fn first_adapter_path(
    objects: &HashMap<
        OwnedObjectPath,
        HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>>,
    >,
) -> Option<String> {
    objects
        .iter()
        .filter(|(_, interfaces)| interface_properties(interfaces, "org.bluez.Adapter1").is_some())
        .map(|(path, _)| path.as_str().to_string())
        .min()
}

fn bool_property(properties: &HashMap<String, OwnedValue>, name: &str) -> bool {
    properties
        .get(name)
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or(false)
}

fn string_property(properties: &HashMap<String, OwnedValue>, name: &str) -> Option<String> {
    properties
        .get(name)
        .and_then(|value| <&str>::try_from(value).ok())
        .map(str::to_string)
}

/// Map BlueZ managed objects to BluetoothState.
///
/// Pure for testability. Selects the smallest-path adapter, reads its
/// Powered/Discovering state, and collects EVERY Device1 object under it:
/// connected, paired-but-disconnected, and discovered-unpaired.
pub(crate) fn map_managed_objects(
    objects: &HashMap<
        OwnedObjectPath,
        HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>>,
    >,
) -> BluetoothState {
    let Some(adapter_path) = first_adapter_path(objects) else {
        return BluetoothState {
            available: false,
            powered: false,
            discovering: false,
            devices: vec![],
            adapter_path: String::new(),
        };
    };

    let (powered, discovering) = objects
        .iter()
        .find(|(path, _)| path.as_str() == adapter_path)
        .and_then(|(_, interfaces)| interface_properties(interfaces, "org.bluez.Adapter1"))
        .map(|properties| {
            (
                bool_property(properties, "Powered"),
                bool_property(properties, "Discovering"),
            )
        })
        .unwrap_or((false, false));

    let mut devices = Vec::new();
    for (path, interfaces) in objects.iter() {
        if !path.as_str().starts_with(&adapter_path) {
            continue;
        }
        let Some(device_properties) = interface_properties(interfaces, "org.bluez.Device1") else {
            continue;
        };

        let address = string_property(device_properties, "Address").unwrap_or_default();
        let name = string_property(device_properties, "Alias")
            .or_else(|| string_property(device_properties, "Name"))
            .unwrap_or_default();
        let battery_percent =
            interface_properties(interfaces, "org.bluez.Battery1").and_then(|battery_properties| {
                battery_properties
                    .get("Percentage")
                    .and_then(|value| u8::try_from(value).ok())
            });

        devices.push(BluetoothDevice {
            address,
            name,
            battery_percent,
            paired: bool_property(device_properties, "Paired"),
            trusted: bool_property(device_properties, "Trusted"),
            connected: bool_property(device_properties, "Connected"),
            icon: string_property(device_properties, "Icon"),
            rssi: device_properties
                .get("RSSI")
                .and_then(|value| i16::try_from(value).ok()),
        });
    }
    devices.sort_by(|a, b| a.address.cmp(&b.address));

    BluetoothState {
        available: true,
        powered,
        discovering,
        devices,
        adapter_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::time::Duration;

    fn string_value(text: &str) -> OwnedValue {
        let value: zbus::zvariant::Value = text.into();
        OwnedValue::try_from(value).unwrap()
    }

    fn adapter_object(
        powered: bool,
    ) -> HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>> {
        let mut interfaces = HashMap::new();
        let mut properties = HashMap::new();
        properties.insert("Powered".to_string(), OwnedValue::from(powered));
        properties.insert("Discovering".to_string(), OwnedValue::from(false));
        let key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Adapter1").unwrap();
        interfaces.insert(key, properties);
        interfaces
    }

    #[allow(clippy::too_many_arguments)]
    fn device_object(
        address: &str,
        alias: &str,
        paired: bool,
        trusted: bool,
        connected: bool,
        rssi: Option<i16>,
        icon: Option<&str>,
    ) -> HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>> {
        let mut interfaces = HashMap::new();
        let mut properties = HashMap::new();
        properties.insert("Address".to_string(), string_value(address));
        properties.insert("Alias".to_string(), string_value(alias));
        properties.insert("Paired".to_string(), OwnedValue::from(paired));
        properties.insert("Trusted".to_string(), OwnedValue::from(trusted));
        properties.insert("Connected".to_string(), OwnedValue::from(connected));
        if let Some(rssi) = rssi {
            properties.insert("RSSI".to_string(), OwnedValue::from(rssi));
        }
        if let Some(icon) = icon {
            properties.insert("Icon".to_string(), string_value(icon));
        }
        let key = zbus::names::OwnedInterfaceName::try_from("org.bluez.Device1").unwrap();
        interfaces.insert(key, properties);
        interfaces
    }

    #[test]
    fn map_includes_paired_but_disconnected_devices() {
        let mut objects = HashMap::new();
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0").unwrap(),
            adapter_object(true),
        );
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").unwrap(),
            device_object(
                "AA:BB:CC:DD:EE:FF",
                "Keyboard",
                true,
                true,
                false,
                None,
                None,
            ),
        );

        let state = map_managed_objects(&objects);
        assert_eq!(state.devices.len(), 1);
        assert!(state.devices[0].paired);
        assert!(state.devices[0].trusted);
        assert!(!state.devices[0].connected);
    }

    #[test]
    fn map_includes_discovered_unpaired_devices_with_rssi_and_icon() {
        let mut objects = HashMap::new();
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0").unwrap(),
            adapter_object(true),
        );
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0/dev_11_22_33_44_55_66").unwrap(),
            device_object(
                "11:22:33:44:55:66",
                "Earbuds",
                false,
                false,
                false,
                Some(-40),
                Some("audio-headset"),
            ),
        );

        let state = map_managed_objects(&objects);
        assert_eq!(state.devices.len(), 1);
        assert!(!state.devices[0].paired);
        assert_eq!(state.devices[0].rssi, Some(-40));
        assert_eq!(state.devices[0].icon.as_deref(), Some("audio-headset"));
    }

    #[test]
    fn map_reports_adapter_path() {
        let mut objects = HashMap::new();
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0").unwrap(),
            adapter_object(true),
        );
        let state = map_managed_objects(&objects);
        assert_eq!(state.adapter_path, "/org/bluez/hci0");
    }

    #[test]
    fn map_sorts_devices_by_address() {
        let mut objects = HashMap::new();
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0").unwrap(),
            adapter_object(true),
        );
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0/dev_BB_00_00_00_00_00").unwrap(),
            device_object(
                "BB:00:00:00:00:00",
                "Second",
                false,
                false,
                false,
                None,
                None,
            ),
        );
        objects.insert(
            OwnedObjectPath::try_from("/org/bluez/hci0/dev_AA_00_00_00_00_00").unwrap(),
            device_object(
                "AA:00:00:00:00:00",
                "First",
                false,
                false,
                false,
                None,
                None,
            ),
        );

        let state = map_managed_objects(&objects);
        let addresses: Vec<&str> = state.devices.iter().map(|d| d.address.as_str()).collect();
        assert_eq!(addresses, vec!["AA:00:00:00:00:00", "BB:00:00:00:00:00"]);
    }

    #[test]
    fn first_adapter_path_returns_none_without_adapters() {
        let objects = HashMap::new();
        assert_eq!(first_adapter_path(&objects), None);
    }

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
    fn map_marks_disconnected_devices_as_not_connected() {
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
        assert_eq!(state.devices.len(), 1);
        assert!(!state.devices[0].connected);
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

    #[test]
    fn parses_discovery_and_session_commands() {
        assert!(matches!(
            parse_bluetooth_action(&serde_json::json!({"command": "start_discovery"})),
            Ok(BluetoothAction::StartDiscovery)
        ));
        assert!(matches!(
            parse_bluetooth_action(&serde_json::json!({"command": "stop_discovery"})),
            Ok(BluetoothAction::StopDiscovery)
        ));
        assert!(matches!(
            parse_bluetooth_action(&serde_json::json!({"command": "open_session"})),
            Ok(BluetoothAction::OpenSession)
        ));
        assert!(matches!(
            parse_bluetooth_action(&serde_json::json!({"command": "close_session"})),
            Ok(BluetoothAction::CloseSession)
        ));
    }

    #[test]
    fn parses_connect_pair_and_remove() {
        assert!(matches!(
            parse_bluetooth_action(
                &serde_json::json!({"command": "connect", "address": "AA:BB:CC:DD:EE:FF"})
            ),
            Ok(BluetoothAction::Connect { ref address }) if address == "AA:BB:CC:DD:EE:FF"
        ));
        assert!(matches!(
            parse_bluetooth_action(
                &serde_json::json!({"command": "pair", "address": "AA:BB:CC:DD:EE:FF"})
            ),
            Ok(BluetoothAction::Pair { ref address }) if address == "AA:BB:CC:DD:EE:FF"
        ));
        assert!(matches!(
            parse_bluetooth_action(
                &serde_json::json!({"command": "remove", "address": "AA:BB:CC:DD:EE:FF"})
            ),
            Ok(BluetoothAction::Remove { ref address }) if address == "AA:BB:CC:DD:EE:FF"
        ));
    }

    #[test]
    fn parses_set_trusted() {
        match parse_bluetooth_action(&serde_json::json!({
            "command": "set_trusted", "address": "AA:BB:CC:DD:EE:FF", "value": true
        })) {
            Ok(BluetoothAction::SetTrusted { address, value }) => {
                assert_eq!(address, "AA:BB:CC:DD:EE:FF");
                assert!(value);
            }
            other => panic!("expected SetTrusted, got {other:?}"),
        }
    }

    #[test]
    fn parses_pairing_response_variants() {
        match parse_bluetooth_action(&serde_json::json!({
            "command": "pairing_response", "address": "AA:BB:CC:DD:EE:FF", "accept": true
        })) {
            Ok(BluetoothAction::PairingResponse {
                accept: true,
                passkey: None,
                pin: None,
                ..
            }) => {}
            other => panic!("expected bare accept, got {other:?}"),
        }
        match parse_bluetooth_action(&serde_json::json!({
            "command": "pairing_response",
            "address": "AA:BB:CC:DD:EE:FF",
            "accept": true,
            "passkey": 123456
        })) {
            Ok(BluetoothAction::PairingResponse {
                passkey: Some(123456),
                ..
            }) => {}
            other => panic!("expected passkey response, got {other:?}"),
        }
        match parse_bluetooth_action(&serde_json::json!({
            "command": "pairing_response",
            "address": "AA:BB:CC:DD:EE:FF",
            "accept": true,
            "pin": "0000"
        })) {
            Ok(BluetoothAction::PairingResponse { pin: Some(pin), .. }) => {
                assert_eq!(pin, "0000")
            }
            other => panic!("expected pin response, got {other:?}"),
        }
    }

    #[test]
    fn rejects_out_of_range_passkey() {
        assert!(parse_bluetooth_action(&serde_json::json!({
            "command": "pairing_response",
            "address": "AA:BB:CC:DD:EE:FF",
            "accept": true,
            "passkey": 1_000_000
        }))
        .is_err());
    }

    #[test]
    fn rejects_new_commands_missing_address() {
        for command in [
            "connect",
            "pair",
            "remove",
            "set_trusted",
            "pairing_response",
        ] {
            assert!(
                parse_bluetooth_action(&serde_json::json!({"command": command})).is_err(),
                "{command} without address must be rejected"
            );
        }
    }

    #[test]
    fn discovery_errors_that_mean_already_in_desired_state_are_ignorable() {
        assert!(discovery_error_is_ignorable(
            "org.bluez.Error.InProgress: Operation already in progress"
        ));
        assert!(discovery_error_is_ignorable(
            "org.bluez.Error.Failed: No discovery started"
        ));
        assert!(!discovery_error_is_ignorable(
            "org.bluez.Error.NotAuthorized: Operation not permitted"
        ));
    }

    #[test]
    fn properties_changed_rule_targets_the_properties_interface() {
        let rule = bluez_properties_changed_rule(":1.42").expect("rule must build");
        let text = rule.to_string();
        assert!(text.contains("member='PropertiesChanged'"), "{text}");
        assert!(
            text.contains("interface='org.freedesktop.DBus.Properties'"),
            "{text}"
        );
        assert!(text.contains("sender=':1.42'"), "{text}");
    }

    #[tokio::test]
    async fn pairing_response_resolves_a_parked_request() {
        use crate::bluetooth_agent::{PairingReply, PendingPairingMap};
        let provider = BluezProvider {
            id: quantum_domain::ProviderId::from("bluetooth"),
            conn: None,
            pairing: std::sync::Arc::new(PendingPairingMap::new()),
        };
        let receiver = provider
            .pairing
            .park("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF")
            .await;
        let action = quantum_domain::Action::Custom {
            kind: "bluetooth".to_string(),
            payload: serde_json::json!({
                "command": "pairing_response",
                "address": "AA:BB:CC:DD:EE:FF",
                "accept": true
            }),
        };
        provider.invoke(&action).await.expect("must resolve");
        assert!(matches!(receiver.await, Ok(PairingReply::Confirm)));
    }

    #[tokio::test]
    async fn pairing_response_without_a_parked_request_fails() {
        use crate::bluetooth_agent::PendingPairingMap;
        let provider = BluezProvider {
            id: quantum_domain::ProviderId::from("bluetooth"),
            conn: None,
            pairing: std::sync::Arc::new(PendingPairingMap::new()),
        };
        let action = quantum_domain::Action::Custom {
            kind: "bluetooth".to_string(),
            payload: serde_json::json!({
                "command": "pairing_response",
                "address": "AA:BB:CC:DD:EE:FF",
                "accept": true
            }),
        };
        assert!(provider.invoke(&action).await.is_err());
    }

    #[tokio::test]
    async fn pairing_response_reject_and_pin_map_to_the_right_replies() {
        use crate::bluetooth_agent::{PairingReply, PendingPairingMap};
        let provider = BluezProvider {
            id: quantum_domain::ProviderId::from("bluetooth"),
            conn: None,
            pairing: std::sync::Arc::new(PendingPairingMap::new()),
        };

        let receiver = provider
            .pairing
            .park("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF")
            .await;
        let reject = quantum_domain::Action::Custom {
            kind: "bluetooth".to_string(),
            payload: serde_json::json!({
                "command": "pairing_response",
                "address": "AA:BB:CC:DD:EE:FF",
                "accept": false
            }),
        };
        provider.invoke(&reject).await.expect("must resolve");
        assert!(matches!(receiver.await, Ok(PairingReply::Reject)));

        let receiver = provider
            .pairing
            .park("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF")
            .await;
        let pin = quantum_domain::Action::Custom {
            kind: "bluetooth".to_string(),
            payload: serde_json::json!({
                "command": "pairing_response",
                "address": "AA:BB:CC:DD:EE:FF",
                "accept": true,
                "pin": "0000"
            }),
        };
        provider.invoke(&pin).await.expect("must resolve");
        match receiver.await {
            Ok(PairingReply::PinCode(pin)) => assert_eq!(pin, "0000"),
            other => panic!("expected PinCode, got {other:?}"),
        }
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
