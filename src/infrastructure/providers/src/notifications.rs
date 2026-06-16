use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use futures::StreamExt;
use tokio::sync::{broadcast, RwLock};
use async_trait::async_trait;

use quantum_domain::error::DomainError;
use quantum_domain::match_result::{IconRef, Match};
use quantum_domain::score::MatchScore;
use quantum_domain::query::Query;
use quantum_domain::{Action, ActionOutcome, ProviderId, ProviderSource};

#[derive(Debug, Clone)]
pub struct DbusNotification {
    pub app_name: String,
    pub icon: String,
    pub id: u32,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub hints: HashMap<String, serde_json::Value>,
}

#[derive(Debug)]
pub struct NotificationsProvider {
    id: ProviderId,
    inner: Arc<NotificationsInner>,
}

#[derive(Debug)]
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

    pub async fn get_all(&self) -> Vec<DbusNotification> { self.inner.store.read().await.clone() }

    pub async fn add_internal_notification(&self, app_name: String, summary: String, body: String, icon: Option<String>, timeout_ms: u64) {
        let mut store = self.inner.store.write().await;
        if let Some(pos) = store.iter().position(|n| n.app_name == app_name && n.summary == summary && n.id == 0) {
            store[pos].body = body;
            let _ = self.inner.tx.send(NotificationEvent::Updated { id: 0 });
        } else {
            let mut next = self.inner.next_id.lock().unwrap();
            *next += 1;
            store.push(DbusNotification { app_name, icon: icon.unwrap_or_default(), id: 0, summary, body, actions: Vec::new(), hints: HashMap::new() });
        }
        let _ = self.inner.tx.send(NotificationEvent::Created { id: 0, timeout_ms: Some(timeout_ms) });
    }

    pub async fn dismiss(&self, id: u32) -> Result<(), DomainError> {
        let mut store = self.inner.store.write().await;
        store.retain(|n| n.id != id);
        let _ = self.inner.tx.send(NotificationEvent::Dismissed { id });
        Ok(())
    }

    pub async fn count(&self) -> usize { self.inner.store.read().await.len() }

    /// Start the D-Bus notification server. Filled in by later tasks.
    pub async fn start_dbus(&self) {}
}

#[async_trait]
impl ProviderSource for NotificationsProvider {
    fn id(&self) -> &ProviderId { &self.id }

    async fn search(&self, _q: &Query) -> Result<Vec<Match>, DomainError> {
        let store = self.inner.store.read().await;
        Ok(store.iter()
            .filter(|n| !n.summary.is_empty())
            .map(|n| Match {
                id: format!("notification-{}", n.id),
                provider: self.id.clone(),
                title: n.summary.clone(),
                subtitle: Some(n.body.clone()),
                icon: if n.icon.is_empty() { None } else { Some(IconRef::Name(n.icon.clone())) },
                score: MatchScore::new(0.9),
                action: quantum_domain::Action::Custom {
                    kind: "notifications.dismiss".to_string(),
                    payload: serde_json::json!({ "id": n.id, "app_name": &n.app_name }),
                },
            }).collect())
    }

    async fn invoke(&self, _action: &Action) -> Result<ActionOutcome, DomainError> { Ok(ActionOutcome { message: None }) }

    fn subscribe(&self) -> Option<futures::stream::BoxStream<'static, serde_json::Value>> {
        let rx = self.inner.tx.subscribe();
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
        assert!(provider.get_all().await.is_empty());
    }
    #[tokio::test]
    async fn adds_and_counts_notification() {
        let provider = NotificationsProvider::new();
        provider.add_internal_notification("Spotify".into(), "Now playing".into(), "Song title".into(), Some("spotify".into()), 5000).await;
        assert_eq!(provider.count().await, 1);
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
