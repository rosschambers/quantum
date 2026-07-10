//! `DirectoryWatcher` implementation backed by the `notify` crate.
//!
//! [`NotifyDirectoryWatcher`] arms a non-recursive `notify` watch on a directory
//! and exposes a debounced stream: a rapid burst of filesystem events collapses
//! into a single stream item carrying the watched path, emitted 250 milliseconds
//! after the last event in the burst settles.
//!
//! The `notify` recommended watcher delivers events synchronously on its own
//! thread through a callback. That callback forwards a bare signal over a
//! [`tokio::sync::mpsc`] channel into an async debounce task. The task owns the
//! watcher, so when it ends (through `unwatch`, a dropped consumer, or a closed
//! channel) the watcher drops and the operating-system watch is released. Each
//! debounced emission is pushed onto a [`futures::channel::mpsc`] whose receiver
//! is returned to the caller as the stream, which keeps this crate free of a
//! `tokio-stream` dependency.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender as StreamSender;
use futures::stream::{BoxStream, StreamExt};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use quantum_domain::{DirectoryWatcher, FilesError};
use tokio::sync::mpsc::{UnboundedReceiver as SignalReceiver, UnboundedSender as SignalSender};
use tokio::sync::oneshot;

/// The quiet period after the last event in a burst before a single item is
/// emitted for that burst.
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(250);

/// A [`DirectoryWatcher`] backed by the `notify` crate with per-burst debouncing.
///
/// The registry maps each watched path to a stop signal for its debounce task,
/// so [`DirectoryWatcher::unwatch`] can terminate delivery and release the
/// underlying watch.
#[derive(Debug, Default)]
pub struct NotifyDirectoryWatcher {
    stops: Mutex<HashMap<String, oneshot::Sender<()>>>,
}

impl NotifyDirectoryWatcher {
    /// Construct a watcher with an empty registry.
    pub fn new() -> Self {
        Self {
            stops: Mutex::new(HashMap::new()),
        }
    }
}

/// Map a `notify` error to a typed [`FilesError`], preserving the affected path.
fn map_notify_error(error: notify::Error, path: &str) -> FilesError {
    match error.kind {
        notify::ErrorKind::PathNotFound => FilesError::NotFound(path.to_string()),
        notify::ErrorKind::Io(io_error) => match io_error.kind() {
            std::io::ErrorKind::NotFound => FilesError::NotFound(path.to_string()),
            std::io::ErrorKind::PermissionDenied => FilesError::PermissionDenied(path.to_string()),
            _ => FilesError::Io(io_error.to_string()),
        },
        other => FilesError::Io(format!("{other:?}")),
    }
}

/// Debounce loop: owns the `notify` watcher for the lifetime of the watch.
///
/// It waits for the first signal of a burst, then resets a 250 millisecond timer
/// on every further signal, emitting exactly one item when the timer elapses.
/// The loop ends when the stop signal fires, the signal channel closes (the
/// watcher was dropped), or the consumer drops the returned stream.
async fn debounce_loop(
    path: String,
    mut signals: SignalReceiver<()>,
    mut stop: oneshot::Receiver<()>,
    emitter: StreamSender<String>,
    _watcher: RecommendedWatcher,
) {
    loop {
        // Wait for the first event of a burst (or for teardown).
        tokio::select! {
            _ = &mut stop => break,
            first = signals.recv() => {
                if first.is_none() {
                    break;
                }
            }
        }

        // Coalesce the burst: each further event restarts the debounce timer.
        loop {
            tokio::select! {
                _ = &mut stop => return,
                next = signals.recv() => {
                    if next.is_none() {
                        let _ = emitter.unbounded_send(path.clone());
                        return;
                    }
                }
                _ = tokio::time::sleep(DEBOUNCE_WINDOW) => {
                    if emitter.unbounded_send(path.clone()).is_err() {
                        return;
                    }
                    break;
                }
            }
        }
    }
}

impl DirectoryWatcher for NotifyDirectoryWatcher {
    fn watch(&self, path: &str) -> Result<BoxStream<'static, String>, FilesError> {
        if !Path::new(path).exists() {
            return Err(FilesError::NotFound(path.to_string()));
        }

        let (signal_sender, signal_receiver): (SignalSender<()>, SignalReceiver<()>) =
            tokio::sync::mpsc::unbounded_channel();

        let mut watcher =
            notify::recommended_watcher(move |result: Result<Event, notify::Error>| {
                if result.is_ok() {
                    let _ = signal_sender.send(());
                }
            })
            .map_err(|error| map_notify_error(error, path))?;

        watcher
            .watch(Path::new(path), RecursiveMode::NonRecursive)
            .map_err(|error| map_notify_error(error, path))?;

        let (emitter, receiver) = futures::channel::mpsc::unbounded::<String>();
        let (stop_sender, stop_receiver) = oneshot::channel::<()>();

        tokio::spawn(debounce_loop(
            path.to_string(),
            signal_receiver,
            stop_receiver,
            emitter,
            watcher,
        ));

        if let Ok(mut stops) = self.stops.lock() {
            // Replacing an existing entry drops its stop sender, which ends the
            // previous debounce task for this path.
            stops.insert(path.to_string(), stop_sender);
        }

        Ok(receiver.boxed())
    }

    fn unwatch(&self, path: &str) {
        if let Ok(mut stops) = self.stops.lock() {
            if let Some(stop_sender) = stops.remove(path) {
                let _ = stop_sender.send(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use quantum_domain::DirectoryWatcher;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test(flavor = "multi_thread")]
    async fn coalesces_a_burst_into_one_item_then_unwatch_stops_delivery() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let path = directory
            .path()
            .to_str()
            .expect("temporary path is valid unicode")
            .to_string();

        let watcher = NotifyDirectoryWatcher::new();
        let mut stream = watcher.watch(&path).expect("watch the temporary directory");

        // Give the underlying inotify watch a moment to arm before writing.
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Three rapid creations inside one debounce window.
        for index in 0..3 {
            let file = directory.path().join(format!("file-{index}.txt"));
            std::fs::write(&file, b"content").expect("write burst file");
        }

        let first = timeout(Duration::from_secs(1), stream.next()).await;
        assert!(
            matches!(first, Ok(Some(_))),
            "expected exactly one debounced item, got {first:?}"
        );

        // No second item: the burst must coalesce into a single emission.
        let extra = timeout(Duration::from_millis(500), stream.next()).await;
        assert!(
            !matches!(extra, Ok(Some(_))),
            "expected no second item from a single burst, got {extra:?}"
        );

        watcher.unwatch(&path);

        // A change after unwatch must not produce a further item.
        let after = directory.path().join("after-unwatch.txt");
        std::fs::write(&after, b"content").expect("write post-unwatch file");
        let post = timeout(Duration::from_millis(500), stream.next()).await;
        assert!(
            !matches!(post, Ok(Some(_))),
            "expected no item after unwatch, got {post:?}"
        );
    }
}
