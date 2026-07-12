//! Gated one-hertz process sampling loop that produces [`ProcessSnapshot`]s.
//!
//! [`TokioProcessMonitor`] spawns a single background task at construction that
//! ticks once a second. The task is subscriber-gated: on each tick, if the
//! broadcast channel reports zero receivers it skips all work (no sampling, no
//! Hyprland query) and waits for the next tick. This mirrors the idle-skip in
//! `proc_stats.rs`, so there is no explicit start/stop plumbing. `watch()`
//! hands out a fresh broadcast receiver (raising `receiver_count()` above
//! zero, which resumes sampling); dropping the returned stream lowers the
//! count again, and `unwatch()` is a documented no-op.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use tokio::runtime::Handle;
use tokio::sync::broadcast;

use quantum_domain::{
    build_forest, GlobalStats, HyprlandClient, ProcessMonitor, ProcessSnapshot, ProcessesError,
    RawProcess,
};

use crate::sampler::ProcfsSampler;
use crate::windows::window_pid_map;

/// Per-tick source of raw process samples and global statistics. Abstracting
/// the sampler behind this trait lets the monitor be driven by a fake in tests
/// while [`ProcfsSampler`] provides the live `/proc` implementation. `sample`
/// takes `&mut self` because the concrete sampler retains the previous tick's
/// counters to compute per-interval rates.
#[async_trait]
pub trait ProcessSampleSource: Send + Sync {
    async fn sample(&mut self) -> Result<(Vec<RawProcess>, GlobalStats), ProcessesError>;
}

#[async_trait]
impl ProcessSampleSource for ProcfsSampler {
    async fn sample(&mut self) -> Result<(Vec<RawProcess>, GlobalStats), ProcessesError> {
        // Explicit path resolves to the inherent `ProcfsSampler::sample`, not
        // this trait method, so there is no recursion.
        ProcfsSampler::sample(self).await
    }
}

/// Watches the machine's processes on a one-second cadence while at least one
/// subscriber is listening, publishing each [`ProcessSnapshot`] on a broadcast
/// channel and caching the freshest snapshot for one-shot readers.
pub struct TokioProcessMonitor {
    sender: broadcast::Sender<ProcessSnapshot>,
    latest: Arc<Mutex<Option<ProcessSnapshot>>>,
}

impl TokioProcessMonitor {
    /// Construct the monitor and spawn its sampling loop on `runtime`. The loop
    /// idles (skips sampling) until `watch()` produces a subscriber.
    pub fn new(
        runtime: Handle,
        sampler: Box<dyn ProcessSampleSource>,
        hyprland: Arc<dyn HyprlandClient>,
    ) -> Self {
        let (sender, _receiver) = broadcast::channel::<ProcessSnapshot>(16);
        let latest: Arc<Mutex<Option<ProcessSnapshot>>> = Arc::new(Mutex::new(None));
        let sender_for_task = sender.clone();
        let latest_for_task = Arc::clone(&latest);
        runtime.spawn(run_loop(
            sampler,
            hyprland,
            sender_for_task,
            latest_for_task,
        ));
        Self { sender, latest }
    }

    /// Shared handle to the freshest snapshot. Task 8's killer holds this to
    /// resolve a subtree against the most recent sample without re-sampling.
    pub fn latest(&self) -> Arc<Mutex<Option<ProcessSnapshot>>> {
        Arc::clone(&self.latest)
    }

    /// A clone of the most recent snapshot, or `None` before the first sample.
    pub fn latest_snapshot(&self) -> Option<ProcessSnapshot> {
        self.latest.lock().ok().and_then(|guard| guard.clone())
    }
}

/// The subscriber-gated sampling loop. Runs until the runtime shuts down.
async fn run_loop(
    mut sampler: Box<dyn ProcessSampleSource>,
    hyprland: Arc<dyn HyprlandClient>,
    sender: broadcast::Sender<ProcessSnapshot>,
    latest: Arc<Mutex<Option<ProcessSnapshot>>>,
) {
    // The monitor's own process (and its ancestors) must never be killed, so it
    // is the protected pid passed to `build_forest`.
    let protected_pid = std::process::id() as i32;
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;

        // Gate: with no subscriber there is nothing to feed, so skip the whole
        // tick. `receiver_count()` goes positive again the moment `watch()`
        // hands out a receiver, and sampling resumes on the next tick.
        if sender.receiver_count() == 0 {
            continue;
        }

        let (raw, global) = match sampler.sample().await {
            Ok(pair) => pair,
            Err(error) => {
                tracing::warn!("process sampling failed, skipping tick: {error}");
                continue;
            }
        };

        // A failed Hyprland query must not kill the tick: fall back to an empty
        // window map so the tree still renders (everything under background).
        let clients_json = match hyprland.command("j/clients").await {
            Ok(json) => json,
            Err(error) => {
                tracing::warn!("hyprland j/clients failed, using empty window map: {error}");
                String::new()
            }
        };

        let windows = window_pid_map(&clients_json);
        let (apps, background) = build_forest(raw, &windows, protected_pid);
        let snapshot = ProcessSnapshot {
            global,
            apps,
            background,
        };

        // Cache the freshest snapshot for one-shot readers, holding the lock
        // only long enough to swap it in (never across an await).
        if let Ok(mut guard) = latest.lock() {
            *guard = Some(snapshot.clone());
        }

        // Ignore the send error: it only means every receiver dropped between
        // the gate check and here, which is harmless.
        let _ = sender.send(snapshot);
    }
}

impl ProcessMonitor for TokioProcessMonitor {
    fn watch(&self) -> BoxStream<'static, ProcessSnapshot> {
        let receiver = self.sender.subscribe();
        tokio_stream::wrappers::BroadcastStream::new(receiver)
            // Drop lagged-receiver errors: a slow watcher that misses snapshots
            // simply resumes at the next successful one.
            .filter_map(|result| async move { result.ok() })
            .boxed()
    }

    fn unwatch(&self) {
        // No-op: receivers drop themselves when the caller drops the stream
        // returned by `watch()`. The sampling loop idles automatically once
        // `receiver_count()` reaches zero, so there is no registration to
        // release here.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use quantum_domain::{DomainError, ProcessNode};

    const WINDOW_PID: i32 = 1000;

    /// The fixed flat sample every fake tick returns: a windowed process
    /// (pid 1000) with one child (pid 2000). `RawProcess` is not `Clone`, so
    /// this rebuilds it on each call.
    fn fake_processes() -> Vec<RawProcess> {
        vec![
            RawProcess {
                pid: WINDOW_PID,
                ppid: 1,
                name: "firefox".to_string(),
                cpu_percent: 5.0,
                mem_bytes: 500,
            },
            RawProcess {
                pid: 2000,
                ppid: WINDOW_PID,
                name: "firefox-tab".to_string(),
                cpu_percent: 2.0,
                mem_bytes: 100,
            },
        ]
    }

    fn fake_global() -> GlobalStats {
        GlobalStats {
            cpu_percent: 7.0,
            mem_used_bytes: 1_000,
            mem_total_bytes: 2_000,
            net_rx_bytes_per_second: 0,
            net_tx_bytes_per_second: 0,
        }
    }

    fn fake_clients_json() -> String {
        r#"[{"pid": 1000, "class": "firefox", "title": "Firefox"}]"#.to_string()
    }

    /// Records how many times `sample()` is called so a test can prove the loop
    /// idles when no one is subscribed.
    struct FakeSampler {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ProcessSampleSource for FakeSampler {
        async fn sample(&mut self) -> Result<(Vec<RawProcess>, GlobalStats), ProcessesError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok((fake_processes(), fake_global()))
        }
    }

    /// Returns canned `j/clients` JSON, or an error to exercise the fallback.
    struct FakeHyprland {
        response: Result<String, ()>,
    }

    #[async_trait]
    impl HyprlandClient for FakeHyprland {
        async fn command(&self, _cmd: &str) -> Result<String, DomainError> {
            match &self.response {
                Ok(json) => Ok(json.clone()),
                Err(()) => Err(DomainError::ActionFailed {
                    reason: "hyprland unavailable".to_string(),
                }),
            }
        }
    }

    /// Depth-first search for a pid anywhere in a forest.
    fn contains_pid(roots: &[ProcessNode], pid: i32) -> bool {
        roots
            .iter()
            .any(|node| node.pid == pid || contains_pid(&node.children, pid))
    }

    // Acceptance criterion 2: no subscriber means no sampling; after `watch()`
    // the loop samples and delivers a snapshot with the windowed pid grouped
    // under apps.
    #[tokio::test(start_paused = true)]
    async fn gating_idles_without_subscribers_then_samples_after_watch() {
        let calls = Arc::new(AtomicUsize::new(0));
        let sampler = Box::new(FakeSampler {
            calls: Arc::clone(&calls),
        });
        let hyprland = Arc::new(FakeHyprland {
            response: Ok(fake_clients_json()),
        });
        let monitor = TokioProcessMonitor::new(Handle::current(), sampler, hyprland);

        // Several ticks pass with no subscriber; the sampler must stay untouched.
        for _ in 0..3 {
            tokio::time::advance(Duration::from_secs(1)).await;
            tokio::task::yield_now().await;
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "sampler must not run without subscribers"
        );

        // Subscribe, then let one tick fire.
        let mut stream = monitor.watch();
        tokio::time::advance(Duration::from_secs(1)).await;
        let snapshot = stream.next().await.expect("snapshot after watch");

        assert!(
            calls.load(Ordering::SeqCst) >= 1,
            "sampler must run once a subscriber exists"
        );
        assert!(
            contains_pid(&snapshot.apps, WINDOW_PID),
            "windowed pid must be grouped under apps"
        );
        assert!(
            contains_pid(&snapshot.apps, 2000),
            "the window's child must nest under the app"
        );
        assert!(snapshot.background.is_empty());
    }

    // Acceptance criterion 3: a Hyprland error yields a snapshot with an empty
    // window map, so everything lands under background and nothing panics.
    #[tokio::test(start_paused = true)]
    async fn hyprland_error_puts_everything_under_background() {
        let calls = Arc::new(AtomicUsize::new(0));
        let sampler = Box::new(FakeSampler {
            calls: Arc::clone(&calls),
        });
        let hyprland = Arc::new(FakeHyprland { response: Err(()) });
        let monitor = TokioProcessMonitor::new(Handle::current(), sampler, hyprland);

        let mut stream = monitor.watch();
        tokio::time::advance(Duration::from_secs(1)).await;
        let snapshot = stream
            .next()
            .await
            .expect("snapshot despite hyprland error");

        assert!(snapshot.apps.is_empty(), "no window map means no apps");
        assert!(
            contains_pid(&snapshot.background, WINDOW_PID),
            "the un-windowed process must appear under background"
        );
    }

    // Acceptance criterion 4: the cached snapshot reflects the most recent tick.
    #[tokio::test(start_paused = true)]
    async fn latest_snapshot_reflects_most_recent_tick() {
        let calls = Arc::new(AtomicUsize::new(0));
        let sampler = Box::new(FakeSampler {
            calls: Arc::clone(&calls),
        });
        let hyprland = Arc::new(FakeHyprland {
            response: Ok(fake_clients_json()),
        });
        let monitor = TokioProcessMonitor::new(Handle::current(), sampler, hyprland);

        assert!(
            monitor.latest_snapshot().is_none(),
            "no snapshot before the first tick"
        );

        let mut stream = monitor.watch();
        tokio::time::advance(Duration::from_secs(1)).await;
        let _ = stream.next().await.expect("first snapshot");

        let latest = monitor.latest_snapshot().expect("cached snapshot present");
        assert!(contains_pid(&latest.apps, WINDOW_PID));
        assert_eq!(latest.global, fake_global());
    }
}
