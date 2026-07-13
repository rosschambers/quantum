//! [`RecursiveSizer`] implementation: cancellable background directory sizing.
//!
//! [`BackgroundSizer::compute`] sizes each immediate child directory of the
//! requested directory: it enumerates the directory's entries and, for every
//! entry that is itself a directory, recursively sums the sizes of every regular
//! file beneath that child and emits [`SizeUpdate`]s keyed by the CHILD
//! directory's path. Regular files directly in the requested directory and
//! symlinked children are ignored (their sizes are already known from the
//! listing, or must not be followed). Each child walk uses synchronous
//! `std::fs` recursion, so the work runs on [`tokio::task::spawn_blocking`] to
//! keep the async runtime free. Per child it emits a [`SizeUpdate`] with the
//! running total no more than once every 100 milliseconds (throttled by
//! wall-clock), then a final update with `complete` set to `true` when that
//! child's walk finishes.
//!
//! Symlinks are never followed: a symlinked child directory is skipped entirely,
//! and inside a child a symlink entry is skipped and its target is neither
//! traversed nor counted, so a link into a large tree outside the child cannot
//! inflate the total.
//!
//! Each computation registers a single `Arc<AtomicBool>` cancellation flag keyed
//! by the requested directory's path, shared by every child walk.
//! [`RecursiveSizer::cancel`] sets that flag; every child walk checks it for
//! every directory and entry and stops promptly, ending the stream WITHOUT a
//! completion item. The registry entry is removed once the walks finish or are
//! cancelled.
//!
//! The blocking walk writes updates to a [`tokio::sync::mpsc`] channel; a bridge
//! task forwards them onto a [`futures::channel::mpsc`] whose receiver is
//! returned as the stream, mirroring the watcher's approach and keeping this
//! crate free of a `tokio-stream` dependency.

use std::collections::HashMap;
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

/// Enumerate `dir`'s immediate children and recursively size each child
/// directory, emitting updates keyed by the child's path. Regular files and
/// symlinks directly in `dir` are ignored. Stops early on cancellation or when
/// the consumer drops the stream.
fn walk_and_emit(dir: &str, cancel: &AtomicBool, updates: &UpdateSender<SizeUpdate>) {
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let child_path = entry.path();
        // `symlink_metadata` reports on the link itself: a symlinked directory
        // has `is_dir() == false` here, so symlinks are skipped, not followed.
        let metadata = match std::fs::symlink_metadata(&child_path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if !metadata.file_type().is_dir() {
            continue;
        }
        let key = child_path.to_string_lossy().to_string();
        if !size_child(&child_path, &key, cancel, updates) {
            return;
        }
    }
}

/// Recursively sum every regular file beneath `root`, emitting throttled
/// progress and a final completion keyed by `key`. Returns `false` when the
/// walk was cancelled or the consumer dropped the stream, so the caller stops.
fn size_child(
    root: &std::path::Path,
    key: &str,
    cancel: &AtomicBool,
    updates: &UpdateSender<SizeUpdate>,
) -> bool {
    let mut total: u64 = 0;
    let mut last_emit = Instant::now();
    let mut pending = vec![root.to_path_buf()];

    while let Some(directory) = pending.pop() {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let entry_path = entry.path();
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
                        path: key.to_string(),
                        bytes: total,
                        complete: false,
                    };
                    if updates.send(progress).is_err() {
                        return false;
                    }
                    last_emit = Instant::now();
                }
            }
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return false;
    }
    updates
        .send(SizeUpdate {
            path: key.to_string(),
            bytes: total,
            complete: true,
        })
        .is_ok()
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

    /// A directory's child directories are each sized and reported under their own
    /// path; loose files in the directory and the directory itself are not emitted.
    #[tokio::test]
    async fn sizes_each_child_directory_under_its_own_path() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let root = directory.path();

        // Loose file directly in root: must NOT be emitted.
        std::fs::write(root.join("loose.bin"), vec![0u8; 500]).expect("write loose.bin");

        let a = root.join("a");
        std::fs::create_dir(&a).expect("create a");
        std::fs::write(a.join("f1"), vec![0u8; 4000]).expect("write a/f1");
        let a_nested = a.join("nested");
        std::fs::create_dir(&a_nested).expect("create a/nested");
        std::fs::write(a_nested.join("f2"), vec![0u8; 2000]).expect("write a/nested/f2");

        let b = root.join("b");
        std::fs::create_dir(&b).expect("create b");
        std::fs::write(b.join("g1"), vec![0u8; 2000]).expect("write b/g1");

        let path = root.to_str().expect("valid unicode");
        let sizer = BackgroundSizer::new();
        let updates: Vec<SizeUpdate> = sizer.compute(path).collect().await;

        let a_key = a.to_str().unwrap();
        let b_key = b.to_str().unwrap();
        let root_key = root.to_str().unwrap();

        let a_final = updates
            .iter()
            .find(|u| u.path == a_key && u.complete)
            .expect("a complete");
        assert_eq!(a_final.bytes, 6000, "a is the recursive sum of its files");
        let b_final = updates
            .iter()
            .find(|u| u.path == b_key && u.complete)
            .expect("b complete");
        assert_eq!(b_final.bytes, 2000);
        assert!(
            updates.iter().all(|u| u.path != root_key),
            "the parent directory is never emitted"
        );
        assert!(
            updates.iter().all(|u| !u.path.ends_with("loose.bin")),
            "loose files are not emitted",
        );
    }

    /// A symlinked child directory is not sized (it is skipped, not followed).
    #[tokio::test]
    async fn skips_symlinked_child_directories() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("big.bin"), vec![0u8; 1_000_000]).expect("write big");

        let directory = tempfile::tempdir().expect("tree");
        let root = directory.path();
        symlink(outside.path(), root.join("link")).expect("symlink child dir");

        let path = root.to_str().expect("valid unicode");
        let sizer = BackgroundSizer::new();
        let updates: Vec<SizeUpdate> = sizer.compute(path).collect().await;
        assert!(
            updates.iter().all(|u| !u.path.ends_with("link")),
            "symlinked child is skipped"
        );
    }

    /// A symlink INSIDE a child directory is not followed; its target is excluded.
    #[tokio::test]
    async fn does_not_follow_symlinks_inside_a_child() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().expect("outside");
        let big = outside.path().join("large.bin");
        std::fs::write(&big, vec![0u8; 1_000_000]).expect("write large");

        let directory = tempfile::tempdir().expect("tree");
        let root = directory.path();
        let child = root.join("child");
        std::fs::create_dir(&child).expect("create child");
        std::fs::write(child.join("real.bin"), vec![0u8; 500]).expect("write real");
        symlink(&big, child.join("link.bin")).expect("symlink into child");

        let path = root.to_str().expect("valid unicode");
        let sizer = BackgroundSizer::new();
        let updates: Vec<SizeUpdate> = sizer.compute(path).collect().await;
        let child_key = child.to_str().unwrap();
        let child_final = updates
            .iter()
            .find(|u| u.path == child_key && u.complete)
            .expect("child complete");
        assert_eq!(child_final.bytes, 500, "symlink target excluded");
    }

    /// A cancelled walk yields no completion items, across all child walks.
    #[tokio::test]
    async fn cancelled_walk_never_yields_completion() {
        let directory = tempfile::tempdir().expect("tree");
        let root = directory.path();
        for c in 0..20 {
            let child = root.join(format!("child-{c}"));
            std::fs::create_dir(&child).expect("create child");
            for f in 0..50 {
                std::fs::write(child.join(format!("f-{f}.bin")), vec![0u8; 64]).expect("write");
            }
        }
        let path = root.to_str().expect("valid unicode").to_string();
        let sizer = BackgroundSizer::new();
        let stream = sizer.compute(&path);
        sizer.cancel(&path);
        let updates: Vec<SizeUpdate> = stream.collect().await;
        assert!(
            updates.iter().all(|u| !u.complete),
            "cancelled: no completion, got {updates:?}"
        );
    }
}
