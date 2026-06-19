//! Notification types for the domain layer.
//! No imports from other workspace crates.

use serde::{Deserialize, Serialize};

/// A single notification entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub icon: Option<String>,
    pub urgency: NotificationUrgency,
    pub timeout_ms: u64,
    pub actions: Vec<(String, String)>,
}

impl Notification {
    /// Create a new notification with default timeout (0 = no auto-dismiss).
    pub fn new(id: u32, app_name: &str, summary: &str, body: &str, icon: Option<&str>) -> Self {
        Self {
            id,
            app_name: app_name.to_string(),
            summary: summary.to_string(),
            body: body.to_string(),
            icon: icon.map(|s| s.to_string()),
            urgency: NotificationUrgency::Normal,
            timeout_ms: 0,
            actions: Vec::new(),
        }
    }

    /// Create from a D-Bus Notify signal. D-Bus id=0 means "create new", >0 means update existing.
    pub fn from_dbus(app_name: &str, icon: &str, id: u32, summary: &str, body: &str) -> Self {
        Self::new(
            id,
            app_name,
            summary,
            body,
            if icon.is_empty() { None } else { Some(icon) },
        )
    }
}

/// Urgency levels for notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NotificationUrgency {
    Low,
    #[default]
    Normal,
    Critical,
}

/// Events emitted by the notification system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum NotificationEvent {
    /// A new notification arrived (from D-Bus or internal provider).
    Created { id: u32, timeout_ms: Option<u64> },
    /// A notification was dismissed by user or timed out.
    Dismissed { id: u32 },
    /// An existing notification was updated in place.
    Updated { id: u32 },
    /// The transient on-screen toasts should be cleared (for example because the
    /// user opened the notification center). The stored notifications are
    /// unchanged; only the toast popups are dismissed.
    ToastsCleared,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_notification_with_defaults() {
        let n = Notification::new(0, "App", "Title", "Body", Some("icon"));
        assert_eq!(n.id, 0);
        assert_eq!(n.app_name, "App");
        assert_eq!(n.summary, "Title");
        assert_eq!(n.body, "Body");
        assert_eq!(n.timeout_ms, 0);
        assert_eq!(n.urgency, NotificationUrgency::Normal);
    }

    #[test]
    fn serde_round_trips_notification() {
        let n = Notification::new(1, "Spotify", "Now playing", "Song", Some("spotify"));
        let json = serde_json::to_value(&n).expect("serde");
        let back: Notification = serde_json::from_value(json).expect("round trip");
        assert_eq!(back.app_name, "Spotify");
        assert_eq!(back.summary, "Now playing");
    }

    #[test]
    fn notification_urgency_serializes_lowercase() {
        let v = serde_json::to_string(&NotificationUrgency::Critical).expect("serde");
        assert_eq!(v, "\"critical\"");
    }

    #[test]
    fn event_kind_field() {
        let e = NotificationEvent::Created {
            id: 42,
            timeout_ms: Some(5000),
        };
        let json = serde_json::to_value(&e).expect("serde");
        assert_eq!(json["type"], "created");
        assert_eq!(json["data"]["id"], 42);
    }

    #[test]
    fn toasts_cleared_serializes_to_tag_only() {
        let json = serde_json::to_value(NotificationEvent::ToastsCleared).expect("serde");
        assert_eq!(json["type"], "toasts_cleared");
        assert!(json.get("data").is_none());
    }
}
