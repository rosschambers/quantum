//! Cursor watch service use case.
//!
//! Core orchestration for the cursor-flash feature. Injects the domain ports
//! (`CursorMonitor`, `EventBus`) and turns frontend requests into port calls,
//! publishing streaming positions on the `cursor.event` channel. This crate
//! depends only on `quantum_domain`; it never touches infrastructure directly.
//! The real ports are injected by the daemon.
//!
//! There is a single cursor stream, so the reference-counted watch
//! subscription is a single `Mutex<Option<Subscription>>` rather than a
//! per-path map. Each position is forwarded verbatim as its serialized JSON on
//! `cursor.event`.

use futures::stream::StreamExt;
use quantum_domain::{CursorMonitor, EventBus};
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

/// The broadcast channel every cursor position is published on.
const CURSOR_EVENT_CHANNEL: &str = "cursor.event";

/// A reference-counted subscription: the single spawned forwarding task shared
/// by every watcher, plus the number of live watchers. The task (and the
/// underlying monitor subscription armed once) is torn down only when the count
/// falls back to zero, so a second watcher never clobbers the first.
struct Subscription {
    count: usize,
    handle: JoinHandle<()>,
}

/// Orchestrates the cursor-flash subsystem. Holds the injected ports plus the
/// single reference-counted handle of the spawned forwarding task so the one
/// cursor stream can be shared across watchers and torn down on the final
/// `unwatch`.
pub struct CursorService {
    monitor: Arc<dyn CursorMonitor>,
    event_bus: Arc<dyn EventBus>,
    /// Reference-counted watch subscription. There is only one cursor stream,
    /// so this is a single slot rather than a per-path map; the monitor is
    /// subscribed once and released only when the last watcher calls `unwatch`.
    watch_handle: Mutex<Option<Subscription>>,
}

impl CursorService {
    pub fn new(monitor: Arc<dyn CursorMonitor>, event_bus: Arc<dyn EventBus>) -> Self {
        Self {
            monitor,
            event_bus,
            watch_handle: Mutex::new(None),
        }
    }

    /// Start watching the cursor, spawning a task that republishes each
    /// position as its serialized JSON on `cursor.event`. Reference-counted:
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
            while let Some(position) = stream.next().await {
                if let Ok(payload) = serde_json::to_string(&position) {
                    let _ = event_bus.publish(CURSOR_EVENT_CHANNEL, &payload).await;
                }
            }
        });
        *handle = Some(Subscription {
            count: 1,
            handle: task,
        });
    }

    /// Stop watching the cursor. Decrements the reference count; only when the
    /// last watcher leaves does it abort the forwarding task and release the
    /// underlying monitor subscription, so one watcher leaving never tears down
    /// a stream another watcher still needs.
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
    use quantum_domain::{CursorPosition, DomainError};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex as TokioMutex;

    /// A sample position, for the forwarding test.
    fn sample_position() -> CursorPosition {
        CursorPosition { x: 42, y: -7 }
    }

    /// Monitor mock whose `watch` yields the supplied positions once.
    struct FakeMonitor {
        positions: Vec<CursorPosition>,
    }

    impl CursorMonitor for FakeMonitor {
        fn watch(&self) -> BoxStream<'static, CursorPosition> {
            stream::iter(self.positions.clone()).boxed()
        }
        fn unwatch(&self) {}
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
    async fn watch_forwards_position_json_on_cursor_event() {
        let monitor = Arc::new(FakeMonitor {
            positions: vec![sample_position()],
        });
        let event_bus = Arc::new(FakeEventBus::new());
        let service = CursorService::new(monitor, event_bus.clone() as Arc<dyn EventBus>);

        service.watch();
        // Let the spawned forwarding task drain the one-element stream.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let events = event_bus.events.lock().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "cursor.event");
        // The payload is exactly the serialized position.
        let expected = serde_json::to_string(&sample_position()).expect("serialize");
        assert_eq!(events[0].1, expected);
        assert!(events[0].1.contains("\"x\":42"));
    }

    /// Cursor-monitor mock that counts how many times the stream is armed and
    /// released, and exposes a channel so a test can push positions into the
    /// live subscription's stream at will.
    struct CountingMonitor {
        watch_calls: Arc<AtomicUsize>,
        unwatch_calls: Arc<AtomicUsize>,
        sender: Arc<Mutex<Option<futures::channel::mpsc::UnboundedSender<CursorPosition>>>>,
    }

    impl CursorMonitor for CountingMonitor {
        fn watch(&self) -> BoxStream<'static, CursorPosition> {
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
        let event_bus = Arc::new(FakeEventBus::new());
        let service = CursorService::new(monitor, event_bus.clone() as Arc<dyn EventBus>);

        // Two watchers subscribe; the monitor is armed exactly once.
        service.watch();
        service.watch();
        assert_eq!(watch_calls.load(Ordering::SeqCst), 1);

        // One watcher leaves. The subscription must stay live for the other.
        service.unwatch();
        assert_eq!(unwatch_calls.load(Ordering::SeqCst), 0);

        let send_position = || {
            sender_slot
                .lock()
                .unwrap()
                .as_ref()
                .expect("monitor armed")
                .unbounded_send(sample_position())
                .expect("send position");
        };
        send_position();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        {
            let events = event_bus.events.lock().await;
            assert_eq!(events.len(), 1, "position should still forward: {events:?}");
        }

        // The second watcher leaves. Now the subscription is finally released.
        service.unwatch();
        assert_eq!(unwatch_calls.load(Ordering::SeqCst), 1);

        // With the forwarder aborted, further positions are not forwarded.
        send_position();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let events = event_bus.events.lock().await;
        assert_eq!(events.len(), 1, "no forwarding after teardown: {events:?}");
    }
}
