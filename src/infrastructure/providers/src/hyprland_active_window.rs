use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::error::ProvidersError;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, MonitorActiveWindowState, ProviderId,
    ProviderSource, Query,
};

use quantum_hyprland::{HyprlandEvent, HyprlandSocketClient};

/// One monitor reported by `hyprctl monitors -j`. Used solely to seed the
/// per-monitor state map at startup; the event stream populates the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MonitorSeed {
    pub name: String,
    pub focused: bool,
}

/// Pure event-application function. Factored out of the spawned event-loop
/// task so it can be unit-tested without spinning up a Tokio runtime.
///
/// Mutates `state` in place per the contract documented in
/// `docs/plans/2026-06-01-multi-monitor-bar.md` Task A.3.
pub(crate) fn apply_event(state: &mut MonitorActiveWindowState, ev: HyprlandEvent) {
    match ev {
        HyprlandEvent::FocusedMon { monitor, .. } => {
            state.monitors.entry(monitor.clone()).or_default();
            state.focused_monitor = Some(monitor);
        }
        HyprlandEvent::MonitorAdded { monitor } => {
            state.monitors.entry(monitor).or_default();
        }
        HyprlandEvent::MonitorRemoved { monitor } => {
            state.monitors.remove(&monitor);
            if state.focused_monitor.as_deref() == Some(monitor.as_str()) {
                state.focused_monitor = None;
            }
        }
        HyprlandEvent::ActiveWindow { class, title } => {
            if let Some(name) = state.focused_monitor.clone() {
                let entry = state.monitors.entry(name).or_default();
                entry.class = class;
                entry.title = title;
            }
        }
        HyprlandEvent::Workspace { name } => {
            if let Some(monitor) = state.focused_monitor.clone() {
                let entry = state.monitors.entry(monitor).or_default();
                entry.workspace_name = name.clone();
                entry.workspace_id = name.parse().unwrap_or(0);
            }
        }
    }
}

/// Fetch the currently-connected monitors from Hyprland via
/// `hyprctl monitors -j`. Factored out so the constructor's seed task can
/// log a single warning if it fails. If hyprctl is missing or returns
/// malformed JSON we return an error and let the caller fall back to the
/// event stream to populate the map.
async fn fetch_initial_monitors() -> Result<Vec<MonitorSeed>, ProvidersError> {
    let output = tokio::process::Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .await
        .map_err(|e| ProvidersError::Spawn(e.to_string()))?;
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let arr = json
        .as_array()
        .ok_or_else(|| ProvidersError::Serde("hyprctl monitors -j: expected array".into()))?;
    Ok(arr
        .iter()
        .filter_map(|v| {
            let name = v["name"].as_str()?.to_string();
            let focused = v["focused"].as_bool().unwrap_or(false);
            Some(MonitorSeed { name, focused })
        })
        .collect())
}

pub struct HyprlandActiveWindowProvider {
    id: ProviderId,
    state: Arc<std::sync::Mutex<MonitorActiveWindowState>>,
    tx: broadcast::Sender<serde_json::Value>,
}

impl HyprlandActiveWindowProvider {
    pub fn new(client: Arc<HyprlandSocketClient>, runtime: tokio::runtime::Handle) -> Self {
        let id = ProviderId::from("hyprland.activewindow");
        let state = Arc::new(std::sync::Mutex::new(MonitorActiveWindowState::default()));
        let (tx, _rx) = broadcast::channel(16);

        // Seed asynchronously via `hyprctl monitors -j`. Both this task and
        // the event-loop task below acquire the same std Mutex, so they are
        // serialized. If a `focusedmon`/`monitoradded` event lands before
        // the seed completes, `entry().or_default()` here won't overwrite
        // anything; only `focused_monitor` can race, and that resolves on
        // the next `FocusedMon` event from the live stream.
        // Seed task is fire-and-forget: it updates `state` so that
        // `subscribe()` returns the seeded snapshot to any caller
        // that connects after the seed completes. We deliberately do
        // NOT broadcast the seed payload because no subscriber has
        // connected yet at construction time — the snapshot in
        // `subscribe()` covers late subscribers.
        let state_for_seed = state.clone();
        runtime.spawn(async move {
            match fetch_initial_monitors().await {
                Ok(seeds) => {
                    let mut guard = state_for_seed.lock().unwrap();
                    for seed in seeds {
                        guard.monitors.entry(seed.name.clone()).or_default();
                        if seed.focused {
                            guard.focused_monitor = Some(seed.name);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("hyprctl monitor seed failed: {e:?}; relying on event stream");
                }
            }
        });

        let state_for_task = state.clone();
        let tx_for_task = tx.clone();
        runtime.spawn(async move {
            // Last payload pushed onto the broadcast channel for this task.
            // Hyprland fires a burst of events around any window switch
            // (focus, title, workspace) and several of them collapse to the
            // same `MonitorActiveWindowState` once `apply_event` finishes.
            // The change-gate suppresses the redundant broadcasts.
            let mut last_published: Option<serde_json::Value> = None;
            // Last broadcast state, compared with `PartialEq` BEFORE
            // serializing. Title-change bursts frequently leave the
            // multi-monitor state unchanged; comparing the state directly
            // means the full `serde_json::to_value` over every monitor only
            // runs when a broadcast will actually be sent.
            let mut last_state: Option<MonitorActiveWindowState> = None;
            let mut backoff_secs = 1u64;
            loop {
                let stream = match client.subscribe_events() {
                    Ok(s) => s,
                    Err(err) => {
                        tracing::warn!("hyprland subscribe_events failed: {err}");
                        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                        backoff_secs = (backoff_secs * 2).min(30);
                        continue;
                    }
                };
                backoff_secs = 1;
                let mut stream = stream;
                while let Some(ev) = stream.next().await {
                    let payload = {
                        let mut guard = state_for_task.lock().unwrap();
                        apply_event(&mut guard, ev);
                        // Cheap equality check first: if the state matches the
                        // last broadcast, skip serialization entirely.
                        if last_state.as_ref() == Some(&*guard) {
                            continue;
                        }
                        let value =
                            serde_json::to_value(&*guard).unwrap_or(serde_json::Value::Null);
                        last_state = Some(guard.clone());
                        value
                    };
                    send_state_if_changed(&tx_for_task, &mut last_published, payload);
                }
                tracing::warn!("hyprland event stream ended; reconnecting");
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(30);
            }
        });

        Self { id, state, tx }
    }
}

/// Forward `candidate` on `tx` only when it differs from `last`.
///
/// Hyprland fires several events per window/workspace transition that may
/// all serialize to the same payload (for example a `focusedmon` to the
/// already-focused monitor). Without this filter every one of those events
/// would wake every subscriber.
pub(crate) fn send_state_if_changed(
    tx: &broadcast::Sender<serde_json::Value>,
    last: &mut Option<serde_json::Value>,
    candidate: serde_json::Value,
) {
    if last.as_ref() == Some(&candidate) {
        return;
    }
    let _ = tx.send(candidate.clone());
    *last = Some(candidate);
}

#[async_trait]
impl ProviderSource for HyprlandActiveWindowProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, _: &Query) -> Result<Vec<Match>, DomainError> {
        Ok(vec![])
    }

    async fn invoke(&self, _: &Action) -> Result<ActionOutcome, DomainError> {
        Err(DomainError::Unsupported(
            "hyprland.activewindow does not handle actions".into(),
        ))
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        // Emit current state immediately, then forward broadcast updates.
        let initial = {
            let state = self.state.lock().unwrap();
            serde_json::to_value(&*state).unwrap_or(serde_json::Value::Null)
        };
        let initial_stream = stream::iter(vec![initial]);
        let rx = self.tx.subscribe();
        let updates = BroadcastStream::new(rx).filter_map(|res| async move { res.ok() });
        Some(initial_stream.chain(updates).boxed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantum_domain::ActiveWindowState;

    fn state_with(monitor: &str, class: &str) -> MonitorActiveWindowState {
        let mut s = MonitorActiveWindowState::default();
        s.monitors.insert(
            monitor.into(),
            ActiveWindowState {
                class: class.into(),
                ..Default::default()
            },
        );
        s.focused_monitor = Some(monitor.into());
        s
    }

    #[test]
    fn focusedmon_swaps_focus_without_clearing_other_monitors() {
        let mut state = state_with("DP-1", "firefox");
        state
            .monitors
            .insert("DP-2".into(), ActiveWindowState::default());
        apply_event(
            &mut state,
            HyprlandEvent::FocusedMon {
                monitor: "DP-2".into(),
                workspace: "1".into(),
            },
        );
        assert_eq!(state.focused_monitor.as_deref(), Some("DP-2"));
        assert_eq!(state.monitors["DP-1"].class, "firefox");
    }

    #[test]
    fn monitoradded_creates_default_entry() {
        let mut state = MonitorActiveWindowState::default();
        apply_event(
            &mut state,
            HyprlandEvent::MonitorAdded {
                monitor: "HDMI-A-1".into(),
            },
        );
        assert!(state.monitors.contains_key("HDMI-A-1"));
    }

    #[test]
    fn monitoradded_is_idempotent() {
        let mut state = state_with("DP-1", "firefox");
        apply_event(
            &mut state,
            HyprlandEvent::MonitorAdded {
                monitor: "DP-1".into(),
            },
        );
        // Existing entry's class is preserved.
        assert_eq!(state.monitors["DP-1"].class, "firefox");
    }

    #[test]
    fn monitorremoved_clears_focus_if_it_was_focused() {
        let mut state = state_with("DP-1", "firefox");
        apply_event(
            &mut state,
            HyprlandEvent::MonitorRemoved {
                monitor: "DP-1".into(),
            },
        );
        assert!(!state.monitors.contains_key("DP-1"));
        assert_eq!(state.focused_monitor, None);
    }

    #[test]
    fn monitorremoved_leaves_focus_when_different_monitor_removed() {
        let mut state = state_with("DP-1", "firefox");
        state
            .monitors
            .insert("DP-2".into(), ActiveWindowState::default());
        apply_event(
            &mut state,
            HyprlandEvent::MonitorRemoved {
                monitor: "DP-2".into(),
            },
        );
        assert!(state.monitors.contains_key("DP-1"));
        assert_eq!(state.focused_monitor.as_deref(), Some("DP-1"));
    }

    #[test]
    fn activewindow_updates_focused_entry() {
        let mut state = state_with("DP-1", "firefox");
        apply_event(
            &mut state,
            HyprlandEvent::ActiveWindow {
                class: "ghostty".into(),
                title: "zsh".into(),
            },
        );
        assert_eq!(state.monitors["DP-1"].class, "ghostty");
        assert_eq!(state.monitors["DP-1"].title, "zsh");
    }

    #[test]
    fn activewindow_noop_when_no_focused_monitor() {
        let mut state = MonitorActiveWindowState::default();
        apply_event(
            &mut state,
            HyprlandEvent::ActiveWindow {
                class: "ghostty".into(),
                title: "zsh".into(),
            },
        );
        // Nothing inserted because focused_monitor is None.
        assert!(state.monitors.is_empty());
    }

    #[test]
    fn workspace_updates_focused_entry() {
        let mut state = state_with("DP-1", "firefox");
        apply_event(&mut state, HyprlandEvent::Workspace { name: "5".into() });
        assert_eq!(state.monitors["DP-1"].workspace_id, 5);
        assert_eq!(state.monitors["DP-1"].workspace_name, "5");
    }

    #[tokio::test]
    async fn send_state_if_changed_suppresses_duplicate() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let mut last: Option<serde_json::Value> = None;
        let value = serde_json::json!({"focused_monitor": "DP-1", "monitors": {}});

        send_state_if_changed(&tx, &mut last, value.clone());
        send_state_if_changed(&tx, &mut last, value.clone());

        assert_eq!(rx.try_recv().expect("first"), value);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn send_state_if_changed_forwards_distinct() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let mut last: Option<serde_json::Value> = None;
        let a = serde_json::json!({"focused_monitor": "DP-1"});
        let b = serde_json::json!({"focused_monitor": "DP-2"});

        send_state_if_changed(&tx, &mut last, a.clone());
        send_state_if_changed(&tx, &mut last, b.clone());

        assert_eq!(rx.try_recv().expect("first"), a);
        assert_eq!(rx.try_recv().expect("second"), b);
    }

    #[tokio::test]
    async fn two_events_collapsing_to_same_state_yield_one_broadcast() {
        // Simulates the situation where two Hyprland events both result in
        // the same `MonitorActiveWindowState` after `apply_event`. The
        // provider's change-gate must collapse these into a single
        // broadcast — that's the whole point of Fix 6.
        let (tx, mut rx) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let mut last: Option<serde_json::Value> = None;

        let mut state = state_with("DP-1", "firefox");
        // First event: focused monitor stays DP-1 (no-op for serialization).
        apply_event(
            &mut state,
            HyprlandEvent::FocusedMon {
                monitor: "DP-1".into(),
                workspace: "1".into(),
            },
        );
        let payload_a = serde_json::to_value(&state).expect("serialize");
        send_state_if_changed(&tx, &mut last, payload_a.clone());

        // Second event: another no-op FocusedMon for the same monitor.
        apply_event(
            &mut state,
            HyprlandEvent::FocusedMon {
                monitor: "DP-1".into(),
                workspace: "1".into(),
            },
        );
        let payload_b = serde_json::to_value(&state).expect("serialize");
        send_state_if_changed(&tx, &mut last, payload_b.clone());

        // Exactly one broadcast.
        assert_eq!(rx.try_recv().expect("first event"), payload_a);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
}
