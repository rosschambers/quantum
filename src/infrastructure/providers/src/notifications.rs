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
    conn: tokio::sync::OnceCell<zbus::Connection>,
}

use quantum_domain::NotificationEvent;

impl NotificationsInner {
    #[allow(clippy::too_many_arguments)]
    async fn apply_notify(
        &self,
        app_name: String,
        app_icon: String,
        replaces_id: u32,
        summary: String,
        body: String,
        actions: Vec<(String, String)>,
        expire_timeout: i32,
    ) -> u32 {
        let timeout_ms = if expire_timeout > 0 {
            Some(expire_timeout as u64)
        } else {
            None
        };
        let mut store = self.store.write().await;

        if replaces_id != 0 {
            if let Some(slot) = store.iter_mut().find(|n| n.id == replaces_id) {
                slot.app_name = app_name;
                slot.icon = app_icon;
                slot.summary = summary;
                slot.body = body;
                slot.actions = actions;
                let _ = self.tx.send(NotificationEvent::Updated { id: replaces_id });
                return replaces_id;
            }
            store.push(DbusNotification {
                app_name,
                icon: app_icon,
                id: replaces_id,
                summary,
                body,
                actions,
                hints: HashMap::new(),
            });
            let _ = self
                .tx
                .send(NotificationEvent::Created { id: replaces_id, timeout_ms });
            return replaces_id;
        }

        let id = {
            let mut next = self.next_id.lock().unwrap();
            *next += 1;
            *next
        };
        store.push(DbusNotification {
            app_name,
            icon: app_icon,
            id,
            summary,
            body,
            actions,
            hints: HashMap::new(),
        });
        let _ = self.tx.send(NotificationEvent::Created { id, timeout_ms });
        id
    }
}

impl Default for NotificationsProvider {
    fn default() -> Self {
        Self::new()
    }
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
                conn: tokio::sync::OnceCell::new(),
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
        {
            let mut store = self.inner.store.write().await;
            store.retain(|n| n.id != id);
        }
        let _ = self.inner.tx.send(NotificationEvent::Dismissed { id });
        if let Some(conn) = self.inner.conn.get() {
            if let Ok(ctxt) = zbus::object_server::SignalContext::new(
                conn,
                "/org/freedesktop/Notifications",
            ) {
                let _ = NotificationServer::notification_closed(&ctxt, id, 2).await;
            }
        }
        Ok(())
    }

    /// Notify the originating application that the user invoked an action.
    pub async fn invoke_action(&self, id: u32, action_key: &str) {
        if let Some(conn) = self.inner.conn.get() {
            if let Ok(ctxt) = zbus::object_server::SignalContext::new(
                conn,
                "/org/freedesktop/Notifications",
            ) {
                let _ =
                    NotificationServer::action_invoked(&ctxt, id, action_key.to_string()).await;
            }
        }
    }

    pub async fn count(&self) -> usize { self.inner.store.read().await.len() }

    /// Become the org.freedesktop.Notifications server. Replaces any running
    /// notification daemon and keeps the connection alive for the process
    /// lifetime by storing it in the shared inner state.
    pub async fn start_dbus(&self) {
        use zbus::fdo::{RequestNameFlags, RequestNameReply};

        let server = NotificationServer::new(self.inner.clone());
        let conn = match zbus::connection::Builder::session()
            .and_then(|builder| builder.serve_at("/org/freedesktop/Notifications", server))
        {
            Ok(builder) => match builder.build().await {
                Ok(conn) => conn,
                Err(error) => {
                    tracing::warn!("notifications: failed to build D-Bus connection: {error}");
                    return;
                }
            },
            Err(error) => {
                tracing::warn!("notifications: failed to configure D-Bus server: {error}");
                return;
            }
        };

        let reply = match conn
            .request_name_with_flags(
                "org.freedesktop.Notifications",
                RequestNameFlags::ReplaceExisting | RequestNameFlags::AllowReplacement,
            )
            .await
        {
            Ok(reply) => reply,
            Err(error) => {
                tracing::warn!("notifications: could not claim org.freedesktop.Notifications: {error}");
                return;
            }
        };

        // Keep the connection alive regardless so we receive ownership later if
        // the current daemon exits (we requested queueing, not DoNotQueue).
        if self.inner.conn.set(conn).is_err() {
            tracing::warn!("notifications: D-Bus server already started");
            return;
        }

        match reply {
            RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => {
                tracing::info!("notifications: serving org.freedesktop.Notifications");
            }
            RequestNameReply::InQueue => {
                tracing::warn!(
                    "notifications: another notification daemon owns \
                     org.freedesktop.Notifications and did not allow replacement; \
                     stop it (for example dunst, mako, swaync) so Quantum can take over. \
                     Quantum is queued and will take over if it exits."
                );
            }
            RequestNameReply::Exists => {
                tracing::warn!(
                    "notifications: org.freedesktop.Notifications is owned by another \
                     daemon that refused replacement; stop it so Quantum can take over."
                );
            }
        }
    }
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

/// D-Bus handler for `org.freedesktop.Notifications`. Shares the provider store.
pub struct NotificationServer {
    inner: Arc<NotificationsInner>,
}

impl NotificationServer {
    fn new(inner: Arc<NotificationsInner>) -> Self {
        Self { inner }
    }

    /// Convert the flat D-Bus actions list [key1, label1, key2, label2, ...]
    /// into (key, label) pairs. A trailing unpaired entry is dropped.
    fn pair_actions(actions: Vec<String>) -> Vec<(String, String)> {
        let mut pairs = Vec::with_capacity(actions.len() / 2);
        let mut iter = actions.into_iter();
        while let (Some(key), Some(label)) = (iter.next(), iter.next()) {
            pairs.push((key, label));
        }
        pairs
    }
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl NotificationServer {
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        _hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let actions = Self::pair_actions(actions);
        self.inner
            .apply_notify(app_name, app_icon, replaces_id, summary, body, actions, expire_timeout)
            .await
    }

    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".to_string(), "actions".to_string()]
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "Quantum".to_string(),
            "quantum".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            "1.2".to_string(),
        )
    }

    async fn close_notification(
        &self,
        #[zbus(signal_context)] ctxt: zbus::object_server::SignalContext<'_>,
        id: u32,
    ) {
        {
            let mut store = self.inner.store.write().await;
            store.retain(|n| n.id != id);
        }
        let _ = self.inner.tx.send(NotificationEvent::Dismissed { id });
        let _ = Self::notification_closed(&ctxt, id, 3).await;
    }

    #[zbus(signal)]
    async fn notification_closed(
        ctxt: &zbus::object_server::SignalContext<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        ctxt: &zbus::object_server::SignalContext<'_>,
        id: u32,
        action_key: String,
    ) -> zbus::Result<()>;
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

    #[tokio::test]
    async fn apply_notify_creates_with_new_id() {
        let provider = NotificationsProvider::new();
        let id = provider
            .inner
            .apply_notify(
                "Spotify".into(),
                "spotify".into(),
                0,
                "Now playing".into(),
                "Song title".into(),
                vec![("default".into(), "Open".into())],
                5000,
            )
            .await;
        assert_eq!(id, 1);
        let all = provider.get_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, 1);
        assert_eq!(all[0].app_name, "Spotify");
        assert_eq!(all[0].summary, "Now playing");
        assert_eq!(all[0].actions, vec![("default".to_string(), "Open".to_string())]);
    }

    #[tokio::test]
    async fn dismiss_succeeds_without_dbus_connection() {
        let provider = NotificationsProvider::new();
        let id = provider
            .inner
            .apply_notify("App".into(), "".into(), 0, "T".into(), "B".into(), Vec::new(), 0)
            .await;
        provider.dismiss(id).await.unwrap();
        assert_eq!(provider.count().await, 0);
    }

    #[tokio::test]
    async fn apply_notify_replaces_existing_id() {
        let provider = NotificationsProvider::new();
        let id = provider
            .inner
            .apply_notify("App".into(), "".into(), 0, "First".into(), "".into(), Vec::new(), 0)
            .await;
        let same = provider
            .inner
            .apply_notify("App".into(), "".into(), id, "Second".into(), "".into(), Vec::new(), 0)
            .await;
        assert_eq!(same, id);
        let all = provider.get_all().await;
        assert_eq!(all.len(), 1, "replace must not add a second entry");
        assert_eq!(all[0].summary, "Second");
    }
}
