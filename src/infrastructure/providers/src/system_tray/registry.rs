//! Pure bookkeeping for registered StatusNotifierItems.
//!
//! An application registers a tray item by calling
//! `RegisterStatusNotifierItem(service)` on the watcher. The `service`
//! argument is either a bus name (in which case the object path defaults to
//! `/StatusNotifierItem`) or an object path starting with `/` (in which case
//! the bus name is the DBus message sender). A single bus connection can host
//! multiple items under one bus name; when that bus name loses its owner every
//! item beneath it must be removed.
//!
//! This module holds no bus, async, or IO state. It is a data structure the
//! host layer drives.

use std::collections::BTreeMap;

/// Default object path used when the register argument is a bus name (or empty)
/// rather than an explicit object path.
const DEFAULT_OBJECT_PATH: &str = "/StatusNotifierItem";

/// Resolve a `RegisterStatusNotifierItem` argument to `(bus_name, object_path)`.
///
/// - An argument starting with `/` is an object path; the bus name is the
///   message sender.
/// - An empty argument defaults the object path to `/StatusNotifierItem`; the
///   bus name is the message sender.
/// - Otherwise the argument is a bus name and the object path defaults to
///   `/StatusNotifierItem`.
///
/// Returns `Some` for all three cases today. The `Option` is retained so the
/// host layer can reject a future invalid case without a signature change.
pub fn parse_service(argument: &str, sender: &str) -> Option<(String, String)> {
    if argument.starts_with('/') {
        return Some((sender.to_string(), argument.to_string()));
    }
    if argument.is_empty() {
        return Some((sender.to_string(), DEFAULT_OBJECT_PATH.to_string()));
    }
    Some((argument.to_string(), DEFAULT_OBJECT_PATH.to_string()))
}

/// Tracks which StatusNotifierItems are registered, keyed by service key.
///
/// The service key is `format!("{bus_name}{object_path}")`. Backing the map
/// with a [`BTreeMap`] keeps [`ItemRegistry::service_keys`] sorted and stable
/// and makes [`ItemRegistry::remove_by_bus_name`] a simple filter.
#[derive(Debug, Default)]
pub struct ItemRegistry {
    items: BTreeMap<String, (String, String)>,
}

impl ItemRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert an item. Returns `false` if this `(bus_name, object_path)` pair
    /// was already present.
    pub fn insert(&mut self, bus_name: &str, object_path: &str) -> bool {
        let service_key = format!("{bus_name}{object_path}");
        if self.items.contains_key(&service_key) {
            return false;
        }
        self.items
            .insert(service_key, (bus_name.to_string(), object_path.to_string()));
        true
    }

    /// Remove every item whose bus name matches. Returns the removed service
    /// keys in sorted order.
    pub fn remove_by_bus_name(&mut self, bus_name: &str) -> Vec<String> {
        let removed: Vec<String> = self
            .items
            .iter()
            .filter(|(_, (item_bus_name, _))| item_bus_name == bus_name)
            .map(|(service_key, _)| service_key.clone())
            .collect();
        for service_key in &removed {
            self.items.remove(service_key);
        }
        removed
    }

    /// All service keys, sorted and stable.
    pub fn service_keys(&self) -> Vec<String> {
        self.items.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_service_handles_object_path_bus_name_and_empty() {
        assert_eq!(
            parse_service("/StatusNotifierItem", ":1.5"),
            Some((":1.5".into(), "/StatusNotifierItem".into()))
        );
        assert_eq!(
            parse_service("", ":1.5"),
            Some((":1.5".into(), "/StatusNotifierItem".into()))
        );
        assert_eq!(
            parse_service("org.kde.StatusNotifierItem-1-1", ":1.5"),
            Some((
                "org.kde.StatusNotifierItem-1-1".into(),
                "/StatusNotifierItem".into()
            ))
        );
    }

    #[test]
    fn insert_dedupes() {
        let mut registry = ItemRegistry::new();
        assert!(registry.insert(":1.42", "/StatusNotifierItem"));
        assert!(!registry.insert(":1.42", "/StatusNotifierItem"));
    }

    #[test]
    fn remove_by_bus_name_removes_all_items_under_the_bus() {
        let mut registry = ItemRegistry::new();
        registry.insert(":1.42", "/StatusNotifierItem");
        registry.insert(":1.42", "/OtherItem");
        registry.insert(":1.99", "/StatusNotifierItem");
        let mut removed = registry.remove_by_bus_name(":1.42");
        removed.sort();
        assert_eq!(
            removed,
            vec![
                ":1.42/OtherItem".to_string(),
                ":1.42/StatusNotifierItem".to_string()
            ]
        );
        assert_eq!(
            registry.service_keys(),
            vec![":1.99/StatusNotifierItem".to_string()]
        );
    }

    #[test]
    fn remove_absent_bus_name_is_noop() {
        let mut registry = ItemRegistry::new();
        registry.insert(":1.42", "/StatusNotifierItem");
        assert!(registry.remove_by_bus_name(":1.99").is_empty());
        assert_eq!(registry.service_keys().len(), 1);
    }

    #[test]
    fn service_keys_are_sorted() {
        let mut registry = ItemRegistry::new();
        registry.insert(":1.99", "/StatusNotifierItem");
        registry.insert(":1.42", "/StatusNotifierItem");
        assert_eq!(
            registry.service_keys(),
            vec![
                ":1.42/StatusNotifierItem".to_string(),
                ":1.99/StatusNotifierItem".to_string()
            ]
        );
    }
}
