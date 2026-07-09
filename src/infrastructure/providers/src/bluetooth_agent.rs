//! BlueZ pairing agent support: the pending-reply map shared between the
//! zbus-served `org.bluez.Agent1` object and the bluetooth provider's
//! `pairing_response` action.

use std::collections::HashMap;
use std::sync::Arc;

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
