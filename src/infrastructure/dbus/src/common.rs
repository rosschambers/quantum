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

use crate::error::DbusError;

/// Closure type for building a DTO from a live connection.
///
/// Takes a borrowed connection so the builder can hop to other paths on
/// the same bus (BlueZ's ObjectManager needs this). Returns
/// `DbusError` so transport errors surface for backoff.
pub type BuildFn<S> =
    Box<dyn for<'a> Fn(&'a Connection) -> BoxFuture<'a, Result<S, DbusError>> + Send + Sync>;

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
            // Open a PropertiesProxy for the target path.
            let props_proxy = match open_properties_proxy(&conn, service, path).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(
                        service,
                        path,
                        error = %e,
                        "open_properties_proxy failed, backing off"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                    continue;
                }
            };

            // Initial state.
            match build(&conn).await {
                Ok(state) => match serde_json::to_value(&state) {
                    Ok(v) => {
                        if last_emitted.as_ref() != Some(&v) {
                            last_emitted = Some(v.clone());
                            yield v;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(service, path, error = %e, "initial state serialization failed");
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(max_backoff);
                        continue;
                    }
                },
                Err(e) => {
                    tracing::warn!(service, path, error = %e, "initial state build failed, backing off");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                    continue;
                }
            }

            // Subscribe to PropertiesChanged.
            let mut changes = match props_proxy.receive_properties_changed().await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(service, path, error = %e, "receive_properties_changed failed");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                    continue;
                }
            };

            // Reset backoff on a successful subscription.
            backoff = Duration::from_secs(1);

            // Stream changes. Yield each rebuilt DTO directly, dedupe by
            // PartialEq on the serialized JSON.
            let mut stream_ended_cleanly = true;
            while let Some(signal) = changes.next().await {
                match signal.args() {
                    Ok(args) => {
                        if args.interface_name().as_str() != interface {
                            continue;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(service, path, error = %e, "malformed PropertiesChanged signal args");
                        continue;
                    }
                }
                match build(&conn).await {
                    Ok(state) => match serde_json::to_value(&state) {
                        Ok(v) => {
                            if last_emitted.as_ref() != Some(&v) {
                                last_emitted = Some(v.clone());
                                yield v;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(service, path, error = %e, "state serialization failed");
                            stream_ended_cleanly = false;
                            break;
                        }
                    },
                    Err(e) => {
                        tracing::warn!(service, path, error = %e, "state build failed during signal loop");
                        stream_ended_cleanly = false;
                        break;
                    }
                }
            }

            if !stream_ended_cleanly {
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
            // Either the signal stream ended (proxy dropped) or we broke
            // out on error. Either way, loop back and re-open the proxy.
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
