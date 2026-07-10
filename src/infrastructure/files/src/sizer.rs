//! [`RecursiveSizer`] implementation: cancellable background directory sizing.
//!
//! [`BackgroundSizer`] walks a directory tree on a blocking thread, summing the
//! sizes of every regular file beneath a root. The walk uses synchronous
//! `std::fs` recursion, so it runs on [`tokio::task::spawn_blocking`] to keep the
//! async runtime free. It emits a [`SizeUpdate`] with the running total no more
//! than once every 100 milliseconds (throttled by wall-clock), then a final
//! update with `complete` set to `true` when the walk finishes.
//!
//! Symlinks are never followed: an entry that is a symlink is skipped and its
//! target is neither traversed nor counted, so a link into a large tree outside
//! the root cannot inflate the total.
//!
//! Each computation registers an `Arc<AtomicBool>` cancellation flag keyed by
//! path. [`RecursiveSizer::cancel`] sets that flag; the walk checks it for every
//! directory and entry and stops promptly, ending the stream WITHOUT a
//! completion item. The registry entry is removed once the walk finishes or is
//! cancelled.
//!
//! The blocking walk writes updates to a [`tokio::sync::mpsc`] channel; a bridge
//! task forwards them onto a [`futures::channel::mpsc`] whose receiver is
//! returned as the stream, mirroring the watcher's approach and keeping this
//! crate free of a `tokio-stream` dependency.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender as StreamSender;
use futures::stream::{BoxStream, StreamExt};
use quantum_domain::{RecursiveSizer, SizeUpdate};
use tokio::sync::mpsc::{UnboundedReceiver as UpdateReceiver, UnboundedSender as UpdateSender};

/// The minimum wall-clock gap between successive progress emissions.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// A [`RecursiveSizer`] that walks directory trees on a blocking thread and
/// streams throttled progress, honouring a per-path cancellation flag.
#[derive(Debug, Default)]
pub struct BackgroundSizer {
    cancellations: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl BackgroundSizer {
    /// Construct a sizer with an empty cancellation registry.
    pub fn new() -> Self {
        Self {
            cancellations: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// Recursively sum the sizes of every regular file beneath `root`, emitting
/// throttled progress and a final completion item.
///
/// Symlinks are skipped rather than followed. The `cancel` flag is checked for
/// every directory and entry; when set, the walk returns immediately without
/// emitting a completion item. Any emission failure (the consumer dropped the
/// stream) also ends the walk.
fn walk_and_emit(root: &str, cancel: &AtomicBool, updates: &UpdateSender<SizeUpdate>) {
    let mut total: u64 = 0;
    let mut last_emit = Instant::now();
    let mut pending = vec![PathBuf::from(root)];

    while let Some(directory) = pending.pop() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }

        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                return;
            }

            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let entry_path = entry.path();

            // `symlink_metadata` reports on the link itself, never its target,
            // so symlinks are identified rather than followed.
            let metadata = match std::fs::symlink_metadata(&entry_path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let file_type = metadata.file_type();

            if file_type.is_symlink() {
                continue;
            }

            if file_type.is_dir() {
                pending.push(entry_path);
                continue;
            }

            if file_type.is_file() {
                total = total.saturating_add(metadata.len());

                if last_emit.elapsed() >= PROGRESS_INTERVAL {
                    let progress = SizeUpdate {
                        path: root.to_string(),
                        bytes: total,
                        complete: false,
                    };
                    if updates.send(progress).is_err() {
                        return;
                    }
                    last_emit = Instant::now();
                }
            }
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return;
    }

    let _ = updates.send(SizeUpdate {
        path: root.to_string(),
        bytes: total,
        complete: true,
    });
}

/// Forward every update from the blocking walk onto the returned stream, then
/// remove the path's registry entry once the walk has finished or been
/// cancelled (signalled by the blocking sender being dropped).
///
/// Removal is guarded by identity: only the flag this walk registered is
/// evicted, so a later [`RecursiveSizer::compute`] on the same path that
/// replaced the entry keeps its own flag.
async fn bridge_updates(
    mut incoming: UpdateReceiver<SizeUpdate>,
    emitter: StreamSender<SizeUpdate>,
    registry: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    key: String,
    flag: Arc<AtomicBool>,
) {
    while let Some(update) = incoming.recv().await {
        if emitter.unbounded_send(update).is_err() {
            break;
        }
    }

    if let Ok(mut map) = registry.lock() {
        if map
            .get(&key)
            .is_some_and(|current| Arc::ptr_eq(current, &flag))
        {
            map.remove(&key);
        }
    }
}

impl RecursiveSizer for BackgroundSizer {
    fn compute(&self, path: &str) -> BoxStream<'static, SizeUpdate> {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut map) = self.cancellations.lock() {
            // Replacing an existing entry drops the previous flag; the older
            // walk finishes on its own and its bridge leaves this newer entry
            // in place thanks to the identity-guarded removal.
            map.insert(path.to_string(), cancel_flag.clone());
        }

        let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel::<SizeUpdate>();
        let walk_path = path.to_string();
        let walk_flag = cancel_flag.clone();
        tokio::task::spawn_blocking(move || {
            walk_and_emit(&walk_path, &walk_flag, &update_sender);
        });

        let (emitter, receiver) = futures::channel::mpsc::unbounded::<SizeUpdate>();
        tokio::spawn(bridge_updates(
            update_receiver,
            emitter,
            self.cancellations.clone(),
            path.to_string(),
            cancel_flag,
        ));

        receiver.boxed()
    }

    fn cancel(&self, path: &str) {
        if let Ok(map) = self.cancellations.lock() {
            if let Some(flag) = map.get(path) {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use quantum_domain::{RecursiveSizer, SizeUpdate};
    use std::os::unix::fs::symlink;

    /// Build the described tree and assert the final update reports the total of
    /// every regular file beneath the root and marks the walk complete.
    #[tokio::test]
    async fn sums_every_regular_file_and_marks_complete() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let root = directory.path();

        std::fs::write(root.join("a.bin"), vec![0u8; 1000]).expect("write a.bin");
        std::fs::write(root.join("b.bin"), vec![0u8; 2000]).expect("write b.bin");
        let subdirectory = root.join("nested");
        std::fs::create_dir(&subdirectory).expect("create nested directory");
        std::fs::write(subdirectory.join("c.bin"), vec![0u8; 3000]).expect("write c.bin");

        let path = root.to_str().expect("temporary path is valid unicode");
        let sizer = BackgroundSizer::new();
        let updates: Vec<SizeUpdate> = sizer.compute(path).collect().await;

        let completed = updates
            .iter()
            .find(|update| update.complete)
            .expect("a completed update");
        assert_eq!(completed.bytes, 6000, "final total should sum all files");
        assert_eq!(
            updates.last().map(|update| update.complete),
            Some(true),
            "the final emitted item should be the completion"
        );
    }

    /// A walk cancelled before it is drained must never yield a completion item.
    /// A large tree plus an immediate synchronous cancel guarantees the walk
    /// cannot finish before the flag is set.
    #[tokio::test]
    async fn cancelled_walk_never_yields_completion() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let root = directory.path();
        for index in 0..1000 {
            std::fs::write(root.join(format!("file-{index}.bin")), vec![0u8; 64])
                .expect("write filler file");
        }

        let path = root
            .to_str()
            .expect("temporary path is valid unicode")
            .to_string();
        let sizer = BackgroundSizer::new();
        let stream = sizer.compute(&path);
        sizer.cancel(&path);

        let updates: Vec<SizeUpdate> = stream.collect().await;
        assert!(
            updates.iter().all(|update| !update.complete),
            "a cancelled walk must not emit a completion item, got {updates:?}"
        );
    }

    /// A symlink pointing at a file outside the tree must not be traversed, so
    /// its target's size is excluded from the total.
    #[tokio::test]
    async fn does_not_follow_symlinks_out_of_the_tree() {
        let outside = tempfile::tempdir().expect("create outside directory");
        let big_target = outside.path().join("large.bin");
        std::fs::write(&big_target, vec![0u8; 1_000_000]).expect("write large target");

        let directory = tempfile::tempdir().expect("create tree directory");
        let root = directory.path();
        std::fs::write(root.join("real.bin"), vec![0u8; 500]).expect("write real file");
        symlink(&big_target, root.join("link.bin")).expect("create symlink into tree");

        let path = root.to_str().expect("temporary path is valid unicode");
        let sizer = BackgroundSizer::new();
        let updates: Vec<SizeUpdate> = sizer.compute(path).collect().await;

        let completed = updates
            .iter()
            .find(|update| update.complete)
            .expect("a completed update");
        assert_eq!(
            completed.bytes, 500,
            "the symlink target's size must be excluded from the total"
        );
    }
}
