use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, MonitorActiveWindowState, ProviderCapabilities,
    ProviderId, ProviderSource, Query,
};

use crate::hyprland::client::{HyprlandEvent, HyprlandSocketClient};

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
        HyprlandEvent::Unknown(_) => {}
    }
}

/// Fetch the currently-connected monitors from Hyprland via
/// `hyprctl monitors -j`. Factored out so the constructor's seed task can
/// log a single warning if it fails. If hyprctl is missing or returns
/// malformed JSON we return an error and let the caller fall back to the
/// event stream to populate the map.
async fn fetch_initial_monitors() -> Result<Vec<MonitorSeed>, crate::error::InfrastructureError> {
    let output = tokio::process::Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .await
        .map_err(|e| crate::error::InfrastructureError::Io(e.to_string()))?;
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let arr = json
        .as_array()
        .ok_or(crate::error::InfrastructureError::HyprlandUnreachable)?;
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
        let state_for_seed = state.clone();
        let tx_for_seed = tx.clone();
        runtime.spawn(async move {
            match fetch_initial_monitors().await {
                Ok(seeds) => {
                    let payload = {
                        let mut guard = state_for_seed.lock().unwrap();
                        for seed in seeds {
                            guard.monitors.entry(seed.name.clone()).or_default();
                            if seed.focused {
                                guard.focused_monitor = Some(seed.name);
                            }
                        }
                        serde_json::to_value(&*guard).unwrap_or(serde_json::Value::Null)
                    };
                    let _ = tx_for_seed.send(payload);
                }
                Err(e) => {
                    tracing::warn!("hyprctl monitor seed failed: {e:?}; relying on event stream");
                }
            }
        });

        let state_for_task = state.clone();
        let tx_for_task = tx.clone();
        runtime.spawn(async move {
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
                        serde_json::to_value(&*guard).unwrap_or(serde_json::Value::Null)
                    };
                    let _ = tx_for_task.send(payload);
                }
                tracing::warn!("hyprland event stream ended; reconnecting");
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(30);
            }
        });

        Self { id, state, tx }
    }
}

#[async_trait]
impl ProviderSource for HyprlandActiveWindowProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            searchable: false,
            streamable: true,
        }
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
}
