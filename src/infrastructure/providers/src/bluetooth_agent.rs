//! BlueZ pairing agent support: the pending-reply map shared between the
//! zbus-served `org.bluez.Agent1` object and the bluetooth provider's
//! `pairing_response` action.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use quantum_domain::EventBus;
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Mutex};

/// A reply the window sends to a parked agent callback.
#[derive(Debug, PartialEq, Eq)]
pub enum PairingReply {
    Confirm,
    Reject,
    Passkey(u32),
    PinCode(String),
}

/// The trailing object-path segment BlueZ derives from a device address:
/// `AA:BB:CC:DD:EE:FF` becomes `/dev_AA_BB_CC_DD_EE_FF`.
pub(crate) fn address_suffix(address: &str) -> String {
    format!("/dev_{}", address.to_uppercase().replace(':', "_"))
}

/// Pending pairing replies, keyed by BlueZ device object path.
///
/// Agent callbacks park a oneshot sender here and await the receiver (with a
/// timeout); the provider's `pairing_response` action resolves it by device
/// address. `Default` gives an empty map.
#[derive(Default)]
pub struct PendingPairingMap {
    inner: Mutex<HashMap<String, oneshot::Sender<PairingReply>>>,
}

impl PendingPairingMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Park a new pending reply for `device_path`, returning the receiver the
    /// agent callback awaits. Replaces (and thereby cancels) any existing
    /// entry for the same device.
    pub async fn park(&self, device_path: &str) -> oneshot::Receiver<PairingReply> {
        let (sender, receiver) = oneshot::channel();
        let mut inner = self.inner.lock().await;
        inner.insert(device_path.to_string(), sender);
        receiver
    }

    /// Resolve the pending reply for the device with `address` (any adapter).
    /// Returns false when nothing is parked for that address.
    pub async fn resolve_by_address(&self, address: &str, reply: PairingReply) -> bool {
        let suffix = address_suffix(address);
        let mut inner = self.inner.lock().await;
        let key = inner.keys().find(|path| path.ends_with(&suffix)).cloned();
        match key {
            Some(key) => match inner.remove(&key) {
                Some(sender) => sender.send(reply).is_ok(),
                None => false,
            },
            None => false,
        }
    }

    /// Drop the pending reply for `device_path`, failing the parked receiver.
    pub async fn cancel(&self, device_path: &str) -> bool {
        let mut inner = self.inner.lock().await;
        inner.remove(device_path).is_some()
    }

    /// Drop every pending reply, returning the device paths that were parked.
    pub async fn cancel_all(&self) -> Vec<String> {
        let mut inner = self.inner.lock().await;
        inner.drain().map(|(device_path, _)| device_path).collect()
    }
}

/// Shared handle type used by the provider and the agent.
pub type SharedPendingPairingMap = Arc<PendingPairingMap>;

// NOTE (deviation from the plan): the plan's Task 6 places
// `BluetoothPairingRequestKind` and `BluetoothEvent` in `quantum-domain`
// (`src/domain/src/bar_state.rs`) and this agent imports them from there. This
// worktree is not permitted to edit the domain crate, so the two pairing-event
// DTOs are defined here in `quantum-providers` instead. Their serde attributes
// are identical to the plan's domain versions, so the JSON published on
// `bluetooth.event` is byte-for-byte what the frontend contract expects. If the
// domain track later lands Task 6, delete these two definitions and switch the
// import below to `quantum_domain::{BluetoothEvent, BluetoothPairingRequestKind}`.

/// The kind of interaction a BlueZ pairing agent callback needs from the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BluetoothPairingRequestKind {
    /// RequestConfirmation: show a passkey, user confirms or rejects.
    Confirm,
    /// RequestPasskey: user types the six-digit passkey shown on the device.
    RequestPasskey,
    /// RequestPinCode: user types a legacy PIN.
    RequestPin,
    /// DisplayPasskey: show a passkey the user types ON the remote device.
    DisplayPasskey,
    /// AuthorizeService: allow or deny a service connection.
    AuthorizeService,
}

/// Out-of-band pairing events the agent publishes on `bluetooth.event`.
///
/// Internally tagged with `event` so subscribers can distinguish these from
/// `BluetoothState` payloads, which never carry an `event` key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum BluetoothEvent {
    PairingRequest {
        request: BluetoothPairingRequestKind,
        address: String,
        device_path: String,
        #[serde(default)]
        passkey: Option<u32>,
        #[serde(default)]
        service_uuid: Option<String>,
    },
    PairingCancelled {
        address: String,
    },
}

/// Object path the agent is served at on the system bus.
pub const AGENT_OBJECT_PATH: &str = "/org/quantum/bluetooth_agent";

/// Channel the agent publishes pairing events on. Must match the provider id
/// convention `{provider_id}.event`.
pub const BLUETOOTH_EVENT_CHANNEL: &str = "bluetooth.event";

/// How long the agent waits for the window's `pairing_response` before
/// rejecting, so BlueZ never hangs on an unattended prompt.
const PAIRING_REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// Recover a device address from a BlueZ device object path
/// (`.../dev_AA_BB_CC_DD_EE_FF` becomes `AA:BB:CC:DD:EE:FF`).
pub(crate) fn address_from_device_path(device_path: &str) -> String {
    device_path
        .rsplit('/')
        .next()
        .and_then(|segment| segment.strip_prefix("dev_"))
        .map(|segment| segment.replace('_', ":"))
        .unwrap_or_default()
}

/// DBus errors BlueZ understands from an agent.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
pub enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

/// The zbus-served `org.bluez.Agent1` object. Callbacks publish pairing
/// requests on `bluetooth.event` and park the pending DBus reply in the
/// shared map; the window's `pairing_response` action resolves it.
pub struct BluezPairingAgent {
    pending: SharedPendingPairingMap,
    event_bus: Arc<dyn EventBus>,
}

impl BluezPairingAgent {
    pub fn new(pending: SharedPendingPairingMap, event_bus: Arc<dyn EventBus>) -> Self {
        Self { pending, event_bus }
    }

    async fn publish(&self, event: &BluetoothEvent) {
        match serde_json::to_string(event) {
            Ok(payload) => {
                if let Err(error) = self
                    .event_bus
                    .publish(BLUETOOTH_EVENT_CHANNEL, &payload)
                    .await
                {
                    tracing::warn!(error = %error, "bluetooth pairing event publish failed");
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "bluetooth pairing event serialization failed");
            }
        }
    }

    async fn publish_cancelled(&self, device_path: &str) {
        self.publish(&BluetoothEvent::PairingCancelled {
            address: address_from_device_path(device_path),
        })
        .await;
    }

    /// Publish a pairing request, park the reply, and await it under the
    /// timeout. Timeout or a dropped sender cancels the pairing.
    async fn request_and_wait(
        &self,
        device_path: &str,
        request: BluetoothPairingRequestKind,
        passkey: Option<u32>,
        service_uuid: Option<String>,
    ) -> Result<PairingReply, AgentError> {
        self.publish(&BluetoothEvent::PairingRequest {
            request,
            address: address_from_device_path(device_path),
            device_path: device_path.to_string(),
            passkey,
            service_uuid,
        })
        .await;
        let receiver = self.pending.park(device_path).await;
        match tokio::time::timeout(PAIRING_REPLY_TIMEOUT, receiver).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_dropped)) => {
                self.publish_cancelled(device_path).await;
                Err(AgentError::Canceled("pairing request replaced".to_string()))
            }
            Err(_elapsed) => {
                self.pending.cancel(device_path).await;
                self.publish_cancelled(device_path).await;
                Err(AgentError::Canceled(
                    "pairing request timed out".to_string(),
                ))
            }
        }
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl BluezPairingAgent {
    async fn release(&self) {
        let cancelled = self.pending.cancel_all().await;
        for device_path in cancelled {
            self.publish_cancelled(&device_path).await;
        }
    }

    async fn request_pin_code(
        &self,
        device: zbus::zvariant::OwnedObjectPath,
    ) -> Result<String, AgentError> {
        match self
            .request_and_wait(
                device.as_str(),
                BluetoothPairingRequestKind::RequestPin,
                None,
                None,
            )
            .await?
        {
            PairingReply::PinCode(pin) => Ok(pin),
            _ => Err(AgentError::Rejected("pin entry rejected".to_string())),
        }
    }

    async fn request_passkey(
        &self,
        device: zbus::zvariant::OwnedObjectPath,
    ) -> Result<u32, AgentError> {
        match self
            .request_and_wait(
                device.as_str(),
                BluetoothPairingRequestKind::RequestPasskey,
                None,
                None,
            )
            .await?
        {
            PairingReply::Passkey(passkey) => Ok(passkey),
            _ => Err(AgentError::Rejected("passkey entry rejected".to_string())),
        }
    }

    async fn display_passkey(
        &self,
        device: zbus::zvariant::OwnedObjectPath,
        passkey: u32,
        _entered: u16,
    ) {
        // Display-only: no reply is parked; the dialog closes on
        // pairing_cancelled or when the device list refreshes as paired.
        self.publish(&BluetoothEvent::PairingRequest {
            request: BluetoothPairingRequestKind::DisplayPasskey,
            address: address_from_device_path(device.as_str()),
            device_path: device.as_str().to_string(),
            passkey: Some(passkey),
            service_uuid: None,
        })
        .await;
    }

    async fn request_confirmation(
        &self,
        device: zbus::zvariant::OwnedObjectPath,
        passkey: u32,
    ) -> Result<(), AgentError> {
        match self
            .request_and_wait(
                device.as_str(),
                BluetoothPairingRequestKind::Confirm,
                Some(passkey),
                None,
            )
            .await?
        {
            PairingReply::Confirm => Ok(()),
            _ => Err(AgentError::Rejected("pairing rejected".to_string())),
        }
    }

    async fn request_authorization(
        &self,
        device: zbus::zvariant::OwnedObjectPath,
    ) -> Result<(), AgentError> {
        match self
            .request_and_wait(
                device.as_str(),
                BluetoothPairingRequestKind::Confirm,
                None,
                None,
            )
            .await?
        {
            PairingReply::Confirm => Ok(()),
            _ => Err(AgentError::Rejected("authorization rejected".to_string())),
        }
    }

    async fn authorize_service(
        &self,
        device: zbus::zvariant::OwnedObjectPath,
        uuid: String,
    ) -> Result<(), AgentError> {
        match self
            .request_and_wait(
                device.as_str(),
                BluetoothPairingRequestKind::AuthorizeService,
                None,
                Some(uuid),
            )
            .await?
        {
            PairingReply::Confirm => Ok(()),
            _ => Err(AgentError::Rejected(
                "service authorization rejected".to_string(),
            )),
        }
    }

    async fn cancel(&self) {
        let cancelled = self.pending.cancel_all().await;
        for device_path in cancelled {
            self.publish_cancelled(&device_path).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_suffix_uppercases_and_replaces_colons() {
        assert_eq!(
            address_suffix("aa:bb:cc:dd:ee:ff"),
            "/dev_AA_BB_CC_DD_EE_FF"
        );
    }

    #[test]
    fn address_from_device_path_recovers_the_address() {
        assert_eq!(
            address_from_device_path("/org/bluez/hci1/dev_AA_BB_CC_DD_EE_FF"),
            "AA:BB:CC:DD:EE:FF"
        );
        assert_eq!(address_from_device_path("/not/a/device/path"), "");
    }

    #[tokio::test]
    async fn park_then_resolve_by_address_delivers_the_reply() {
        let map = PendingPairingMap::new();
        let receiver = map.park("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").await;
        let resolved = map
            .resolve_by_address("AA:BB:CC:DD:EE:FF", PairingReply::Confirm)
            .await;
        assert!(resolved);
        assert!(matches!(receiver.await, Ok(PairingReply::Confirm)));
    }

    #[tokio::test]
    async fn resolve_by_address_with_nothing_parked_returns_false() {
        let map = PendingPairingMap::new();
        assert!(
            !map.resolve_by_address("AA:BB:CC:DD:EE:FF", PairingReply::Confirm)
                .await
        );
    }

    #[tokio::test]
    async fn cancel_drops_the_sender_so_the_receiver_errors() {
        let map = PendingPairingMap::new();
        let receiver = map.park("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").await;
        assert!(map.cancel("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").await);
        assert!(receiver.await.is_err());
    }

    #[tokio::test]
    async fn cancel_all_returns_every_parked_device_path() {
        let map = PendingPairingMap::new();
        let _first = map.park("/org/bluez/hci0/dev_AA_00_00_00_00_00").await;
        let _second = map.park("/org/bluez/hci0/dev_BB_00_00_00_00_00").await;
        let mut cancelled = map.cancel_all().await;
        cancelled.sort();
        assert_eq!(
            cancelled,
            vec![
                "/org/bluez/hci0/dev_AA_00_00_00_00_00".to_string(),
                "/org/bluez/hci0/dev_BB_00_00_00_00_00".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn parking_twice_replaces_the_first_receiver() {
        let map = PendingPairingMap::new();
        let first = map.park("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").await;
        let second = map.park("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").await;
        assert!(first.await.is_err(), "replaced sender must be dropped");
        assert!(
            map.resolve_by_address("AA:BB:CC:DD:EE:FF", PairingReply::Reject)
                .await
        );
        assert!(matches!(second.await, Ok(PairingReply::Reject)));
    }
}
