use async_trait::async_trait;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, RwLock};

use quantum_domain::error::DomainError;
use quantum_domain::match_result::{IconRef, Match};
use quantum_domain::query::Query;
use quantum_domain::score::MatchScore;
use quantum_domain::{Action, ActionOutcome, ProviderId, ProviderSource};

#[derive(Debug, Clone)]
pub struct DbusNotification {
    pub app_name: String,
    pub icon: String,
    pub id: u32,
    pub summary: String,
    pub body: String,
    pub urgency: String,
    pub timeout_ms: u64,
    pub actions: Vec<(String, String)>,
    pub hints: HashMap<String, serde_json::Value>,
    /// True for notifications synthesized inside Quantum (for example timer
    /// completions) rather than received over D-Bus. Internal notifications are
    /// deduplicated by content among themselves and never fold into a D-Bus
    /// entry that happens to share an app name and summary.
    pub internal: bool,
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

/// Default auto-dismiss applied when a notification requests the server
/// default (D-Bus `expire_timeout == -1`), in milliseconds. The
/// org.freedesktop.Notifications spec leaves the concrete value to the server.
const DEFAULT_EXPIRE_MS: u64 = 5000;

/// Resolve a D-Bus `expire_timeout` into a stored `timeout_ms`, where the
/// value `0` means "never auto-dismiss".
///
/// Per the org.freedesktop.Notifications spec the incoming `expire_timeout` is:
/// `> 0` an explicit timeout in milliseconds, `0` meaning never expire, and
/// `-1` asking the server to pick a default. The previous implementation
/// collapsed both `0` and `-1` into `0`, so a notification that asked to
/// persist (`0`) was wrongly auto-dismissed using the toast's fallback. Here
/// the server default (`-1`) resolves to a concrete positive value, leaving
/// `0` to unambiguously mean never-expire. A critical notification that asks
/// for the server default persists until dismissed.
fn resolve_timeout_ms(expire_timeout: i32, urgency: &str) -> u64 {
    if expire_timeout > 0 {
        expire_timeout as u64
    } else if expire_timeout == 0 {
        0
    } else if urgency == "critical" {
        0
    } else {
        DEFAULT_EXPIRE_MS
    }
}

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
        urgency: String,
    ) -> u32 {
        let timeout_ms = resolve_timeout_ms(expire_timeout, &urgency);
        let event_timeout = if timeout_ms > 0 {
            Some(timeout_ms)
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
                slot.urgency = urgency;
                slot.timeout_ms = timeout_ms;
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
                urgency,
                timeout_ms,
                actions,
                hints: HashMap::new(),
                internal: false,
            });
            let _ = self.tx.send(NotificationEvent::Created {
                id: replaces_id,
                timeout_ms: event_timeout,
            });
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
            urgency,
            timeout_ms,
            actions,
            hints: HashMap::new(),
            internal: false,
        });
        let _ = self.tx.send(NotificationEvent::Created {
            id,
            timeout_ms: event_timeout,
        });
        id
    }

    /// Build a JSON snapshot of all active notifications for the event envelope.
    async fn snapshot_json(&self) -> Vec<serde_json::Value> {
        let store = self.store.read().await;
        store
            .iter()
            .map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "app_name": n.app_name,
                    "summary": n.summary,
                    "body": n.body,
                    "icon": if n.icon.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(n.icon.clone())
                    },
                    "urgency": n.urgency,
                    "timeout_ms": n.timeout_ms,
                    "actions": n.actions,
                })
            })
            .collect()
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

    pub async fn get_all(&self) -> Vec<DbusNotification> {
        self.inner.store.read().await.clone()
    }

    /// Add (or update) a notification synthesized inside Quantum, returning its
    /// id. Internal notifications sharing an app name and summary are
    /// deduplicated in place so a re-firing timer updates its existing entry
    /// rather than stacking duplicates; each distinct one gets a unique id so
    /// consumers can key and dismiss them independently.
    pub async fn add_internal_notification(
        &self,
        app_name: String,
        summary: String,
        body: String,
        icon: Option<String>,
        timeout_ms: u64,
    ) -> u32 {
        let mut store = self.inner.store.write().await;
        if let Some(pos) = store
            .iter()
            .position(|n| n.internal && n.app_name == app_name && n.summary == summary)
        {
            store[pos].body = body;
            let id = store[pos].id;
            let _ = self.inner.tx.send(NotificationEvent::Updated { id });
            return id;
        }
        let id = {
            let mut next = self.inner.next_id.lock().unwrap();
            *next += 1;
            *next
        };
        store.push(DbusNotification {
            app_name,
            icon: icon.unwrap_or_default(),
            id,
            summary,
            body,
            urgency: "normal".to_string(),
            timeout_ms,
            actions: Vec::new(),
            hints: HashMap::new(),
            internal: true,
        });
        let _ = self.inner.tx.send(NotificationEvent::Created {
            id,
            timeout_ms: Some(timeout_ms),
        });
        id
    }

    pub async fn dismiss(&self, id: u32) -> Result<(), DomainError> {
        {
            let mut store = self.inner.store.write().await;
            store.retain(|n| n.id != id);
        }
        let _ = self.inner.tx.send(NotificationEvent::Dismissed { id });
        if let Some(conn) = self.inner.conn.get() {
            if let Ok(ctxt) =
                zbus::object_server::SignalContext::new(conn, "/org/freedesktop/Notifications")
            {
                let _ = NotificationServer::notification_closed(&ctxt, id, 2).await;
            }
        }
        Ok(())
    }

    /// Notify the originating application that the user invoked an action.
    pub async fn invoke_action(&self, id: u32, action_key: &str) {
        if let Some(conn) = self.inner.conn.get() {
            if let Ok(ctxt) =
                zbus::object_server::SignalContext::new(conn, "/org/freedesktop/Notifications")
            {
                let _ = NotificationServer::action_invoked(&ctxt, id, action_key.to_string()).await;
            }
        }
    }

    pub async fn count(&self) -> usize {
        self.inner.store.read().await.len()
    }

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
                tracing::warn!(
                    "notifications: could not claim org.freedesktop.Notifications: {error}"
                );
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
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, _q: &Query) -> Result<Vec<Match>, DomainError> {
        let store = self.inner.store.read().await;
        Ok(store
            .iter()
            .filter(|n| !n.summary.is_empty())
            .map(|n| Match {
                id: format!("notification-{}", n.id),
                provider: self.id.clone(),
                title: n.summary.clone(),
                subtitle: Some(n.body.clone()),
                icon: if n.icon.is_empty() {
                    None
                } else {
                    Some(IconRef::Name(n.icon.clone()))
                },
                score: MatchScore::new(0.9),
                action: quantum_domain::Action::Custom {
                    kind: "notifications".to_string(),
                    payload: serde_json::json!({ "command": "dismiss", "id": n.id }),
                },
            })
            .collect())
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        let Action::Custom { kind, payload } = action else {
            return Ok(ActionOutcome { message: None });
        };
        if kind != "notifications" {
            return Ok(ActionOutcome { message: None });
        }
        let command = payload.get("command").and_then(serde_json::Value::as_str);
        match command {
            Some("dismiss") => match payload.get("id").and_then(serde_json::Value::as_u64) {
                Some(id) => {
                    self.dismiss(id as u32).await?;
                }
                None => {
                    tracing::warn!("notifications: dismiss command missing id");
                }
            },
            Some("action") => {
                let id = payload.get("id").and_then(serde_json::Value::as_u64);
                let action_key = payload
                    .get("action_key")
                    .and_then(serde_json::Value::as_str);
                match (id, action_key) {
                    (Some(id), Some(action_key)) => {
                        let id = id as u32;
                        self.invoke_action(id, action_key).await;
                        self.dismiss(id).await?;
                    }
                    _ => {
                        tracing::warn!("notifications: action command missing id or action_key");
                    }
                }
            }
            other => {
                tracing::warn!("notifications: unknown command {other:?}");
            }
        }
        Ok(ActionOutcome { message: None })
    }

    fn subscribe(&self) -> Option<futures::stream::BoxStream<'static, serde_json::Value>> {
        let rx = self.inner.tx.subscribe();
        let inner = self.inner.clone();
        // State machine for the stream: first yield the current snapshot so a
        // freshly opened consumer (and `provider.query`) catches up without
        // waiting for the next change event, then stream subsequent changes.
        enum StreamState {
            Initial,
            Streaming,
        }
        Some(
            futures::stream::unfold(
                (StreamState::Initial, rx, inner),
                |(state, mut rx, inner)| async move {
                    if let StreamState::Initial = state {
                        let notifications = inner.snapshot_json().await;
                        let envelope = serde_json::json!({
                            "change": serde_json::Value::Null,
                            "notifications": notifications,
                        });
                        return Some((envelope, (StreamState::Streaming, rx, inner)));
                    }
                    loop {
                        match rx.recv().await {
                            Ok(event) => {
                                let change = serde_json::to_value(&event).ok()?;
                                let notifications = inner.snapshot_json().await;
                                let envelope = serde_json::json!({
                                    "change": change,
                                    "notifications": notifications,
                                });
                                return Some((envelope, (StreamState::Streaming, rx, inner)));
                            }
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!(
                                    "notifications subscription lagged: {skipped} events dropped"
                                );
                                continue;
                            }
                            Err(broadcast::error::RecvError::Closed) => return None,
                        }
                    }
                },
            )
            .boxed(),
        )
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

    /// Map the D-Bus `urgency` hint (a byte) to a string. 0 => low, 1 => normal,
    /// 2 => critical; anything else or absent => normal.
    fn urgency_from_hints(
        hints: &std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> String {
        let level = hints
            .get("urgency")
            .and_then(|value| u8::try_from(value).ok());
        match level {
            Some(0) => "low".to_string(),
            Some(2) => "critical".to_string(),
            _ => "normal".to_string(),
        }
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
        hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let actions = Self::pair_actions(actions);
        let urgency = Self::urgency_from_hints(&hints);
        self.inner
            .apply_notify(
                app_name,
                app_icon,
                replaces_id,
                summary,
                body,
                actions,
                expire_timeout,
                urgency,
            )
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
        provider
            .add_internal_notification(
                "Spotify".into(),
                "Now playing".into(),
                "Song title".into(),
                Some("spotify".into()),
                5000,
            )
            .await;
        assert_eq!(provider.count().await, 1);
    }
    #[tokio::test]
    async fn dismisses_notification() {
        let provider = NotificationsProvider::new();
        let id = provider
            .add_internal_notification("App".into(), "Title".into(), "Body".into(), None, 5000)
            .await;
        assert_eq!(provider.count().await, 1);
        provider.dismiss(id).await.unwrap();
        assert_eq!(provider.count().await, 0);
    }

    #[tokio::test]
    async fn internal_notifications_get_distinct_nonzero_ids() {
        // Two different timers (same app name, different label) must produce
        // two separately keyed entries, not collapse onto a shared id 0.
        let provider = NotificationsProvider::new();
        let first = provider
            .add_internal_notification(
                "Quantum Timer".into(),
                "Tea".into(),
                "Timer complete".into(),
                None,
                0,
            )
            .await;
        let second = provider
            .add_internal_notification(
                "Quantum Timer".into(),
                "Pasta".into(),
                "Timer complete".into(),
                None,
                0,
            )
            .await;
        assert_ne!(first, 0);
        assert_ne!(second, 0);
        assert_ne!(first, second);
        assert_eq!(provider.count().await, 2);
    }

    #[tokio::test]
    async fn internal_notification_dedupes_same_content_in_place() {
        // Re-firing the same logical notification updates the existing entry
        // and keeps its id, rather than stacking a duplicate.
        let provider = NotificationsProvider::new();
        let first = provider
            .add_internal_notification(
                "Quantum Timer".into(),
                "Tea".into(),
                "Brewing".into(),
                None,
                0,
            )
            .await;
        let again = provider
            .add_internal_notification(
                "Quantum Timer".into(),
                "Tea".into(),
                "Timer complete".into(),
                None,
                0,
            )
            .await;
        assert_eq!(first, again);
        assert_eq!(provider.count().await, 1);
        let all = provider.get_all().await;
        assert_eq!(all[0].body, "Timer complete");
    }

    #[tokio::test]
    async fn dismiss_removes_only_targeted_internal_notification() {
        let provider = NotificationsProvider::new();
        let first = provider
            .add_internal_notification(
                "Quantum Timer".into(),
                "Tea".into(),
                "Timer complete".into(),
                None,
                0,
            )
            .await;
        let second = provider
            .add_internal_notification(
                "Quantum Timer".into(),
                "Pasta".into(),
                "Timer complete".into(),
                None,
                0,
            )
            .await;
        provider.dismiss(first).await.unwrap();
        let all = provider.get_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, second);
        assert_eq!(all[0].summary, "Pasta");
    }

    #[tokio::test]
    async fn internal_dedup_does_not_match_dbus_notification() {
        // An internal add must never fold itself into a D-Bus notification that
        // happens to share an app name and summary; they are distinct sources.
        let provider = NotificationsProvider::new();
        provider
            .inner
            .apply_notify(
                "Quantum Timer".into(),
                String::new(),
                0,
                "Tea".into(),
                "From app".into(),
                Vec::new(),
                0,
                "normal".into(),
            )
            .await;
        provider
            .add_internal_notification(
                "Quantum Timer".into(),
                "Tea".into(),
                "Timer complete".into(),
                None,
                0,
            )
            .await;
        assert_eq!(provider.count().await, 2);
    }
    #[test]
    fn subscribes_returns_stream() {
        let provider = NotificationsProvider::new();
        assert!(provider.subscribe().is_some());
    }

    #[test]
    fn resolve_timeout_distinguishes_never_default_and_explicit() {
        // An explicit positive timeout is honored verbatim.
        assert_eq!(resolve_timeout_ms(5000, "normal"), 5000);
        // Zero means never auto-dismiss and must not be turned into a default.
        assert_eq!(resolve_timeout_ms(0, "normal"), 0);
        // The server default (-1) resolves to a concrete positive value.
        assert_eq!(resolve_timeout_ms(-1, "normal"), DEFAULT_EXPIRE_MS);
        // A critical notification asking for the server default persists.
        assert_eq!(resolve_timeout_ms(-1, "critical"), 0);
        // An explicit positive timeout still wins for critical urgency.
        assert_eq!(resolve_timeout_ms(3000, "critical"), 3000);
    }

    #[tokio::test]
    async fn apply_notify_persists_zero_timeout_as_never() {
        // A D-Bus notification with expire_timeout 0 must be stored with
        // timeout_ms 0 (never), not silently rewritten to a default.
        let provider = NotificationsProvider::new();
        provider
            .inner
            .apply_notify(
                "App".into(),
                String::new(),
                0,
                "Persistent".into(),
                "Body".into(),
                Vec::new(),
                0,
                "normal".into(),
            )
            .await;
        let all = provider.get_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].timeout_ms, 0);
    }

    #[tokio::test]
    async fn apply_notify_resolves_server_default_to_concrete_value() {
        let provider = NotificationsProvider::new();
        provider
            .inner
            .apply_notify(
                "App".into(),
                String::new(),
                0,
                "Default".into(),
                "Body".into(),
                Vec::new(),
                -1,
                "normal".into(),
            )
            .await;
        let all = provider.get_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].timeout_ms, DEFAULT_EXPIRE_MS);
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
                "normal".into(),
            )
            .await;
        assert_eq!(id, 1);
        let all = provider.get_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, 1);
        assert_eq!(all[0].app_name, "Spotify");
        assert_eq!(all[0].summary, "Now playing");
        assert_eq!(
            all[0].actions,
            vec![("default".to_string(), "Open".to_string())]
        );
    }

    #[tokio::test]
    async fn dismiss_succeeds_without_dbus_connection() {
        let provider = NotificationsProvider::new();
        let id = provider
            .inner
            .apply_notify(
                "App".into(),
                "".into(),
                0,
                "T".into(),
                "B".into(),
                Vec::new(),
                0,
                "normal".into(),
            )
            .await;
        provider.dismiss(id).await.unwrap();
        assert_eq!(provider.count().await, 0);
    }

    #[tokio::test]
    async fn subscribe_emits_initial_snapshot_without_prior_event() {
        // A freshly opened consumer (for example the notification center) must
        // receive the current notification list on subscribe, even when no
        // create/update/dismiss event fires afterwards. This is the documented
        // streaming contract every other provider honors and is what
        // `provider.query` relies on to catch up.
        let provider = NotificationsProvider::new();
        provider.inner.store.write().await.push(DbusNotification {
            app_name: "Spotify".into(),
            icon: String::new(),
            id: 7,
            summary: "Now playing".into(),
            body: "Song".into(),
            urgency: "normal".into(),
            timeout_ms: 5000,
            actions: Vec::new(),
            hints: HashMap::new(),
            internal: false,
        });

        let mut stream = provider.subscribe().expect("stream");
        let value = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("subscribe must emit an initial snapshot within the timeout")
            .expect("envelope");

        let notifications = value["notifications"].as_array().expect("array");
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0]["app_name"], "Spotify");
        assert_eq!(notifications[0]["id"], 7);
    }

    #[tokio::test]
    async fn subscribe_emits_envelope_with_snapshot() {
        let provider = NotificationsProvider::new();
        let mut stream = provider.subscribe().expect("stream");
        // First emission is the initial snapshot (empty, change null); the
        // create event follows as the second emission.
        let initial = stream.next().await.expect("initial snapshot");
        assert!(initial["change"].is_null());
        assert_eq!(initial["notifications"].as_array().expect("array").len(), 0);
        provider
            .inner
            .apply_notify(
                "Spotify".into(),
                "spotify".into(),
                0,
                "Now playing".into(),
                "Song".into(),
                vec![("default".into(), "Open".into())],
                5000,
                "critical".into(),
            )
            .await;
        let value = stream.next().await.expect("envelope");
        assert_eq!(value["change"]["type"], "created");
        let notifications = value["notifications"].as_array().expect("array");
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0]["app_name"], "Spotify");
        assert_eq!(notifications[0]["urgency"], "critical");
        assert_eq!(notifications[0]["timeout_ms"], 5000);
        assert_eq!(notifications[0]["actions"][0][0], "default");
        assert_eq!(notifications[0]["actions"][0][1], "Open");
    }

    #[test]
    fn urgency_from_hints_maps_known_bytes() {
        use zbus::zvariant::OwnedValue;

        let mut hints: HashMap<String, OwnedValue> = HashMap::new();

        hints.insert("urgency".to_string(), OwnedValue::from(0u8));
        assert_eq!(NotificationServer::urgency_from_hints(&hints), "low");

        hints.insert("urgency".to_string(), OwnedValue::from(2u8));
        assert_eq!(NotificationServer::urgency_from_hints(&hints), "critical");

        hints.insert("urgency".to_string(), OwnedValue::from(1u8));
        assert_eq!(NotificationServer::urgency_from_hints(&hints), "normal");

        hints.insert("urgency".to_string(), OwnedValue::from(7u8));
        assert_eq!(NotificationServer::urgency_from_hints(&hints), "normal");

        hints.remove("urgency");
        assert_eq!(NotificationServer::urgency_from_hints(&hints), "normal");
    }

    #[tokio::test]
    async fn invoke_dismiss_removes_notification() {
        let provider = NotificationsProvider::new();
        let id = provider
            .inner
            .apply_notify(
                "App".into(),
                "".into(),
                0,
                "T".into(),
                "B".into(),
                Vec::new(),
                0,
                "normal".into(),
            )
            .await;
        assert_eq!(provider.count().await, 1);
        let action = Action::Custom {
            kind: "notifications".to_string(),
            payload: serde_json::json!({ "command": "dismiss", "id": id }),
        };
        let outcome = provider.invoke(&action).await.unwrap();
        assert!(outcome.message.is_none());
        assert_eq!(provider.count().await, 0);
    }

    #[tokio::test]
    async fn invoke_unknown_command_is_noop() {
        let provider = NotificationsProvider::new();
        let id = provider
            .inner
            .apply_notify(
                "App".into(),
                "".into(),
                0,
                "T".into(),
                "B".into(),
                Vec::new(),
                0,
                "normal".into(),
            )
            .await;
        assert_eq!(provider.count().await, 1);
        let action = Action::Custom {
            kind: "notifications".to_string(),
            payload: serde_json::json!({ "command": "frobnicate", "id": id }),
        };
        let outcome = provider.invoke(&action).await.unwrap();
        assert!(outcome.message.is_none());
        assert_eq!(provider.count().await, 1);
    }

    #[tokio::test]
    async fn apply_notify_replaces_existing_id() {
        let provider = NotificationsProvider::new();
        let id = provider
            .inner
            .apply_notify(
                "App".into(),
                "".into(),
                0,
                "First".into(),
                "".into(),
                Vec::new(),
                0,
                "normal".into(),
            )
            .await;
        let same = provider
            .inner
            .apply_notify(
                "App".into(),
                "".into(),
                id,
                "Second".into(),
                "".into(),
                Vec::new(),
                0,
                "normal".into(),
            )
            .await;
        assert_eq!(same, id);
        let all = provider.get_all().await;
        assert_eq!(all.len(), 1, "replace must not add a second entry");
        assert_eq!(all[0].summary, "Second");
    }
}
