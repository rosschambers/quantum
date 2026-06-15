/// D-Bus notification provider.
/// Bridges org.freedesktop.Notifications signals into the event bus.

use std::sync::{Arc, Mutex};
use futures::StreamExt;
use tokio::sync::{broadcast, RwLock};
use async_trait::async_trait;

use quantum_domain::error::DomainError;
use quantum_domain::match_result::{IconRef, Match};
use quantum_domain::query::Query;
use quantum_domain::{Action, ActionOutcome, ProviderId, ProviderSource};
use serde_json::{self, Value as JsonValue};

/// A notification from D-Bus.
#[derive(Debug, Clone)]
pub struct DbusNotification {
    pub app_name: String,
    /// Icon can be an absolute path or a theme icon name. Empty means no icon.
    pub icon: String,
    /// The ID returned by the notification server. 0 means "create new".
    pub id: u32,
    pub summary: String,
    pub body: String,
    /// List of (action_name, action_value) pairs.
    pub actions: Vec<(String, String)>,
    pub hints: std::collections::HashMap<String, JsonValue>,
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
                app_name, icon: icon.unwrap_or_default(), id: 0, summary, body, actions: Vec::new(), hints: std::collections::HashMap::new(),
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
                conn.match_signal(|msg| {
                    msg.interface().is_some_and(|iface| iface == "org.freedesktop.Notifications")
                        && msg.member().is_some_and(|m| m == "Notify")
                }).on_await(async move |signal: zbus::Message| {
                    let body = signal.body();
                    let app_name: String = <&str>::try_from(body.read::<zbus::zvariant::Value>().ok()).unwrap_or_default().to_string();
                    let db_id: u32 = <u32>::try_from(body.read::<zbus::zvariant::Value>().ok()).unwrap_or(0);
                    let icon: String = <&str>::try_from(body.read::<zbus::zvariant::Value>().ok()).map(|s| s.to_string()).unwrap_or_default();
                    let summary: String = <&str>::try_from(body.read::<zbus::zvariant::Value>().ok()).map(|s| s.to_string()).unwrap_or_default();
                    let body_str: String = <&str>::try_from(body.read::<zbus::zvariant::Value>().ok()).map(|s| s.to_string()).unwrap_or_default();

                    let notification = DbusNotification { app_name, icon, id: db_id, summary, body: body_str, actions: Vec::new(), hints: std::collections::HashMap::new() };
                    let mut store_guard = inner.store.write().await;
                    if db_id != 0 {
                        if let Some(pos) = store_guard.iter().position(|n| n.id == db_id) {
                            store_guard[pos] = notification;
                            let _ = inner.tx.send(NotificationEvent::Updated { id: db_id });
                            return;
                        }
                    }
                    let mut next_id = inner.next_id.lock().unwrap();
                    *next_id += 1;
                    store_guard.push(notification);
                    let _ = inner.tx.send(NotificationEvent::Created { id: *next_id, timeout_ms: None });
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
                    payload: serde_json::json!({ "id": n.id, "app_name": &n.app_name, "summary": &n.summary }),
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
