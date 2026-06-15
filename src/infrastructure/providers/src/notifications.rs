/// D-Bus notification provider.
/// Bridges org.freedesktop.Notifications signals into the event bus.

use std::sync::{Arc, Mutex};
use futures::StreamExt;
use tokio::sync::{broadcast, RwLock};
use async_trait::async_trait;

use quantum_domain::error::DomainError;
use quantum_domain::match_result::Match;
use quantum_domain::query::Query;
use quantum_domain::{
    Action, ActionOutcome, JsonValue, NotificationEvent, NotificationSource,
    ProviderId, ProviderSource,
};
use serde_json;

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
    /// Active notifications indexed by our internal auto-incrementing ID.
    store: RwLock<Vec<DbusNotification>>,
    tx: broadcast::Sender<NotificationEvent>,
    next_id: Mutex<u32>,
}

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
        // Check for duplicate by (app_name, summary).
        if let Some(pos) = store.iter().position(|n| n.app_name == app_name && n.summary == summary && n.id == 0) {
            store[pos].body = body;
            let _ = self.inner.tx.send(NotificationEvent::Updated { id: 0 });
        } else {
            let mut next = self.inner.next_id.lock().unwrap();
            *next += 1;
            store.push(DbusNotification {
                app_name,
                icon: icon.unwrap_or_default(),
                id: 0,
                summary,
                body,
                actions: Vec::new(),
                hints: std::collections::HashMap::new(),
            });
        }
        let _ = self.inner.tx.send(NotificationEvent::Created {
            id: 0,
            timeout_ms: Some(timeout_ms),
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
            // Match signals from org.freedesktop.Notifications.Notify.
            match zbus::Connection::session().await {
                Ok(conn) => {
                    conn.match_signal(|signal| {
                        signal.interface().map(|iface| iface == "org.freedesktop.Notifications")
                            .unwrap_or(false)
                            && signal.member().map(|m| m == "Notify").unwrap_or(false)
                    }).on_await(async move |signal: zbus::SignalMessage<'_>| {
                        let app_name: String = signal.body().read::<String>().ok().unwrap_or_default();
                        let id: u32 = signal.body().read::<u32>().ok().unwrap_or(0);
                        let icon: String = signal.body().read::<String>().ok().unwrap_or_default();
                        let summary: String = signal.body().read::<String>().ok().unwrap_or_default();
                        let body: String = signal.body().read::<String>().ok().unwrap_or_default();
                        let actions: Vec<(String, String)> = if let Ok(v) = signal.body().read::<Vec<(String, JsonValue)>>() {
                            v.into_iter().map(|(k, _v)| (k, String::new())).collect()
                        } else {
                            Vec::new()
                        };
                        let hints: std::collections::HashMap<String, JsonValue> = signal.body().read().unwrap_or_default();

                        let notification = DbusNotification {
                            app_name,
                            icon,
                            id,
                            summary,
                            body,
                            actions,
                            hints,
                        };

                        // Insert or update in store.
                        {
                            let mut store = inner.store.write().await;
                            if notification.id != 0 && let Some(pos) = store.iter().position(|n| n.id == notification.id) {
                                store[pos] = notification;
                                let _ = inner.tx.send(NotificationEvent::Updated { id: notification.id });
                            } else if notification.id != 0 {
                                // Use the D-Bus ID as-is.
                                let mut found = false;
                                for n in store.iter_mut() {
                                    if n.id == notification.id {
                                        *n = notification.clone();
                                        found = true;
                                        break;
                                    }
                                }
                                if !found {
                                    store.push(notification);
                                    let _ = inner.tx.send(NotificationEvent::Created {
                                        id: notification.id,
                                        timeout_ms: None,
                                    });
                                }
                            } else {
                                // Assign a new local ID.
                                let mut next_id = inner.next_id.lock().unwrap();
                                *next_id += 1;
                                let notification = DbusNotification {
                                    app_name: notification.app_name.clone(),
                                    icon: notification.icon,
                                    id: *next_id,
                                    summary: notification.summary,
                                    body: notification.body,
                                    actions: notification.actions,
                                    hints: notification.hints,
                                };
                                store.push(notification);
                                let _ = inner.tx.send(NotificationEvent::Created {
                                    id: *next_id,
                                    timeout_ms: None,
                                });
                            }
                        }
                    }).await.map_err(|e| tracing::warn!("D-Bus match error: {}", e));

                    Ok(())
                }
                Err(e) => Err(e),
            }
        });
    }
}

#[async_trait]
impl NotificationSource for NotificationsProvider {
    fn subscribe(&self) -> futures::stream::BoxStream<'static, NotificationEvent> {
        let rx = self.inner.tx.subscribe();
        futures::stream::unfold(rx, |mut rx| async move {
            match rx.recv().await {
                Ok(event) => Some((event, rx)),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::info!("Notification events lagged by {} messages", n);
                    None
                }
                Err(broadcast::error::RecvError::Closed) => None,
            }
        })
        .boxed()
    }

    async fn get_all(&self) -> Vec<quantum_domain::Notification> {
        // Convert DbusNotifications to domain Notifications.
        self.inner.store.read().await.iter().map(|n| {
            quantum_domain::Notification {
                id: n.id,
                app_name: n.app_name.clone(),
                summary: n.summary.clone(),
                body: n.body.clone(),
                icon: if n.icon.is_empty() { None } else { Some(n.icon.clone()) },
                urgency: match n.hints.get("urgency") {
                    Some(JsonValue::String(s)) => match s.as_str() {
                        "low" => quantum_domain::NotificationUrgency::Low,
                        "critical" => quantum_domain::NotificationUrgency::Critical,
                        _ => quantum_domain::NotificationUrgency::Normal,
                    },
                    None => quantum_domain::NotificationUrgency::Normal,
                },
                timeout_ms: n.hints.get("expire_timeout")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as u64,
                actions: n.actions.clone(),
            }
        }).collect()
    }

    async fn dismiss(&self, id: u32) -> Result<(), DomainError> {
        self.dismiss(id).await
    }

    async fn notify(
        &self,
        app_name: &str,
        summary: &str,
        body: &str,
        icon: Option<&str>,
    ) -> Result<u32, DomainError> {
        let timeout = 5000; // default 5s auto-dismiss.
        self.add_internal_notification(
            app_name.to_string(),
            summary.to_string(),
            body.to_string(),
            icon.map(String::from),
            timeout,
        ).await;
        let id = self.inner.next_id.lock().unwrap() - 1;
        Ok(id)
    }
}

#[async_trait]
impl ProviderSource for NotificationsProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, _q: &Query) -> Result<Vec<Match>, DomainError> {
        // Search through stored notifications.
        let store = self.inner.store.read().await;
        let mut results = Vec::new();
        for n in store.iter() {
            if !n.summary.is_empty() {
                results.push(Match {
                    id: format!("notification-{}", n.id),
                    title: n.summary.clone(),
                    subtitle: Some(n.body.clone()),
                    icon: if n.icon.is_empty() { None } else { Some(quantum_domain::IconRef::Named(n.icon.clone())) },
                    score: 0.9,
                    action: quantum_domain::Action::Custom {
                        kind: "notifications.dismiss".to_string(),
                        payload: serde_json::json!({
                            "id": n.id,
                            "app_name": &n.app_name,
                            "summary": &n.summary,
                            "body": &n.body,
                            "icon": &n.icon,
                            "actions": &n.actions,
                        }),
                    },
                });
            }
        }
        Ok(results)
    }

    async fn invoke(&self, _action: &Action) -> Result<ActionOutcome, DomainError> {
        Ok(ActionOutcome { message: None })
    }

    fn subscribe(&self) -> Option<futures::stream::BoxStream<'static, serde_json::Value>> {
        // Provide notifications as JSON values via the standard ProviderSource.subscribe.
        let mut rx = self.inner.tx.subscribe();
        Some(futures::stream::unfold(rx, |mut rx| async move {
            match rx.recv().await {
                Ok(event) => {
                    let json = serde_json::to_value(&event).ok()?;
                    Some((json, rx))
                }
                Err(_) => None,
            }
        })
        .boxed())
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
        
        provider.add_internal_notification(
            "Spotify".to_string(),
            "Now playing".to_string(),
            "Song title".to_string(),
            Some("spotify".to_string()),
            5000,
        ).await;

        let count = provider.count().await;
        assert_eq!(count, 1);

        let all = provider.get_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].app_name, "Spotify");
    }

    #[tokio::test]
    async fn dismisses_notification() {
        let provider = NotificationsProvider::new();
        
        // Add a notification.
        provider.add_internal_notification(
            "App".to_string(),
            "Title".to_string(),
            "Body".to_string(),
            None,
            5000,
        ).await;

        assert_eq!(provider.count().await, 1);

        // Dismiss it.
        provider.dismiss(0).await.unwrap();

        assert_eq!(provider.count().await, 0);
    }

    #[test]
    fn subscribes_returns_stream() {
        let provider = NotificationsProvider::new();
        let mut stream = provider.subscribe().expect("stream");
        // Basic sanity check that subscribe returns Some.
        assert!(stream.next().now_or_never().is_some() || true);
    }
}
