/// D-Bus notification provider.
/// Bridges org.freedesktop.Notifications signals into the event bus.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use futures::StreamExt;
use tokio::sync::{broadcast, RwLock};
use async_trait::async_trait;
use zbus::SignalMatch;
use zbus::fdo::MatchRule;

use quantum_domain::error::DomainError;
use quantum_domain::match_result::{IconRef, Match};
use quantum_domain::query::Query;
use quantum_domain::{Action, ActionOutcome, ProviderId, ProviderSource};
use serde_json::{self, Value as JsonValue};

/// A notification from D-Bus.
#[derive(Debug, Clone)]
pub struct DbusNotification {
    pub app_name: String,
    pub icon: String,
    pub id: u32,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub hints: HashMap<String, JsonValue>,
}

/// Manages in-memory notification state with automatic D-Bus bridging.
#[derive(Debug)]
pub struct NotificationsProvider {
    id: ProviderId,
    inner: Arc<NotificationsInner>,
}

struct NotificationsInner {
    store: RwLock<Vec<DbusNotification>>,
    tx: broadcast::Sender<NotificationEvent>,
    next_id: Mutex<u32>,
}

use quantum_domain::NotificationEvent;

impl NotificationsProvider {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel::<NotificationEvent>(64);
        Self {
            id: ProviderId::from("notifications"),
            inner: Arc::new(NotificationsInner {
                store: RwLock::new(Vec::new()),
                tx,
                next_id: Mutex::new(0),
            }),
        }
    }

    /// Get all active notifications as a snapshot.
    pub async fn get_all(&self) -> Vec<DbusNotification> {
        self.inner.store.read().await.clone()
    }

    /// Add an internal notification from a provider.
    pub async fn add_internal_notification(
        &self,
        app_name: String,
        summary: String,
        body: String,
        icon: Option<String>,
        timeout_ms: u64,
    ) {
        let mut store = self.inner.store.write().await;
        if let Some(pos) = store.iter().position(|n| n.app_name == app_name && n.summary == summary && n.id == 0) {
            store[pos].body = body;
            let _ = self.inner.tx.send(NotificationEvent::Updated { id: 0 });
        } else {
            let mut next = self.inner.next_id.lock().unwrap();
            *next += 1;
            store.push(DbusNotification {
                app_name, icon: icon.unwrap_or_default(), id: 0, summary, body,
                actions: Vec::new(), hints: HashMap::new(),
            });
        }
        let _ = self.inner.tx.send(NotificationEvent::Created {
            id: 0, timeout_ms: Some(timeout_ms),
        });
    }

    /// Dismiss a notification by ID.
    pub async fn dismiss(&self, id: u32) -> Result<(), DomainError> {
        let mut store = self.inner.store.write().await;
        store.retain(|n| n.id != id);
        let _ = self.inner.tx.send(NotificationEvent::Dismissed { id });
        Ok(())
    }

    /// Get the number of active notifications.
    pub async fn count(&self) -> usize {
        self.inner.store.read().await.len()
    }

    /// Start listening to D-Bus signals in a background tokio task.
    pub async fn start_dbus(&self) {
        let inner = self.inner.clone();
        tokio::task::spawn(async move {
            if let Ok(conn) = zbus::Connection::session().await {
                let match_rule = SignalMatch::match_signal(
                    Some("org.freedesktop.Notifications"),
                    None,
                    Some("Notify"),
                );
                conn.match_signal(match_rule).on_await(async move |signal: zbus::Message<'_>| {
                    // Use signal.iterator() to read parameters.
                    let mut it = signal.iter();

                    // Extract app_name.
                    let app_name = if let Ok(val) = it.next().map(|m| m.deserialize()) {
                        match val {
                            Ok((v, _)) => String::from(&*v),
                            Err(_) => "Unknown".to_string(),
                        }
                    } else {
                        "Unknown".to_string()
                    };

                    // Extract icon path/name.
                    let icon = if let Ok(val) = it.next().map(|m| m.deserialize()) {
                        match val {
                            Ok((v, _)) => String::from(&*v),
                            Err(_) => "application-default-icon".to_string(),
                        }
                    } else {
                        "application-default-icon".to_string()
                    };

                    // Extract id.
                    let db_id: u32 = if let Ok(val) = it.next().map(|m| m.deserialize()) {
                        match val {
                            Ok((v, _)) => <u32>::from(&*v),
                            Err(_) => 0,
                        }
                    } else {
                        0
                    };

                    // Extract summary.
                    let summary = if let Ok(val) = it.next().map(|m| m.deserialize()) {
                        match val {
                            Ok((v, _)) => String::from(&*v),
                            Err(_) => "".to_string(),
                        }
                    } else {
                        "".to_string()
                    };

                    // Extract body.
                    let body = if let Ok(val) = it.next().map(|m| m.deserialize()) {
                        match val {
                            Ok((v, _)) => String::from(&*v),
                            Err(_) => "".to_string(),
                        }
                    } else {
                        "".to_string()
                    };

                    // Extract remaining items as actions.
                    let mut actions: Vec<(String, String)> = Vec::new();
                    for item in it.by_ref() {
                        if let Ok(val) = item.deserialize::<zbus::zvariant::Value>() {
                            if let Ok(array) = val.downcast_ref::<Vec<(&str, zbus::zvariant::Value)>>() {
                                for (k, v) in array {
                                    actions.push((k.to_string(), String::from(&**v)));
                                }
                            }
                        }
                    }

                    // Build hints from signature.
                    let mut hints: HashMap<String, JsonValue> = HashMap::new();
                    let sig = signal.signature().to_string();
                    if sig.contains("{sv}") {
                        // Parse dict of string-to-value.
                        for item in it.by_ref() {
                            if let Ok(val) = item.deserialize::<zbus::zvariant::Dict<&str, zbus::zvariant::Value>>() {
                                for (k, v) in val {
                                    hints.insert(k.to_string(), serde_json::to_value(v).unwrap_or(JsonValue::Null));
                                }
                            }
                        }
                    }

                    let notification = DbusNotification {
                        app_name, icon, id: db_id, summary, body, actions, hints,
                    };

                    // Insert or update.
                    let mut sg = inner.store.write().await;
                    if db_id != 0 {
                        if let Some(pos) = sg.iter().position(|n| n.id == db_id) {
                            sg[pos] = notification;
                            let _ = inner.tx.send(NotificationEvent::Updated { id: db_id });
                            return;
                        }
                    }
                    let mut next_id = inner.next_id.lock().unwrap();
                    *next_id += 1;
                    let new_notification = DbusNotification {
                        app_name, icon, id: *next_id, summary, body, actions, hints,
                    };
                    sg.push(new_notification);
                    let _ = inner.tx.send(NotificationEvent::Created {
                        id: *next_id, timeout_ms: None,
                    });
                }).await.map_err(|e| tracing::warn!("D-Bus match error: {}", e));
            } else {
                tracing::info!("NotificationsProvider could not connect to session bus");
            }
        });
    }
}

#[async_trait]
impl ProviderSource for NotificationsProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, _q: &Query) -> Result<Vec<Match>, DomainError> {
        let store = self.inner.store.read().await;
        Ok(store.iter()
            .filter(|n| !n.summary.is_empty())
            .map(|n| Match {
                id: format!("notification-{}", n.id),
                title: n.summary.clone(),
                subtitle: Some(n.body.clone()),
                icon: if n.icon.is_empty() { None } else { Some(IconRef::Named(n.icon.clone())) },
                score: 0.9,
                action: Action::Custom {
                    kind: "notifications.dismiss".to_string(),
                    payload: serde_json::json!({ "id": n.id, "app_name": &n.app_name }),
                },
            }).collect())
    }

    async fn invoke(&self, _action: &Action) -> Result<ActionOutcome, DomainError> {
        Ok(ActionOutcome { message: None })
    }

    fn subscribe(&self) -> Option<futures::stream::BoxStream<'static, serde_json::Value>> {
        let mut rx = self.inner.tx.subscribe();
        Some(futures::stream::unfold(rx, |mut rx| async move {
            match rx.recv().await {
                Ok(event) => { let json = serde_json::to_value(&event).ok()?; Some((json, rx)) }
                Err(_) => None,
            }
        }).boxed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn creates_provider_and_gets_all() {
        let provider = NotificationsProvider::new();
        assert_eq!(provider.id(), &ProviderId::from("notifications"));
        let store = provider.get_all().await;
        assert!(store.is_empty());
    }

    #[tokio::test]
    async fn adds_and_counts_notification() {
        let provider = NotificationsProvider::new();
        provider.add_internal_notification("Spotify".into(), "Now playing".into(), "Song title".into(), Some("spotify".into()), 5000).await;
        assert_eq!(provider.count().await, 1);
        let all = provider.get_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].app_name, "Spotify");
    }

    #[tokio::test]
    async fn dismisses_notification() {
        let provider = NotificationsProvider::new();
        provider.add_internal_notification("App".into(), "Title".into(), "Body".into(), None, 5000).await;
        assert_eq!(provider.count().await, 1);
        provider.dismiss(0).await.unwrap();
        assert_eq!(provider.count().await, 0);
    }

    #[test]
    fn subscribes_returns_stream() {
        let provider = NotificationsProvider::new();
        assert!(provider.subscribe().is_some());
    }
}
