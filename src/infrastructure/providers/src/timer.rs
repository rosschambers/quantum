//! Streaming timer provider: broadcasts timer-store snapshots to subscribers.

use std::sync::RwLock;

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, ProviderId, ProviderSource, Query, TimerBroadcast,
    TimerStoreData,
};

/// Streaming provider that holds a broadcast channel of timer-store snapshot
/// envelopes. The dispatcher drives state changes through `publish`, which
/// stores the latest envelope and fans it out to every live subscriber.
pub struct TimerProvider {
    id: ProviderId,
    tx: broadcast::Sender<serde_json::Value>,
    last: RwLock<Option<serde_json::Value>>,
}

impl TimerProvider {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel::<serde_json::Value>(16);
        Self {
            id: ProviderId::from("timer"),
            tx,
            last: RwLock::new(None),
        }
    }
}

impl Default for TimerProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProviderSource for TimerProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, _q: &Query) -> Result<Vec<Match>, DomainError> {
        Ok(vec![])
    }

    async fn invoke(&self, _action: &Action) -> Result<ActionOutcome, DomainError> {
        // Timer commands flow through dedicated dispatcher methods, not provider
        // actions, so there is nothing to do here.
        Ok(ActionOutcome { message: None })
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

impl TimerBroadcast for TimerProvider {
    fn publish(&self, data: &TimerStoreData) {
        let envelope = serde_json::json!({
            "change": "snapshot",
            "settings": data.settings,
            "timers": data.timers,
        });
        if let Ok(mut guard) = self.last.write() {
            *guard = Some(envelope.clone());
        }
        let _ = self.tx.send(envelope);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantum_domain::{
        NotifyConfig, ProviderSource, Timer, TimerBroadcast, TimerId, TimerKind, TimerStoreData,
        TimerStatus, VisualConfig,
    };

    fn sample_timer() -> Timer {
        Timer {
            id: TimerId::from("t1"),
            label: "Tea".to_string(),
            kind: TimerKind::OneShot { end_unix: 1700 },
            visual: VisualConfig::default(),
            notify: NotifyConfig::default(),
            status: TimerStatus::Active,
            scatter_pos: None,
        }
    }

    #[tokio::test]
    async fn publish_emits_snapshot_envelope() {
        let provider = TimerProvider::new();
        let mut stream = provider.subscribe().expect("subscribe");
        let mut data = TimerStoreData::default();
        data.timers.push(sample_timer());
        provider.publish(&data);
        let envelope = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            futures::StreamExt::next(&mut stream),
        )
        .await
        .expect("no timeout")
        .expect("an item");
        assert_eq!(envelope["change"], "snapshot");
        assert_eq!(envelope["timers"].as_array().expect("timers array").len(), 1);
    }
}
