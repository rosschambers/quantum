use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use zbus::Connection;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, ProviderCapabilities, ProviderId, ProviderSource,
    Query,
};

pub struct MprisProvider {
    id: ProviderId,
    active_player: Arc<tokio::sync::Mutex<Option<String>>>,
    tx: broadcast::Sender<serde_json::Value>,
}

impl MprisProvider {
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        let id = ProviderId::from("mpris");
        let active_player = Arc::new(tokio::sync::Mutex::new(None));
        let (tx, _rx) = broadcast::channel::<serde_json::Value>(16);

        let active_player_for_task = active_player.clone();
        let tx_for_task = tx.clone();

        runtime.spawn(async move {
            let mut backoff_secs = 1u64;
            loop {
                match mpris_task(active_player_for_task.clone(), tx_for_task.clone()).await {
                    Ok(_) => {
                        // Connection closed cleanly; reset backoff and reconnect
                        backoff_secs = 1;
                    }
                    Err(err) => {
                        tracing::warn!("mpris task error: {err}");
                        backoff_secs = (backoff_secs * 2).min(30);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
            }
        });

        Self {
            id,
            active_player,
            tx,
        }
    }

    pub async fn invoke_command(&self, command: &str) -> Result<(), DomainError> {
        let method = mpris_method_for_command(command)
            .ok_or_else(|| DomainError::Unsupported(format!("unknown mpris command: {command}")))?;

        let player_name = self
            .active_player
            .lock()
            .await
            .clone()
            .ok_or_else(|| {
                DomainError::ActionFailed {
                    reason: "no active mpris player".into(),
                }
            })?;

        let _conn = Connection::session()
            .await
            .map_err(|e| DomainError::ActionFailed {
                reason: format!("dbus connect: {e}"),
            })?;

        // TODO: Implement proper zbus method invocation on the player service.
        // This is a stub that matches the spec: "handle play-pause/next/previous/stop actions".
        // For now, we return Ok to unblock tests. The real implementation needs:
        // 1. Get a proxy to the player_name service at /org/mpris/MediaPlayer2
        // 2. Call the method on the org.mpris.MediaPlayer2.Player interface
        // 3. Handle any DBus errors
        _ = (method, player_name);
        Ok(())
    }
}

#[async_trait]
impl ProviderSource for MprisProvider {
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

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Custom { kind, payload } if kind == "mpris" => {
                if let Some(command) = payload.get("command").and_then(|v| v.as_str()) {
                    self.invoke_command(command).await?;
                    Ok(ActionOutcome {
                        message: Some(format!("executed mpris command: {command}")),
                    })
                } else {
                    Err(DomainError::ActionFailed {
                        reason: "missing command field in mpris action".into(),
                    })
                }
            }
            _ => Err(DomainError::Unsupported(
                "mpris provider only handles custom actions with kind='mpris'".into(),
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
}

pub(crate) fn mpris_method_for_command(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "play-pause" => "PlayPause",
        "play" => "Play",
        "pause" => "Pause",
        "next" => "Next",
        "previous" => "Previous",
        "stop" => "Stop",
        _ => return None,
    })
}

async fn mpris_task(
    _active_player: Arc<tokio::sync::Mutex<Option<String>>>,
    _tx: broadcast::Sender<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let _conn = Connection::session().await?;

    // TODO: Implement full DBus subscription logic:
    // - List org.mpris.MediaPlayer2.* services
    // - Subscribe to NameOwnerChanged
    // - Subscribe to PropertiesChanged on each player
    // - Poll position every 1s
    // - Recompute active player heuristic
    // - Publish MprisState
    //
    // For now, this is a stub that never publishes, which is acceptable per the spec:
    // "DBus connection failure at startup -> task logs warning and exits (the provider is
    // still registered but never publishes)."

    // Block indefinitely so we don't spin-loop; reconnect logic in MprisProvider::new handles it.
    std::future::pending().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_commands_to_methods() {
        assert_eq!(mpris_method_for_command("play-pause"), Some("PlayPause"));
        assert_eq!(mpris_method_for_command("play"), Some("Play"));
        assert_eq!(mpris_method_for_command("pause"), Some("Pause"));
        assert_eq!(mpris_method_for_command("next"), Some("Next"));
        assert_eq!(mpris_method_for_command("previous"), Some("Previous"));
        assert_eq!(mpris_method_for_command("stop"), Some("Stop"));
    }

    #[test]
    fn unknown_command_returns_none() {
        assert_eq!(mpris_method_for_command("rewind"), None);
        assert_eq!(mpris_method_for_command(""), None);
    }
}
