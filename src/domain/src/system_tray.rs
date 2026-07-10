use crate::IconRef;
use serde::{Deserialize, Serialize};

/// Snapshot of every registered StatusNotifierItem, published on
/// `system_tray.event`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SystemTrayState {
    pub items: Vec<SystemTrayItem>,
}

/// One tray item: identity, presentation, and its mirrored menu tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemTrayItem {
    /// Stable key: `<bus name><object path>`, for example
    /// `:1.42/StatusNotifierItem`. Also the handle actions send back.
    pub service: String,
    pub title: String,
    pub tooltip: String,
    /// `Active`, `Passive`, or `NeedsAttention` per the protocol.
    pub status: String,
    pub icon: Option<IconRef>,
    /// When true, left-click opens the menu instead of activating.
    pub item_is_menu: bool,
    pub menu: Vec<SystemTrayMenuNode>,
}

/// One node of a com.canonical.dbusmenu layout tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemTrayMenuNode {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub separator: bool,
    /// `checkmark` or `radio` when the item is a toggle.
    pub toggle_type: Option<String>,
    /// `Some(true)` checked, `Some(false)` unchecked, `None` not a toggle
    /// or indeterminate.
    pub toggle_state: Option<bool>,
    pub icon_name: Option<String>,
    pub children: Vec<SystemTrayMenuNode>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_through_json() {
        let state = SystemTrayState {
            items: vec![SystemTrayItem {
                service: ":1.42/StatusNotifierItem".to_string(),
                title: "Steam".to_string(),
                tooltip: "Steam".to_string(),
                status: "Active".to_string(),
                icon: Some(crate::IconRef::Name("steam".to_string())),
                item_is_menu: false,
                menu: vec![SystemTrayMenuNode {
                    id: 1,
                    label: "Exit".to_string(),
                    enabled: true,
                    visible: true,
                    separator: false,
                    toggle_type: None,
                    toggle_state: None,
                    icon_name: None,
                    children: vec![],
                }],
            }],
        };
        let json = serde_json::to_string(&state).expect("serialize");
        let back: SystemTrayState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, state);
    }

    #[test]
    fn default_state_is_empty_items() {
        let json = serde_json::to_value(SystemTrayState::default()).expect("serialize");
        assert_eq!(json, serde_json::json!({ "items": [] }));
    }
}
