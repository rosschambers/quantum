//! The system tray [`ProviderSource`] implementation.
//!
//! Wires the bus-facing host ([`super::host::run_system_tray_host`]) into the
//! provider registry: it owns the shared [`SystemTrayState`], the broadcast
//! channel the host publishes on, and the per-item handles the host mirrors.
//! A single background task runs the host under a reconnect backoff loop, and
//! `invoke` forwards user activations (activate, menu selection, about-to-show)
//! back to the originating application over DBus.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use serde_json::Value;
use tokio::sync::{broadcast, Mutex};
use tokio_stream::wrappers::BroadcastStream;
use zbus::names::BusName;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, Proxy};

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, ProviderId, ProviderSource, Query, SystemTrayState,
};

use super::host::{run_system_tray_host, HostExit, ItemHandles};
use super::menu::parse_menu_layout;

/// Whether the provider's reconnect loop should stop for good or reconnect
/// after a backoff.
enum LoopAction {
    /// Stop the loop and never retry.
    Stop,
    /// Reconnect after a backoff.
    Retry,
}

/// Map a host run's outcome to a loop decision: only a dormant exit stops the
/// loop; a disconnect or a transport error is retried.
fn next_loop_action(result: &Result<HostExit, quantum_dbus::DbusError>) -> LoopAction {
    match result {
        Ok(HostExit::Dormant) => LoopAction::Stop,
        Ok(HostExit::Disconnected) => LoopAction::Retry,
        Err(_) => LoopAction::Retry,
    }
}

/// The two interfaces a StatusNotifierItem may export `Activate` under. The
/// KDE name is tried first; freedesktop-only applications need the fallback.
const ITEM_INTERFACES: [&str; 2] = [
    "org.kde.StatusNotifierItem",
    "org.freedesktop.StatusNotifierItem",
];

/// The com.canonical.dbusmenu interface a tray item's menu is served under.
const DBUSMENU_INTERFACE: &str = "com.canonical.dbusmenu";

/// The DBus coordinates of a mirrored tray item, copied out of the shared
/// [`ItemHandles`] map so the lock is not held across the DBus call.
struct ItemCoordinates {
    bus_name: String,
    item_path: String,
    menu_path: Option<String>,
}

/// System tray provider: mirrors StatusNotifierItems into the shared state and
/// forwards activations back to the applications over DBus.
pub struct SystemTrayProvider {
    id: ProviderId,
    shared: Arc<Mutex<SystemTrayState>>,
    tx: broadcast::Sender<serde_json::Value>,
    handles: Arc<Mutex<ItemHandles>>,
}

impl SystemTrayProvider {
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        let id = ProviderId::from("system_tray");
        let shared = Arc::new(Mutex::new(SystemTrayState::default()));
        let (tx, _rx) = broadcast::channel::<serde_json::Value>(16);
        let handles = Arc::new(Mutex::new(ItemHandles::default()));

        let shared_for_task = shared.clone();
        let tx_for_task = tx.clone();
        let handles_for_task = handles.clone();

        runtime.spawn(async move {
            let mut backoff_secs = 1u64;
            loop {
                let result = run_system_tray_host(
                    shared_for_task.clone(),
                    tx_for_task.clone(),
                    handles_for_task.clone(),
                )
                .await;
                match next_loop_action(&result) {
                    LoopAction::Stop => {
                        // Dormant: another watcher owns the name. Do not
                        // busy-loop reclaiming it.
                        break;
                    }
                    LoopAction::Retry => {
                        match &result {
                            Ok(_) => tracing::warn!(
                                "system_tray host disconnected; reconnecting after backoff"
                            ),
                            Err(error) => tracing::warn!("system_tray host error: {error}"),
                        }
                        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                        backoff_secs = (backoff_secs * 2).min(30);
                    }
                }
            }
        });

        Self {
            id,
            shared,
            tx,
            handles,
        }
    }

    /// Copy the DBus coordinates of a mirrored item out of the handle map.
    /// Returns `ActionFailed` when the service key is not currently mirrored,
    /// before any DBus connection is opened.
    async fn coordinates_for(&self, service: &str) -> Result<ItemCoordinates, DomainError> {
        let handles = self.handles.lock().await;
        let handle = handles
            .by_service
            .get(service)
            .ok_or_else(|| DomainError::ActionFailed {
                reason: format!("unknown system tray service: {service}"),
            })?;
        Ok(ItemCoordinates {
            bus_name: handle.bus_name.clone(),
            item_path: handle.item_path.clone(),
            menu_path: handle.menu_path.clone(),
        })
    }

    async fn activate(&self, payload: &Value) -> Result<ActionOutcome, DomainError> {
        let service = require_str(payload, "service")?;
        let coordinates = self.coordinates_for(&service).await?;
        let conn = session_connection().await?;
        let bus_name = build_bus_name(&coordinates.bus_name)?;

        let mut last_error: Option<String> = None;
        for interface in ITEM_INTERFACES {
            let proxy = match Proxy::new(
                &conn,
                bus_name.clone(),
                coordinates.item_path.clone(),
                interface,
            )
            .await
            {
                Ok(proxy) => proxy,
                Err(error) => {
                    last_error = Some(format!("build {interface} proxy: {error}"));
                    continue;
                }
            };
            match proxy.call_method("Activate", &(0i32, 0i32)).await {
                Ok(_) => {
                    return Ok(ActionOutcome {
                        message: Some(format!("activated system tray item {service}")),
                    });
                }
                Err(error) => {
                    last_error = Some(format!("{interface} Activate failed: {error}"));
                }
            }
        }

        Err(DomainError::ActionFailed {
            reason: last_error.unwrap_or_else(|| "activate failed".into()),
        })
    }

    async fn menu_select(&self, payload: &Value) -> Result<ActionOutcome, DomainError> {
        let service = require_str(payload, "service")?;
        let item_id = require_i32(payload, "item_id")?;
        let coordinates = self.coordinates_for(&service).await?;
        let menu_path = coordinates
            .menu_path
            .ok_or_else(|| DomainError::ActionFailed {
                reason: format!("system tray service {service} has no menu"),
            })?;

        let conn = session_connection().await?;
        let bus_name = build_bus_name(&coordinates.bus_name)?;
        let proxy = Proxy::new(&conn, bus_name, menu_path, DBUSMENU_INTERFACE)
            .await
            .map_err(|error| DomainError::ActionFailed {
                reason: format!("build dbusmenu proxy: {error}"),
            })?;

        // dbusmenu Event signature is `(i s v u)`: id, eventId, data (variant),
        // timestamp. A "clicked" event carries no data, so pass an empty
        // variant (a zero u32) as the placeholder.
        let data = zbus::zvariant::Value::from(0u32);
        proxy
            .call_method("Event", &(item_id, "clicked", data, unix_timestamp_u32()))
            .await
            .map_err(|error| DomainError::ActionFailed {
                reason: format!("dbusmenu Event failed: {error}"),
            })?;

        Ok(ActionOutcome {
            message: Some(format!(
                "selected menu item {item_id} on system tray service {service}"
            )),
        })
    }

    async fn about_to_show(&self, payload: &Value) -> Result<ActionOutcome, DomainError> {
        let service = require_str(payload, "service")?;
        let coordinates = self.coordinates_for(&service).await?;
        let menu_path = coordinates
            .menu_path
            .ok_or_else(|| DomainError::ActionFailed {
                reason: format!("system tray service {service} has no menu"),
            })?;

        let conn = session_connection().await?;
        let bus_name = build_bus_name(&coordinates.bus_name)?;
        let proxy = Proxy::new(&conn, bus_name, menu_path, DBUSMENU_INTERFACE)
            .await
            .map_err(|error| DomainError::ActionFailed {
                reason: format!("build dbusmenu proxy: {error}"),
            })?;

        // AboutToShow lets the application populate the menu before it is
        // displayed and returns whether the layout changed. A call failure is
        // non-fatal: the mirror task's LayoutUpdated subscription still
        // refreshes the tree eventually.
        let need_update: bool = match proxy.call("AboutToShow", &0i32).await {
            Ok(need_update) => need_update,
            Err(error) => {
                tracing::warn!("system_tray: AboutToShow failed for {service}: {error}");
                return Ok(ActionOutcome {
                    message: Some(format!("requested menu for system tray service {service}")),
                });
            }
        };

        // When the application reports a changed layout, refetch it now and
        // rebroadcast so the frontend's follow-up query sees the fresh tree.
        if need_update {
            self.refetch_menu(&proxy, &service).await;
        }

        Ok(ActionOutcome {
            message: Some(format!("requested menu for system tray service {service}")),
        })
    }

    /// Refetch a menu's `GetLayout`, rebuild the matching item's tree in the
    /// shared state, and broadcast the new full state. A call or parse failure
    /// is non-fatal and simply leaves the existing tree in place.
    async fn refetch_menu(&self, proxy: &Proxy<'_>, service: &str) {
        let reply: (u32, OwnedValue) = match proxy
            .call("GetLayout", &(0i32, -1i32, &[] as &[&str]))
            .await
        {
            Ok(reply) => reply,
            Err(error) => {
                tracing::warn!("system_tray: GetLayout failed for {service}: {error}");
                return;
            }
        };
        let menu = parse_menu_layout(&reply.1);

        let mut state = self.shared.lock().await;
        if let Some(item) = state.items.iter_mut().find(|item| item.service == service) {
            item.menu = menu;
        }
        let value = serde_json::to_value(&*state).unwrap_or(Value::Null);
        let _ = self.tx.send(value);
    }
}

#[async_trait]
impl ProviderSource for SystemTrayProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, _: &Query) -> Result<Vec<Match>, DomainError> {
        Ok(vec![])
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Custom { kind, payload } if kind == "system_tray" => {
                let command = require_str(payload, "command")?;
                match command.as_str() {
                    "activate" => self.activate(payload).await,
                    "menu_select" => self.menu_select(payload).await,
                    "about_to_show" => self.about_to_show(payload).await,
                    other => Err(DomainError::ActionFailed {
                        reason: format!("unknown system tray command: {other}"),
                    }),
                }
            }
            _ => Err(DomainError::Unsupported(
                "system tray provider only handles custom actions with kind='system_tray'".into(),
            )),
        }
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        let rx = self.tx.subscribe();
        Some(
            BroadcastStream::new(rx)
                .filter_map(|res| async move { res.ok() })
                .boxed(),
        )
    }

    async fn snapshot(&self) -> Option<serde_json::Value> {
        Some(serde_json::to_value(&*self.shared.lock().await).unwrap_or(Value::Null))
    }
}

/// Read a required string field from the action payload.
fn require_str(payload: &Value, field: &str) -> Result<String, DomainError> {
    payload
        .get(field)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| DomainError::ActionFailed {
            reason: format!("missing system tray field: {field}"),
        })
}

/// Read a required `i32` field from the action payload.
fn require_i32(payload: &Value, field: &str) -> Result<i32, DomainError> {
    payload
        .get(field)
        .and_then(|value| value.as_i64())
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| DomainError::ActionFailed {
            reason: format!("missing or invalid system tray field: {field}"),
        })
}

/// Open a fresh session-bus connection for a single invocation, matching the
/// MPRIS provider's per-action connection pattern.
async fn session_connection() -> Result<Connection, DomainError> {
    Connection::session()
        .await
        .map_err(|error| DomainError::ActionFailed {
            reason: format!("dbus connect: {error}"),
        })
}

/// Parse a bus name string into a validated [`BusName`].
fn build_bus_name(name: &str) -> Result<BusName<'static>, DomainError> {
    BusName::try_from(name.to_string()).map_err(|error| DomainError::ActionFailed {
        reason: format!("invalid bus name {name}: {error}"),
    })
}

/// Seconds since the Unix epoch as a `u32`, or `0` on overflow or a clock set
/// before the epoch.
fn unix_timestamp_u32() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u32::try_from(duration.as_secs()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> SystemTrayProvider {
        SystemTrayProvider::new(tokio::runtime::Handle::current())
    }

    #[test]
    fn dormant_exit_stops_the_loop() {
        let result = Ok(HostExit::Dormant);
        assert!(matches!(next_loop_action(&result), LoopAction::Stop));
    }

    #[test]
    fn disconnected_exit_retries() {
        let result = Ok(HostExit::Disconnected);
        assert!(matches!(next_loop_action(&result), LoopAction::Retry));
    }

    #[test]
    fn transport_error_retries() {
        let result = Err(quantum_dbus::DbusError::Transport("boom".to_string()));
        assert!(matches!(next_loop_action(&result), LoopAction::Retry));
    }

    #[tokio::test]
    async fn snapshot_of_fresh_provider_is_empty_state() {
        let provider = provider();
        let value = provider.snapshot().await.expect("snapshot present");
        let state: SystemTrayState =
            serde_json::from_value(value).expect("snapshot deserializes to SystemTrayState");
        assert!(state.items.is_empty());
    }

    #[tokio::test]
    async fn non_custom_action_is_unsupported() {
        let provider = provider();
        let action = Action::Custom {
            kind: "not_tray".to_string(),
            payload: serde_json::json!({ "command": "activate" }),
        };
        let error = provider.invoke(&action).await.expect_err("should error");
        assert!(matches!(error, DomainError::Unsupported(_)), "{error:?}");
    }

    #[tokio::test]
    async fn unknown_command_is_action_failed() {
        let provider = provider();
        let action = Action::Custom {
            kind: "system_tray".to_string(),
            payload: serde_json::json!({ "command": "explode" }),
        };
        let error = provider.invoke(&action).await.expect_err("should error");
        assert!(
            matches!(error, DomainError::ActionFailed { .. }),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn activate_missing_service_is_action_failed() {
        let provider = provider();
        let action = Action::Custom {
            kind: "system_tray".to_string(),
            payload: serde_json::json!({ "command": "activate" }),
        };
        let error = provider.invoke(&action).await.expect_err("should error");
        assert!(
            matches!(error, DomainError::ActionFailed { .. }),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn activate_unknown_service_is_action_failed_without_bus() {
        // The handle lookup fails before any DBus connection is opened, so
        // this passes with no session bus available.
        let provider = provider();
        let action = Action::Custom {
            kind: "system_tray".to_string(),
            payload: serde_json::json!({ "command": "activate", "service": "x" }),
        };
        let error = provider.invoke(&action).await.expect_err("should error");
        assert!(
            matches!(error, DomainError::ActionFailed { .. }),
            "{error:?}"
        );
    }
}
