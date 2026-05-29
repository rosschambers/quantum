use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use quantum_domain::{
    Action, ActionOutcome, ActiveWindowState, DomainError, Match, ProviderCapabilities, ProviderId,
    ProviderSource, Query,
};

use crate::hyprland::client::{HyprlandEvent, HyprlandSocketClient};

pub struct HyprlandActiveWindowProvider {
    id: ProviderId,
    state: Arc<std::sync::Mutex<ActiveWindowState>>,
    tx: broadcast::Sender<serde_json::Value>,
}

impl HyprlandActiveWindowProvider {
    pub fn new(client: Arc<HyprlandSocketClient>, runtime: tokio::runtime::Handle) -> Self {
        let id = ProviderId::from("hyprland.activewindow");
        let state = Arc::new(std::sync::Mutex::new(ActiveWindowState {
            title: String::new(),
            class: String::new(),
            workspace_id: 0,
            workspace_name: String::new(),
        }));
        let (tx, _rx) = broadcast::channel(16);
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
                    let mut guard = state_for_task.lock().unwrap();
                    match ev {
                        HyprlandEvent::ActiveWindow { class, title } => {
                            guard.class = class;
                            guard.title = title;
                        }
                        HyprlandEvent::Workspace { name } => {
                            guard.workspace_name = name.clone();
                            guard.workspace_id = name.parse().unwrap_or(0);
                        }
                        HyprlandEvent::Unknown(_) => continue,
                    }
                    let payload = serde_json::to_value(&*guard).unwrap_or(serde_json::Value::Null);
                    drop(guard);
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
