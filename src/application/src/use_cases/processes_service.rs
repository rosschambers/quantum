//! Process task-manager service use case.
//!
//! Core orchestration for the task-manager feature. Injects the domain ports
//! (`ProcessMonitor`, `ProcessKiller`, `EventBus`) and turns frontend requests
//! into port calls, publishing streaming snapshots on the `processes.event`
//! channel. This crate depends only on `quantum_domain`; it never touches
//! infrastructure directly. The real ports are injected by the daemon.
//!
//! Unlike the file explorer there is a single process stream (not one per
//! path), so the reference-counted watch subscription is a single
//! `Mutex<Option<Subscription>>` rather than a per-path map. Each snapshot is
//! forwarded verbatim as its serialized JSON on `processes.event`.

use crate::error::Result;
use futures::stream::StreamExt;
use quantum_domain::{EventBus, KillSignal, ProcessKiller, ProcessMonitor};
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

/// The broadcast channel every process snapshot is published on.
const PROCESSES_EVENT_CHANNEL: &str = "processes.event";

/// A reference-counted subscription: the single spawned forwarding task shared
/// by every watcher, plus the number of live watchers. The task (and the
/// underlying monitor subscription armed once) is torn down only when the count
/// falls back to zero, so a second watcher never clobbers the first.
struct Subscription {
    count: usize,
    handle: JoinHandle<()>,
}

/// Orchestrates the process task-manager subsystem. Holds the injected ports
/// plus the single reference-counted handle of the spawned forwarding task so
/// the one process stream can be shared across watchers and torn down on the
/// final `unwatch`.
pub struct ProcessesService {
    monitor: Arc<dyn ProcessMonitor>,
    killer: Arc<dyn ProcessKiller>,
    event_bus: Arc<dyn EventBus>,
    /// Reference-counted watch subscription. There is only one process stream,
    /// so this is a single slot rather than a per-path map; the monitor is
    /// subscribed once and released only when the last watcher calls `unwatch`.
    watch_handle: Mutex<Option<Subscription>>,
}

impl ProcessesService {
    pub fn new(
        monitor: Arc<dyn ProcessMonitor>,
        killer: Arc<dyn ProcessKiller>,
        event_bus: Arc<dyn EventBus>,
    ) -> Self {
        Self {
            monitor,
            killer,
            event_bus,
            watch_handle: Mutex::new(None),
        }
    }

    /// Start watching the process forest, spawning a task that republishes each
    /// snapshot as its serialized JSON on `processes.event`. Reference-counted:
    /// a second watcher increments the count and shares the single armed monitor
    /// subscription and forwarding task rather than subscribing the monitor a
    /// second time or spawning a duplicate forwarder.
    pub fn watch(&self) {
        let mut handle = Self::lock(&self.watch_handle);
        if let Some(subscription) = handle.as_mut() {
            subscription.count += 1;
            return;
        }
        let stream = self.monitor.watch();
        let event_bus = self.event_bus.clone();
        let task = tokio::spawn(async move {
            let mut stream = stream;
            while let Some(snapshot) = stream.next().await {
                if let Ok(payload) = serde_json::to_string(&snapshot) {
                    let _ = event_bus.publish(PROCESSES_EVENT_CHANNEL, &payload).await;
                }
            }
        });
        *handle = Some(Subscription {
            count: 1,
            handle: task,
        });
    }

    /// Stop watching the process forest. Decrements the reference count; only
    /// when the last watcher leaves does it abort the forwarding task and
    /// release the underlying monitor subscription, so one watcher leaving never
    /// tears down a stream another watcher still needs.
    pub fn unwatch(&self) {
        let mut handle = Self::lock(&self.watch_handle);
        let remaining = match handle.as_mut() {
            Some(subscription) => {
                subscription.count -= 1;
                subscription.count
            }
            None => return,
        };
        if remaining > 0 {
            return;
        }
        if let Some(subscription) = handle.take() {
            subscription.handle.abort();
        }
        drop(handle);
        self.monitor.unwatch();
    }

    /// Terminate the process subtree rooted at `pid` with `signal`, delegating
    /// to the killer. A killer error (protected subtree, permission denied,
    /// missing snapshot, and so on) is mapped into the application error type
    /// and propagated.
    pub async fn kill(&self, pid: i32, signal: KillSignal) -> Result<()> {
        self.killer.kill_subtree(pid, signal).await?;
        Ok(())
    }

    /// Lock the watch handle, recovering the guard if a panicking task poisoned
    /// the mutex. The guard is never held across an await, so recovery is safe.
    fn lock(
        handle: &Mutex<Option<Subscription>>,
    ) -> std::sync::MutexGuard<'_, Option<Subscription>> {
        handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream::{self, BoxStream};
    use quantum_domain::{DomainError, GlobalStats, ProcessNode, ProcessSnapshot, ProcessesError};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex as TokioMutex;

    /// A minimal snapshot with one background process, for the forwarding test.
    fn sample_snapshot() -> ProcessSnapshot {
        ProcessSnapshot {
            global: GlobalStats {
                cpu_percent: 3.5,
                mem_used_bytes: 1_000,
                mem_total_bytes: 2_000,
                net_rx_bytes_per_second: 0,
                net_tx_bytes_per_second: 0,
            },
            apps: Vec::new(),
            background: vec![ProcessNode {
                pid: 4321,
                name: "sleep".to_string(),
                cpu_percent: 0.0,
                mem_bytes: 100,
                aggregate_cpu_percent: 0.0,
                aggregate_mem_bytes: 100,
                window: None,
                protected: false,
                children: Vec::new(),
            }],
        }
    }

    /// Monitor mock whose `watch` yields the supplied snapshots once.
    struct FakeMonitor {
        snapshots: Vec<ProcessSnapshot>,
    }

    impl ProcessMonitor for FakeMonitor {
        fn watch(&self) -> BoxStream<'static, ProcessSnapshot> {
            stream::iter(self.snapshots.clone()).boxed()
        }
        fn unwatch(&self) {}
    }

    /// Killer mock: records every `(pid, signal)` it is asked to kill, and can
    /// be configured to fail with a protected-process error instead.
    struct FakeKiller {
        calls: TokioMutex<Vec<(i32, KillSignal)>>,
        protected: bool,
    }

    impl FakeKiller {
        fn new(protected: bool) -> Self {
            Self {
                calls: TokioMutex::new(Vec::new()),
                protected,
            }
        }
    }

    #[async_trait]
    impl ProcessKiller for FakeKiller {
        async fn kill_subtree(
            &self,
            pid: i32,
            signal: KillSignal,
        ) -> std::result::Result<(), ProcessesError> {
            self.calls.lock().await.push((pid, signal));
            if self.protected {
                Err(ProcessesError::Protected(pid))
            } else {
                Ok(())
            }
        }
    }

    /// Event-bus mock that captures every `(channel, payload)` it is asked to
    /// publish, behind an async mutex so the spawned forwarder can record
    /// concurrently.
    struct FakeEventBus {
        events: TokioMutex<Vec<(String, String)>>,
    }

    impl FakeEventBus {
        fn new() -> Self {
            Self {
                events: TokioMutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl EventBus for FakeEventBus {
        async fn publish(
            &self,
            event: &str,
            payload: &str,
        ) -> std::result::Result<(), DomainError> {
            self.events
                .lock()
                .await
                .push((event.to_string(), payload.to_string()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn watch_forwards_snapshot_json_on_processes_event() {
        let monitor = Arc::new(FakeMonitor {
            snapshots: vec![sample_snapshot()],
        });
        let killer = Arc::new(FakeKiller::new(false));
        let event_bus = Arc::new(FakeEventBus::new());
        let service =
            ProcessesService::new(monitor, killer, event_bus.clone() as Arc<dyn EventBus>);

        service.watch();
        // Let the spawned forwarding task drain the one-element stream.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let events = event_bus.events.lock().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "processes.event");
        // The payload is exactly the serialized snapshot.
        let expected = serde_json::to_string(&sample_snapshot()).expect("serialize");
        assert_eq!(events[0].1, expected);
        assert!(events[0].1.contains("\"pid\":4321"));
    }

    #[tokio::test]
    async fn kill_delegates_pid_and_signal_to_killer() {
        let monitor = Arc::new(FakeMonitor { snapshots: vec![] });
        let killer = Arc::new(FakeKiller::new(false));
        let event_bus = Arc::new(FakeEventBus::new());
        let service =
            ProcessesService::new(monitor, killer.clone() as Arc<dyn ProcessKiller>, event_bus);

        service.kill(4321, KillSignal::Kill).await.expect("kill");

        let calls = killer.calls.lock().await;
        assert_eq!(*calls, vec![(4321, KillSignal::Kill)]);
    }

    #[tokio::test]
    async fn kill_error_propagates() {
        let monitor = Arc::new(FakeMonitor { snapshots: vec![] });
        let killer = Arc::new(FakeKiller::new(true));
        let event_bus = Arc::new(FakeEventBus::new());
        let service = ProcessesService::new(monitor, killer, event_bus);

        let result = service.kill(100, KillSignal::Term).await;
        assert!(result.is_err());
    }

    /// Process-monitor mock that counts how many times the stream is armed and
    /// released, and exposes a channel so a test can push snapshots into the
    /// live subscription's stream at will.
    struct CountingMonitor {
        watch_calls: Arc<AtomicUsize>,
        unwatch_calls: Arc<AtomicUsize>,
        sender: Arc<Mutex<Option<futures::channel::mpsc::UnboundedSender<ProcessSnapshot>>>>,
    }

    impl ProcessMonitor for CountingMonitor {
        fn watch(&self) -> BoxStream<'static, ProcessSnapshot> {
            self.watch_calls.fetch_add(1, Ordering::SeqCst);
            let (sender, receiver) = futures::channel::mpsc::unbounded();
            *self
                .sender
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sender);
            receiver.boxed()
        }
        fn unwatch(&self) {
            self.unwatch_calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn watch_is_reference_counted_across_watchers() {
        let watch_calls = Arc::new(AtomicUsize::new(0));
        let unwatch_calls = Arc::new(AtomicUsize::new(0));
        let sender_slot = Arc::new(Mutex::new(None));
        let monitor = Arc::new(CountingMonitor {
            watch_calls: watch_calls.clone(),
            unwatch_calls: unwatch_calls.clone(),
            sender: sender_slot.clone(),
        });
        let killer = Arc::new(FakeKiller::new(false));
        let event_bus = Arc::new(FakeEventBus::new());
        let service =
            ProcessesService::new(monitor, killer, event_bus.clone() as Arc<dyn EventBus>);

        // Two watchers subscribe; the monitor is armed exactly once.
        service.watch();
        service.watch();
        assert_eq!(watch_calls.load(Ordering::SeqCst), 1);

        // One watcher leaves. The subscription must stay live for the other.
        service.unwatch();
        assert_eq!(unwatch_calls.load(Ordering::SeqCst), 0);

        let send_snapshot = || {
            sender_slot
                .lock()
                .unwrap()
                .as_ref()
                .expect("monitor armed")
                .unbounded_send(sample_snapshot())
                .expect("send snapshot");
        };
        send_snapshot();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        {
            let events = event_bus.events.lock().await;
            assert_eq!(events.len(), 1, "snapshot should still forward: {events:?}");
        }

        // The second watcher leaves. Now the subscription is finally released.
        service.unwatch();
        assert_eq!(unwatch_calls.load(Ordering::SeqCst), 1);

        // With the forwarder aborted, further snapshots are not forwarded.
        send_snapshot();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let events = event_bus.events.lock().await;
        assert_eq!(events.len(), 1, "no forwarding after teardown: {events:?}");
    }
}
