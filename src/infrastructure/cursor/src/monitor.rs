//! Gated ~60Hz cursor-position polling loop that produces [`CursorPosition`]s.
//!
//! [`TokioCursorMonitor`] spawns a single background task at construction that
//! ticks roughly sixty times a second. The task is subscriber-gated: on each
//! tick, if the broadcast channel reports zero receivers it skips all work (no
//! Hyprland query) and waits for the next tick. This mirrors
//! [`quantum_processes::TokioProcessMonitor`] minus the sampler, so there is no
//! explicit start/stop plumbing. `watch()` hands out a fresh broadcast receiver
//! (raising `receiver_count()` above zero, which resumes polling); dropping the
//! returned stream lowers the count again, and `unwatch()` is a documented
//! no-op.

use std::sync::Arc;
use std::time::Duration;

use futures::stream::{BoxStream, StreamExt};
use tokio::runtime::Handle;
use tokio::sync::broadcast;

use quantum_domain::{CursorMonitor, CursorPosition, HyprlandClient};

/// Polls the pointer position on a ~60Hz cadence while at least one subscriber
/// is listening, publishing each [`CursorPosition`] on a broadcast channel.
pub struct TokioCursorMonitor {
    sender: broadcast::Sender<CursorPosition>,
}

const CURSORPOS_COMMAND: &str = "j/cursorpos";

impl TokioCursorMonitor {
    /// Construct the monitor and spawn its poll loop on `runtime`. The loop
    /// idles (skips polling) until `watch()` produces a subscriber.
    pub fn new(runtime: Handle, hyprland: Arc<dyn HyprlandClient>) -> Self {
        let (sender, _receiver) = broadcast::channel::<CursorPosition>(16);
        runtime.spawn(run_loop(hyprland, sender.clone()));
        Self { sender }
    }
}

/// Parse the Hyprland `j/cursorpos` JSON reply into a [`CursorPosition`].
pub(crate) fn parse_cursorpos(reply: &str) -> Option<CursorPosition> {
    #[derive(serde::Deserialize)]
    struct Raw {
        x: i32,
        y: i32,
    }
    let raw: Raw = serde_json::from_str(reply.trim()).ok()?;
    Some(CursorPosition { x: raw.x, y: raw.y })
}

/// The subscriber-gated poll loop. Runs until the runtime shuts down.
async fn run_loop(hyprland: Arc<dyn HyprlandClient>, sender: broadcast::Sender<CursorPosition>) {
    let mut interval = tokio::time::interval(Duration::from_millis(16));
    // Skip missed ticks rather than bursting to catch up: after a system
    // suspend/resume the monotonic clock jumps, and `Skip` re-anchors the next
    // deadline to the first period boundary after "now" so resume yields a
    // single catch-up tick, never a burst or spin (tokio#7883).
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;

        // Gate: with no subscriber there is nothing to feed, so skip the whole
        // tick and issue zero Hyprland commands. `receiver_count()` goes
        // positive again the moment `watch()` hands out a receiver, and polling
        // resumes on the next tick.
        if sender.receiver_count() == 0 {
            continue;
        }

        match hyprland.command(CURSORPOS_COMMAND).await {
            Ok(reply) => match parse_cursorpos(&reply) {
                Some(position) => {
                    // Ignore the send error: it only means every receiver
                    // dropped between the gate check and here, which is harmless.
                    let _ = sender.send(position);
                }
                None => tracing::warn!("cursorpos parse failed: {reply}"),
            },
            Err(error) => tracing::warn!("cursorpos read failed, skipping tick: {error}"),
        }
    }
}

impl CursorMonitor for TokioCursorMonitor {
    fn watch(&self) -> BoxStream<'static, CursorPosition> {
        let receiver = self.sender.subscribe();
        tokio_stream::wrappers::BroadcastStream::new(receiver)
            // Drop lagged-receiver errors: a slow watcher that misses positions
            // simply resumes at the next successful one.
            .filter_map(|result| async move { result.ok() })
            .boxed()
    }

    fn unwatch(&self) {
        // No-op: receivers drop themselves when the caller drops the stream
        // returned by `watch()`. The poll loop idles automatically once
        // `receiver_count()` reaches zero, so there is no registration to
        // release here.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use quantum_domain::DomainError;

    /// Records how many times `command()` is called so a test can prove the
    /// loop idles when no one is subscribed. Always returns a valid cursorpos
    /// reply.
    struct CountingHyprland {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl HyprlandClient for CountingHyprland {
        async fn command(&self, _cmd: &str) -> Result<String, DomainError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok("{\"x\": 5, \"y\": 9}".to_string())
        }
    }

    // Acceptance criterion 3 and 8(a): no subscriber means zero Hyprland
    // commands; after `watch()` the loop polls and delivers a position.
    #[tokio::test(start_paused = true)]
    async fn gating_idles_without_subscribers_then_polls_after_watch() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hyprland = Arc::new(CountingHyprland {
            calls: Arc::clone(&calls),
        });
        let monitor = TokioCursorMonitor::new(Handle::current(), hyprland);

        // Several ticks pass with no subscriber; no command must be issued.
        for _ in 0..3 {
            tokio::time::advance(Duration::from_millis(16)).await;
            tokio::task::yield_now().await;
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "loop must issue zero Hyprland commands without subscribers"
        );

        // Subscribe, then let one tick fire.
        let mut stream = monitor.watch();
        tokio::time::advance(Duration::from_millis(16)).await;
        let position = stream.next().await.expect("position after watch");

        assert_eq!(position, CursorPosition { x: 5, y: 9 });
        assert!(
            calls.load(Ordering::SeqCst) >= 1,
            "loop must poll once a subscriber exists"
        );
    }

    // Acceptance criterion 8(b): the pure parser accepts valid JSON and rejects
    // garbage.
    #[test]
    fn parse_cursorpos_parses_valid_and_rejects_garbage() {
        assert_eq!(
            parse_cursorpos("{\"x\": 5, \"y\": 9}"),
            Some(CursorPosition { x: 5, y: 9 })
        );
        assert_eq!(parse_cursorpos("not json"), None);
    }
}
