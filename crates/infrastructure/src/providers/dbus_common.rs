//! Shared helpers for DBus-backed providers.
//!
//! Five of the tray providers share the same loop shape: open a session
//! or system bus connection, fetch an initial state, subscribe to
//! `PropertiesChanged` (or `InterfacesAdded` / `InterfacesRemoved` for
//! ObjectManager-shaped services), rebuild the DTO on each signal,
//! dedupe identical consecutive states, and publish on a
//! `BoxStream<serde_json::Value>`. This module factors that out.
//!
//! The helper is intentionally bus-agnostic at the call site: providers
//! pass a `BuildFn` closure that takes a `&zbus::Connection` and returns
//! their fully-constructed DTO. Reconnect-on-error uses exponential
//! backoff capped at 30 seconds, matching the `hyprland_active_window`
//! provider's pattern.

use std::time::Duration;

use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};
use serde::Serialize;
use zbus::Connection;

use crate::error::InfrastructureError;

/// Closure type for building a DTO from a live connection.
///
/// Takes a borrowed connection so the builder can hop to other paths on
/// the same bus (BlueZ's ObjectManager needs this). Returns
/// `InfrastructureError` so transport errors surface for backoff.
pub type BuildFn<S> = Box<
    dyn for<'a> Fn(&'a Connection) -> BoxFuture<'a, Result<S, InfrastructureError>> + Send + Sync,
>;

/// Check whether a DBus service is currently owned on the given bus.
///
/// Returns false on transport error rather than panicking — callers
/// should treat that the same as "service missing", because the user-
/// observable outcome is identical: the provider publishes an
/// unavailable state and waits for the service to appear.
pub async fn service_available(conn: &Connection, service: &str) -> bool {
    let proxy = match zbus::fdo::DBusProxy::new(conn).await {
        Ok(p) => p,
        Err(_) => return false,
    };
    let bus_name = match zbus::names::BusName::try_from(service) {
        Ok(n) => n,
        Err(_) => return false,
    };
    proxy.name_has_owner(bus_name).await.unwrap_or(false)
}

/// A stream that yields the default value of `S` once, then stays
/// pending forever.
///
/// Used by providers whose backing service was missing at startup. The
/// stream stays alive (does not terminate) so the subscribe use case
/// keeps the channel open; if the service appears later, the provider
/// is expected to restart its real stream via a `NameOwnerChanged`
/// watcher running elsewhere.
pub fn unavailable_stream<S: Default + Serialize + Send + 'static>(
) -> BoxStream<'static, serde_json::Value> {
    let first = serde_json::to_value(S::default()).unwrap_or(serde_json::Value::Null);
    let initial = futures::stream::iter(std::iter::once(first));
    let pending: futures::stream::Pending<serde_json::Value> = futures::stream::pending();
    Box::pin(initial.chain(pending))
}

/// Run a property-subscription loop on `(service, path, interface)`,
/// yielding fresh DTOs via the supplied `build` closure on every
/// `PropertiesChanged` signal.
///
/// The initial state is yielded before the loop blocks on signals.
/// Identical consecutive states are deduped via `serde_json::Value`
/// equality. On any error the loop sleeps with exponential backoff
/// (1s, 2s, 4s, ..., 30s cap) and retries.
///
/// This function never returns under normal conditions. Drop the
/// resulting stream to stop it.
pub fn property_subscription_stream<S>(
    conn: Connection,
    service: &'static str,
    path: &'static str,
    interface: &'static str,
    build: BuildFn<S>,
) -> BoxStream<'static, serde_json::Value>
where
    S: Serialize + Send + Sync + 'static,
{
    Box::pin(async_stream::stream! {
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(30);
        let mut last_emitted: Option<serde_json::Value> = None;

        loop {
            match run_one_session(&conn, service, path, interface, &build, &mut last_emitted).await {
                Ok(emissions) => {
                    for v in emissions {
                        yield v;
                    }
                    // Successful sub ended (proxy dropped) — reconnect from scratch.
                    backoff = Duration::from_secs(1);
                }
                Err((emissions, _err)) => {
                    for v in emissions {
                        yield v;
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                }
            }
        }
    })
}

async fn open_properties_proxy<'a>(
    conn: &'a Connection,
    service: &str,
    path: &str,
) -> zbus::Result<zbus::fdo::PropertiesProxy<'a>> {
    zbus::fdo::PropertiesProxy::builder(conn)
        .destination(service.to_string())?
        .path(path.to_string())?
        .interface("org.freedesktop.DBus.Properties")?
        .build()
        .await
}

/// One connect + initial fetch + signal loop. Returns the list of
/// emissions to yield in either branch so callers can flush them
/// before retrying.
async fn run_one_session<S>(
    conn: &Connection,
    service: &str,
    path: &str,
    interface: &str,
    build: &BuildFn<S>,
    last_emitted: &mut Option<serde_json::Value>,
) -> Result<Vec<serde_json::Value>, (Vec<serde_json::Value>, InfrastructureError)>
where
    S: Serialize + Send + Sync + 'static,
{
    let mut emissions = Vec::new();

    // Open a PropertiesProxy for the target path.
    let props_proxy = match open_properties_proxy(conn, service, path).await {
        Ok(p) => p,
        Err(e) => return Err((emissions, InfrastructureError::DbusTransport(e.to_string()))),
    };

    // Yield initial state.
    match build(conn).await {
        Ok(state) => {
            let v = serde_json::to_value(&state)
                .map_err(|e| (emissions.clone(), InfrastructureError::from(e)))?;
            if last_emitted.as_ref() != Some(&v) {
                *last_emitted = Some(v.clone());
                emissions.push(v);
            }
        }
        Err(e) => return Err((emissions, e)),
    }

    // Subscribe.
    let mut changes = match props_proxy.receive_properties_changed().await {
        Ok(s) => s,
        Err(e) => return Err((emissions, InfrastructureError::DbusTransport(e.to_string()))),
    };

    let _ = interface; // reserved for future per-interface filtering

    while let Some(_signal) = changes.next().await {
        match build(conn).await {
            Ok(state) => {
                let v = match serde_json::to_value(&state) {
                    Ok(v) => v,
                    Err(e) => return Err((emissions, InfrastructureError::from(e))),
                };
                if last_emitted.as_ref() != Some(&v) {
                    *last_emitted = Some(v.clone());
                    emissions.push(v);
                }
            }
            Err(e) => return Err((emissions, e)),
        }
    }

    Ok(emissions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use quantum_domain::PowerState;
    use std::time::Duration;

    #[tokio::test]
    #[ignore = "requires session bus"]
    async fn dbus_itself_is_available() {
        let conn = Connection::session().await.expect("session bus");
        assert!(service_available(&conn, "org.freedesktop.DBus").await);
    }

    #[tokio::test]
    #[ignore = "requires session bus"]
    async fn nonexistent_service_is_unavailable() {
        let conn = Connection::session().await.expect("session bus");
        assert!(!service_available(&conn, "com.does.not.Exist").await);
    }

    #[tokio::test]
    async fn unavailable_stream_yields_one_default_then_pends() {
        let mut s = unavailable_stream::<PowerState>();
        let v = tokio::time::timeout(Duration::from_millis(100), s.next())
            .await
            .expect("first item within 100ms")
            .expect("item is Some");
        assert_eq!(v["available"], false);
        // Second poll must time out (stream stays pending).
        let next = tokio::time::timeout(Duration::from_millis(100), s.next()).await;
        assert!(next.is_err(), "stream should stay pending after first item");
    }
}
