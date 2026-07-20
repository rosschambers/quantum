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

/// The upper bound on how many directory sizes the [`SizeCache`] retains before
/// it starts evicting the least recently used entry.
const MAX_CACHE_ENTRIES: usize = 4096;

/// A single cached directory size together with the modification time it was
/// computed against and the access counter value of its most recent use.
struct CacheEntry {
    /// The directory's modification time when its size was recorded; a later
    /// lookup with a different mtime is treated as a miss.
    mtime: std::time::SystemTime,
    /// The recursively summed size in bytes.
    bytes: u64,
    /// The [`SizeCache::counter`] value at this entry's most recent access,
    /// used to pick the least recently used entry for eviction.
    last_access: u64,
}

/// A bounded, least-recently-used in-memory cache of recursive directory sizes
/// keyed by directory path. Entries are validated against the directory's
/// modification time, and the cache never holds more than its capacity: an
/// over-capacity insert first evicts the least recently used entry.
///
/// The sizer walk consults this cache before walking each child directory and
/// repopulates it whenever a child is walked to completion.
struct SizeCache {
    entries: std::collections::HashMap<String, CacheEntry>,
    /// A monotonic counter bumped on every get-hit and insert; the current
    /// value stamps an entry's `last_access` to order entries by recency.
    counter: u64,
    /// The maximum number of entries retained before eviction begins.
    capacity: usize,
}

impl Default for SizeCache {
    fn default() -> Self {
        Self::with_capacity(MAX_CACHE_ENTRIES)
    }
}

impl SizeCache {
    /// Construct an empty cache that retains at most `capacity` entries. The
    /// public [`Default`] impl uses [`MAX_CACHE_ENTRIES`]; tests use a small
    /// capacity to exercise eviction cheaply.
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            counter: 0,
            capacity,
        }
    }

    /// Return the cached size for `path` only when an entry exists AND its
    /// recorded modification time equals `current_mtime`. On such a hit the
    /// access counter is bumped and the entry's `last_access` updated so it
    /// counts as most recently used; a missing entry or an mtime mismatch
    /// returns `None`. A stale entry is left in place for a later insert to
    /// overwrite rather than being removed here.
    fn get(&mut self, path: &str, current_mtime: std::time::SystemTime) -> Option<u64> {
        let entry = self.entries.get_mut(path)?;
        if entry.mtime != current_mtime {
            return None;
        }
        self.counter += 1;
        entry.last_access = self.counter;
        Some(entry.bytes)
    }

    /// Record `bytes` as the size of `path` at modification time `mtime`. An
    /// existing entry for `path` is overwritten with a fresh `last_access`.
    /// Inserting a NEW key that would exceed the capacity first evicts the entry
    /// with the smallest `last_access` (the least recently used).
    fn insert(&mut self, path: String, mtime: std::time::SystemTime, bytes: u64) {
        self.counter += 1;
        let is_new_key = !self.entries.contains_key(&path);
        if is_new_key && self.entries.len() >= self.capacity {
            self.evict_least_recently_used();
        }
        self.entries.insert(
            path,
            CacheEntry {
                mtime,
                bytes,
                last_access: self.counter,
            },
        );
    }

    /// Remove the entry with the smallest `last_access`. The scan is linear in
    /// the number of entries but only runs on an over-capacity insert.
    fn evict_least_recently_used(&mut self) {
        let victim = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(path, _)| path.clone());
        if let Some(path) = victim {
            self.entries.remove(&path);
        }
    }
}

/// A [`RecursiveSizer`] that walks directory trees on a blocking thread and
/// streams throttled progress, honouring a per-path cancellation flag.
#[derive(Default)]
pub struct BackgroundSizer {
    cancellations: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    /// Recursive directory sizes already computed, reused when a child's mtime
    /// is unchanged so an unchanged folder is served instantly without a walk.
    cache: Arc<Mutex<SizeCache>>,
}

impl BackgroundSizer {
    /// Construct a sizer with an empty cancellation registry and size cache.
    pub fn new() -> Self {
        Self {
            cancellations: Arc::new(Mutex::new(HashMap::new())),
            cache: Arc::new(Mutex::new(SizeCache::default())),
        }
    }
}

/// Enumerate `dir`'s immediate children and recursively size each child
/// directory, emitting updates keyed by the child's path. Regular files and
/// symlinks directly in `dir` are ignored. Stops early on cancellation or when
/// the consumer drops the stream.
fn walk_and_emit(
    dir: &str,
    cancel: &AtomicBool,
    updates: &UpdateSender<SizeUpdate>,
    cache: &Mutex<SizeCache>,
) {
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

        // The child's own directory mtime validates any cached size. A failed
        // `modified()` read (unsupported filesystem, permissions) degrades to a
        // cache miss: the child is walked and never cached.
        let mtime = metadata.modified().ok();

        // A cache hit serves the recorded size instantly, without a walk. A
        // poisoned lock is treated as a miss rather than panicking.
        let cached = mtime.and_then(|current_mtime| {
            cache
                .lock()
                .ok()
                .and_then(|mut cache| cache.get(&key, current_mtime))
        });
        if let Some(bytes) = cached {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let hit = SizeUpdate {
                path: key.clone(),
                bytes,
                complete: true,
            };
            if updates.send(hit).is_err() {
                return;
            }
            continue;
        }

        match size_child(&child_path, &key, cancel, updates) {
            // A completed walk emitted its final update; record it so an
            // unchanged repeat is served from cache. Only cache when the mtime
            // read succeeded, so a cached entry always has a validator.
            Some(total) => {
                if let Some(current_mtime) = mtime {
                    if let Ok(mut cache) = cache.lock() {
                        cache.insert(key.clone(), current_mtime, total);
                    }
                }
            }
            // Cancelled or the consumer dropped the stream: stop the whole walk
            // and cache nothing.
            None => return,
        }
    }
}

/// Recursively sum every regular file beneath `root`, emitting throttled
/// progress and a final completion keyed by `key`. Returns `Some(total)` when
/// the walk completed and emitted its final update, so the caller can cache the
/// size; returns `None` when the walk was cancelled or the consumer dropped the
/// stream, so the caller stops and caches nothing.
fn size_child(
    root: &std::path::Path,
    key: &str,
    cancel: &AtomicBool,
    updates: &UpdateSender<SizeUpdate>,
) -> Option<u64> {
    let mut total: u64 = 0;
    let mut last_emit = Instant::now();
    let mut pending = vec![root.to_path_buf()];

    while let Some(directory) = pending.pop() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                return None;
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
                        return None;
                    }
                    last_emit = Instant::now();
                }
            }
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let sent = updates
        .send(SizeUpdate {
            path: key.to_string(),
            bytes: total,
            complete: true,
        })
        .is_ok();
    if sent {
        Some(total)
    } else {
        None
    }
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
        let walk_cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            walk_and_emit(&walk_path, &walk_flag, &update_sender, &walk_cache);
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

    /// Find the final (`complete`) size emitted for `key`, if any.
    fn final_bytes_for(updates: &[SizeUpdate], key: &str) -> Option<u64> {
        updates
            .iter()
            .find(|u| u.path == key && u.complete)
            .map(|u| u.bytes)
    }

    /// Sizing the same unchanged tree twice serves the child's size from the
    /// cache on the second pass: after a first full walk records `a`'s size, we
    /// grow a file NESTED beneath `a` (so `a`'s OWN directory mtime does not
    /// change), then size again. The second pass reports `a`'s ORIGINAL bytes,
    /// proving it was served from cache rather than re-walked. The test asserts
    /// its own precondition — that `a`'s mtime is unchanged by the nested edit —
    /// so a filesystem that behaves otherwise fails loudly rather than silently.
    #[tokio::test]
    async fn second_compute_of_unchanged_tree_serves_cache() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let root = directory.path();

        let a = root.join("a");
        std::fs::create_dir(&a).expect("create a");
        let nested = a.join("nested");
        std::fs::create_dir(&nested).expect("create a/nested");
        let nested_file = nested.join("f");
        std::fs::write(&nested_file, vec![0u8; 1000]).expect("write a/nested/f");

        let a_key = a.to_str().expect("valid unicode").to_string();
        let path = root.to_str().expect("valid unicode").to_string();

        let a_mtime_before = std::fs::metadata(&a)
            .expect("a metadata")
            .modified()
            .expect("a mtime");

        let sizer = BackgroundSizer::new();
        let first: Vec<SizeUpdate> = sizer.compute(&path).collect().await;
        let original_bytes = final_bytes_for(&first, &a_key).expect("a sized on first pass");
        assert_eq!(original_bytes, 1000, "a is the sum of its nested file");

        // Grow the file nested beneath `a`. This changes the nested file's size
        // but must NOT change `a`'s own directory mtime, because `a`'s direct
        // entries are unchanged.
        std::fs::write(&nested_file, vec![0u8; 5000]).expect("grow a/nested/f");

        let a_mtime_after = std::fs::metadata(&a)
            .expect("a metadata")
            .modified()
            .expect("a mtime");
        assert_eq!(
            a_mtime_before, a_mtime_after,
            "precondition: editing a nested file leaves a's own mtime unchanged"
        );

        let second: Vec<SizeUpdate> = sizer.compute(&path).collect().await;
        let cached_bytes = final_bytes_for(&second, &a_key).expect("a reported on second pass");
        assert_eq!(
            cached_bytes, original_bytes,
            "unchanged a served the stale cached size, not re-walked"
        );
    }

    /// Changing a child's OWN directory mtime invalidates its cache entry: after
    /// a first walk caches `a`, we add a NEW file directly inside `a` (which
    /// bumps `a`'s mtime), then size again. The second pass re-walks and reports
    /// the larger size including the new file.
    #[tokio::test]
    async fn changed_child_mtime_invalidates() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let root = directory.path();

        let a = root.join("a");
        std::fs::create_dir(&a).expect("create a");
        std::fs::write(a.join("f1"), vec![0u8; 1000]).expect("write a/f1");

        let a_key = a.to_str().expect("valid unicode").to_string();
        let path = root.to_str().expect("valid unicode").to_string();

        let sizer = BackgroundSizer::new();
        let first: Vec<SizeUpdate> = sizer.compute(&path).collect().await;
        assert_eq!(final_bytes_for(&first, &a_key), Some(1000));

        // A new file directly in `a` changes a's own directory mtime.
        std::fs::write(a.join("f2"), vec![0u8; 2000]).expect("write a/f2");

        let second: Vec<SizeUpdate> = sizer.compute(&path).collect().await;
        assert_eq!(
            final_bytes_for(&second, &a_key),
            Some(3000),
            "a was re-walked because its mtime changed"
        );
    }

    /// A child never sized before is walked and reported normally (the cache
    /// starts empty, so the first sizing is always a miss).
    #[tokio::test]
    async fn new_child_not_in_cache_is_walked() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let root = directory.path();

        let a = root.join("a");
        std::fs::create_dir(&a).expect("create a");
        std::fs::write(a.join("f1"), vec![0u8; 1234]).expect("write a/f1");

        let a_key = a.to_str().expect("valid unicode").to_string();
        let path = root.to_str().expect("valid unicode").to_string();

        let sizer = BackgroundSizer::new();
        let updates: Vec<SizeUpdate> = sizer.compute(&path).collect().await;
        assert_eq!(
            final_bytes_for(&updates, &a_key),
            Some(1234),
            "a never-seen child is walked and reported"
        );
    }

    /// A cancelled walk must not populate the cache: after cancelling a first
    /// sizing before it completes, a second sizing of the same tree must walk
    /// the child (and emit a completion), proving nothing was cached from the
    /// aborted walk.
    #[tokio::test]
    async fn cancelled_walk_does_not_cache() {
        let directory = tempfile::tempdir().expect("create temporary directory");
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
        let first: Vec<SizeUpdate> = stream.collect().await;
        assert!(
            first.iter().all(|u| !u.complete),
            "cancelled first pass yields no completion"
        );

        // A fresh sizing must actually walk the children (not serve them from a
        // cache the cancelled walk was forbidden to populate).
        let second: Vec<SizeUpdate> = sizer.compute(&path).collect().await;
        assert!(
            second.iter().any(|u| u.complete),
            "second pass walks the tree, so completions appear"
        );
    }
}

#[cfg(test)]
mod size_cache_tests {
    use super::{SizeCache, MAX_CACHE_ENTRIES};
    use std::time::{Duration, SystemTime};

    /// Fabricate a distinct `SystemTime` from a whole-second offset so tests do
    /// not depend on any real clock.
    fn mtime_at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    /// A get against an empty cache misses.
    #[test]
    fn get_on_empty_cache_returns_none() {
        let mut cache = SizeCache::default();
        assert_eq!(cache.get("/some/path", mtime_at(1)), None);
    }

    /// Inserting then getting with the SAME mtime returns the stored size.
    #[test]
    fn get_after_insert_with_matching_mtime_returns_bytes() {
        let mut cache = SizeCache::default();
        cache.insert("/a".to_string(), mtime_at(10), 4096);
        assert_eq!(cache.get("/a", mtime_at(10)), Some(4096));
    }

    /// A get with an mtime that differs from the stored one misses, because the
    /// directory changed since the size was recorded.
    #[test]
    fn get_with_different_mtime_returns_none() {
        let mut cache = SizeCache::default();
        cache.insert("/a".to_string(), mtime_at(10), 4096);
        assert_eq!(cache.get("/a", mtime_at(11)), None);
    }

    /// Inserting a new key beyond capacity evicts the least-recently-used entry.
    /// With capacity two, inserting A then B then C evicts A (the oldest touch),
    /// leaving B and C present.
    #[test]
    fn insert_past_capacity_evicts_least_recently_used() {
        let mut cache = SizeCache::with_capacity(2);
        cache.insert("/a".to_string(), mtime_at(1), 100);
        cache.insert("/b".to_string(), mtime_at(2), 200);
        cache.insert("/c".to_string(), mtime_at(3), 300);

        assert_eq!(cache.get("/a", mtime_at(1)), None, "a was evicted");
        assert_eq!(cache.get("/b", mtime_at(2)), Some(200), "b survives");
        assert_eq!(cache.get("/c", mtime_at(3)), Some(300), "c survives");
    }

    /// A get updates recency, so it, not raw insertion order, decides the
    /// eviction victim. With capacity two, insert A then B, then get A (making B
    /// the least recently used), then insert C: B is evicted and A survives.
    #[test]
    fn get_updates_recency_so_it_changes_the_eviction_victim() {
        let mut cache = SizeCache::with_capacity(2);
        cache.insert("/a".to_string(), mtime_at(1), 100);
        cache.insert("/b".to_string(), mtime_at(2), 200);

        // Touch A so B becomes the least recently used entry.
        assert_eq!(cache.get("/a", mtime_at(1)), Some(100));

        cache.insert("/c".to_string(), mtime_at(3), 300);

        assert_eq!(cache.get("/b", mtime_at(2)), None, "b was evicted, not a");
        assert_eq!(
            cache.get("/a", mtime_at(1)),
            Some(100),
            "a survives via recency"
        );
        assert_eq!(cache.get("/c", mtime_at(3)), Some(300), "c survives");
    }

    /// The default constructor uses the module-wide capacity constant: filling
    /// exactly `MAX_CACHE_ENTRIES` keeps every key, and one more distinct key
    /// evicts the least recently used (here the first-inserted, untouched key).
    #[test]
    fn default_uses_max_cache_entries_capacity() {
        let mut cache = SizeCache::default();
        for index in 0..MAX_CACHE_ENTRIES {
            cache.insert(
                format!("/path/{index}"),
                mtime_at(index as u64),
                index as u64,
            );
        }
        // At exactly capacity, the last-inserted key is present and nothing was
        // evicted (checking without touching /path/0's recency).
        let last = MAX_CACHE_ENTRIES - 1;
        assert_eq!(
            cache.get(&format!("/path/{last}"), mtime_at(last as u64)),
            Some(last as u64)
        );

        // One more distinct key tips over capacity and evicts the oldest,
        // least-recently-used entry, which is the never-accessed /path/0.
        cache.insert(
            "/path/overflow".to_string(),
            mtime_at(MAX_CACHE_ENTRIES as u64),
            999,
        );
        assert_eq!(cache.get("/path/0", mtime_at(0)), None, "oldest evicted");
    }
}
