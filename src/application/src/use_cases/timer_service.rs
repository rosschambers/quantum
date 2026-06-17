//! Timer service use case.
//!
//! Core orchestration for the visual-timers feature. Owns the in-memory set of
//! timers plus their arming tasks, and drives persistence and broadcasting
//! through the domain ports (`Clock`, `TimerStore`, `TimerNotifier`,
//! `TimerBroadcast`). This crate depends only on `quantum_domain`; it never
//! touches infrastructure directly.
//!
//! An inner-`Arc` design lets a spawned arming task call back into the service
//! to re-arm a recurring timer after it fires.

use quantum_domain::{
    seconds_until_next, Clock, NotifyConfig, Point, TimeOfDay, Timer, TimerBroadcast, TimerError,
    TimerId, TimerKind, TimerNotifier, TimerSettings, TimerStatus, TimerStore, TimerStoreData,
    VisualConfig, WeekdaySet,
};
use rand::Rng;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::AbortHandle;

/// How a new timer's schedule is expressed by the caller. Resolved into a
/// concrete `TimerKind` against the clock at creation time.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimerStart {
    /// Fire once after `secs` seconds from now.
    Duration { secs: u64 },
    /// Fire once at the next occurrence of `time` (any day).
    At { time: TimeOfDay },
    /// Fire repeatedly at `time` on each day in `days`.
    Recurring { days: WeekdaySet, time: TimeOfDay },
}

/// Input for creating a timer. `visual`/`notify` default to the subsystem's
/// configured defaults when omitted.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateTimerSpec {
    pub label: String,
    pub start: TimerStart,
    pub visual: Option<VisualConfig>,
    pub notify: Option<NotifyConfig>,
}

/// Partial update for an existing timer. Each `Some` field replaces the
/// corresponding value; `None` leaves it unchanged. Supplying `time` (and, for
/// recurring timers, `days`) reschedules and re-arms the timer.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EditChanges {
    pub label: Option<String>,
    pub visual: Option<VisualConfig>,
    pub notify: Option<NotifyConfig>,
    pub time: Option<TimeOfDay>,
    pub days: Option<WeekdaySet>,
    pub scatter_pos: Option<Point>,
}

/// Orchestrates the timer lifecycle. Cheap to clone (it is a single `Arc`).
pub struct TimerService {
    inner: Arc<TimerServiceInner>,
}

struct TimerServiceInner {
    clock: Arc<dyn Clock>,
    store: Arc<dyn TimerStore>,
    notifier: Arc<dyn TimerNotifier>,
    broadcast: Arc<dyn TimerBroadcast>,
    state: Mutex<TimerServiceState>,
}

struct TimerServiceState {
    settings: TimerSettings,
    timers: HashMap<TimerId, Timer>,
    handles: HashMap<TimerId, AbortHandle>,
}

impl TimerService {
    pub fn new(
        clock: Arc<dyn Clock>,
        store: Arc<dyn TimerStore>,
        notifier: Arc<dyn TimerNotifier>,
        broadcast: Arc<dyn TimerBroadcast>,
    ) -> Self {
        Self {
            inner: Arc::new(TimerServiceInner {
                clock,
                store,
                notifier,
                broadcast,
                state: Mutex::new(TimerServiceState {
                    settings: TimerSettings::default(),
                    timers: HashMap::new(),
                    handles: HashMap::new(),
                }),
            }),
        }
    }

    /// Create, arm, persist, and broadcast a new timer.
    pub async fn create(&self, spec: CreateTimerSpec) -> Result<TimerId, TimerError> {
        self.inner.create(spec).await
    }

    /// Snapshot of the current settings and all timers.
    pub async fn list(&self) -> TimerStoreData {
        self.inner.list().await
    }

    /// Run a timer's completion logic immediately. Exposed to the crate so the
    /// daemon (and tests) can trigger a fire without waiting on the arming
    /// task; the normal path is the spawned sleep installed by `arm`. The
    /// daemon wiring lands in a later task, so allow it to be unused for now in
    /// non-test builds.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn fire(&self, id: TimerId) {
        self.inner.fire(id).await
    }

    /// Cancel and remove a timer.
    pub async fn cancel(&self, id: TimerId) -> Result<(), TimerError> {
        self.inner.cancel(id).await
    }

    /// Dismiss a timer. In v1 this is identical to removal.
    pub async fn dismiss(&self, id: TimerId) -> Result<(), TimerError> {
        self.inner.dismiss(id).await
    }

    /// Apply partial changes to a timer, rescheduling if its time changed.
    pub async fn edit(&self, id: TimerId, changes: EditChanges) -> Result<(), TimerError> {
        self.inner.edit(id, changes).await
    }

    /// Load persisted state and arm every still-relevant timer.
    pub async fn load_and_arm(&self) -> Result<(), TimerError> {
        self.inner.load_and_arm().await
    }
}

impl TimerServiceInner {
    /// Build a persistable snapshot from already-locked state.
    fn snapshot(state: &TimerServiceState) -> TimerStoreData {
        TimerStoreData {
            settings: state.settings.clone(),
            timers: state.timers.values().cloned().collect(),
        }
    }

    /// Pick an 8 hex-character id not already present in `existing`.
    fn generate_id(existing: &HashMap<TimerId, Timer>) -> TimerId {
        loop {
            let value: u32 = rand::thread_rng().gen();
            let id = TimerId::from(format!("{value:08x}"));
            if !existing.contains_key(&id) {
                return id;
            }
        }
    }

    /// Spawn (or replace) the arming task for `id`. The task sleeps until
    /// `fires_at_unix` then calls `fire`, which re-arms recurring timers. The
    /// caller already holds the state lock and passes it in so the handle is
    /// installed without re-locking. Any prior handle for `id` is aborted.
    fn arm(
        inner: &Arc<TimerServiceInner>,
        state: &mut TimerServiceState,
        id: TimerId,
        fires_at_unix: u64,
    ) {
        let delay = fires_at_unix.saturating_sub(inner.clock.now_unix());
        let inner_for_task = inner.clone();
        let id_for_task = id.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(delay)).await;
            inner_for_task.fire(id_for_task).await;
        });
        if let Some(previous) = state.handles.insert(id, handle.abort_handle()) {
            previous.abort();
        }
    }

    /// Persist the current snapshot then broadcast it. Persistence failures are
    /// logged, not propagated, so a transient disk error cannot wedge the
    /// in-memory subsystem or block a broadcast.
    async fn persist_and_broadcast(&self) {
        let data = {
            let state = self.state.lock().await;
            Self::snapshot(&state)
        };
        if let Err(error) = self.store.save(&data).await {
            tracing::warn!("timer persistence failed: {error}");
        }
        self.broadcast.publish(&data);
    }

    async fn create(self: &Arc<Self>, spec: CreateTimerSpec) -> Result<TimerId, TimerError> {
        let now = self.clock.now_unix();
        let civil = self.clock.local_civil();

        let kind = match spec.start {
            TimerStart::Duration { secs } => TimerKind::OneShot {
                end_unix: now + secs,
            },
            TimerStart::At { time } => {
                let delta =
                    seconds_until_next(civil.weekday, civil.secs_into_day, &WeekdaySet::all(), time)
                        .unwrap_or(0);
                TimerKind::OneShot {
                    end_unix: now + delta,
                }
            }
            TimerStart::Recurring { days, time } => {
                if days.is_empty() {
                    return Err(TimerError::EmptyWeekdaySet);
                }
                let delta =
                    seconds_until_next(civil.weekday, civil.secs_into_day, &days, time).unwrap_or(0);
                TimerKind::Recurring {
                    days,
                    time,
                    next_fire_unix: now + delta,
                }
            }
        };

        let id;
        {
            let mut state = self.state.lock().await;
            id = Self::generate_id(&state.timers);
            let visual = spec
                .visual
                .unwrap_or_else(|| state.settings.defaults_visual.clone());
            let notify = spec
                .notify
                .unwrap_or_else(|| state.settings.defaults_notify.clone());
            let timer = Timer {
                id: id.clone(),
                label: spec.label,
                kind,
                visual,
                notify,
                status: TimerStatus::Active,
                scatter_pos: None,
            };
            let fires_at = timer.fires_at_unix();
            state.timers.insert(id.clone(), timer);
            Self::arm(self, &mut state, id.clone(), fires_at);
        }

        self.persist_and_broadcast().await;
        Ok(id)
    }

    async fn list(&self) -> TimerStoreData {
        let state = self.state.lock().await;
        Self::snapshot(&state)
    }

    async fn fire(self: &Arc<Self>, id: TimerId) {
        let timer_snapshot = {
            let state = self.state.lock().await;
            match state.timers.get(&id) {
                Some(timer) => timer.clone(),
                None => return,
            }
        };

        self.notifier.notify_complete(&timer_snapshot).await;

        // Update state under the lock and capture whether (and when) to re-arm.
        // Crucially we do NOT arm here: the arming task that is running `fire`
        // has its own `AbortHandle` registered under `id`, and `arm` would
        // abort it. We must persist and broadcast to completion FIRST, then
        // re-arm last (see `recurring_fire_from_spawned_task_still_broadcasts`).
        let rearm_at: Option<u64> = {
            let mut state = self.state.lock().await;
            // Re-read: the timer may have been removed while the notifier ran,
            // and we need its current kind to decide expire-vs-rearm.
            let recurring = match state.timers.get(&id) {
                None => return,
                Some(timer) => match &timer.kind {
                    TimerKind::OneShot { .. } => None,
                    TimerKind::Recurring { days, time, .. } => Some((*days, *time)),
                },
            };
            match recurring {
                None => {
                    if let Some(timer) = state.timers.get_mut(&id) {
                        timer.status = TimerStatus::Expired;
                    }
                    state.handles.remove(&id);
                    None
                }
                Some((days, time)) => {
                    let civil = self.clock.local_civil();
                    let now = self.clock.now_unix();
                    let new_fire =
                        seconds_until_next(civil.weekday, civil.secs_into_day, &days, time)
                            .map(|delta| now + delta);
                    // Invariant: `seconds_until_next` returns `Some` for any
                    // non-empty day set, which a recurring timer always has.
                    debug_assert!(
                        days.is_empty() || new_fire.is_some(),
                        "seconds_until_next must yield Some for a non-empty day set"
                    );

                    let mut fires_at = None;
                    if let Some(timer) = state.timers.get_mut(&id) {
                        if let (Some(next), TimerKind::Recurring { next_fire_unix, .. }) =
                            (new_fire, &mut timer.kind)
                        {
                            *next_fire_unix = next;
                        }
                        timer.status = TimerStatus::Active;
                        fires_at = Some(timer.fires_at_unix());
                    }
                    fires_at
                }
            }
        };

        // Persist and broadcast to completion before touching the arming task.
        self.persist_and_broadcast().await;

        // Re-arm last. No `.await` may follow this in the recurring path:
        // `arm` aborts the prior handle for `id` — which is this very task — and
        // that abort is harmless only because `fire` returns immediately after.
        if let Some(fires_at) = rearm_at {
            let mut state = self.state.lock().await;
            Self::arm(self, &mut state, id, fires_at);
        }
    }

    async fn cancel(&self, id: TimerId) -> Result<(), TimerError> {
        {
            let mut state = self.state.lock().await;
            if state.timers.remove(&id).is_none() {
                return Err(TimerError::NotFound(id.to_string()));
            }
            if let Some(handle) = state.handles.remove(&id) {
                handle.abort();
            }
        }
        self.persist_and_broadcast().await;
        Ok(())
    }

    /// In v1 a dismiss is the same operation as a cancel: stop the arming task
    /// and remove the timer entirely.
    async fn dismiss(&self, id: TimerId) -> Result<(), TimerError> {
        self.cancel(id).await
    }

    async fn edit(self: &Arc<Self>, id: TimerId, changes: EditChanges) -> Result<(), TimerError> {
        let now = self.clock.now_unix();
        let civil = self.clock.local_civil();

        {
            let mut state = self.state.lock().await;
            let needs_rearm;
            let fires_at;
            {
                let Some(timer) = state.timers.get_mut(&id) else {
                    return Err(TimerError::NotFound(id.to_string()));
                };

                if let Some(label) = changes.label {
                    timer.label = label;
                }
                if let Some(visual) = changes.visual {
                    timer.visual = visual;
                }
                if let Some(notify) = changes.notify {
                    timer.notify = notify;
                }
                if let Some(scatter_pos) = changes.scatter_pos {
                    timer.scatter_pos = Some(scatter_pos);
                }

                if changes.time.is_some() || changes.days.is_some() {
                    match &mut timer.kind {
                        TimerKind::Recurring {
                            days,
                            time,
                            next_fire_unix,
                        } => {
                            if let Some(new_days) = changes.days {
                                *days = new_days;
                            }
                            if let Some(new_time) = changes.time {
                                *time = new_time;
                            }
                            if let Some(delta) = seconds_until_next(
                                civil.weekday,
                                civil.secs_into_day,
                                days,
                                *time,
                            ) {
                                *next_fire_unix = now + delta;
                            }
                            needs_rearm = true;
                        }
                        TimerKind::OneShot { end_unix } => {
                            // A one-shot ignores `days`; only a new `time`
                            // reschedules it, to the next occurrence (any day).
                            if let Some(new_time) = changes.time {
                                let delta = seconds_until_next(
                                    civil.weekday,
                                    civil.secs_into_day,
                                    &WeekdaySet::all(),
                                    new_time,
                                )
                                .unwrap_or(0);
                                *end_unix = now + delta;
                                needs_rearm = true;
                            } else {
                                needs_rearm = false;
                            }
                        }
                    }
                } else {
                    needs_rearm = false;
                }

                fires_at = timer.fires_at_unix();
            }

            if needs_rearm {
                Self::arm(self, &mut state, id.clone(), fires_at);
            }
        }

        self.persist_and_broadcast().await;
        Ok(())
    }

    async fn load_and_arm(self: &Arc<Self>) -> Result<(), TimerError> {
        let data = self.store.load().await?;
        let now = self.clock.now_unix();
        let civil = self.clock.local_civil();

        {
            let mut state = self.state.lock().await;
            state.settings = data.settings;
            // Start from a clean slate so a re-load is idempotent.
            for handle in std::mem::take(&mut state.handles).into_values() {
                handle.abort();
            }
            state.timers.clear();

            for mut timer in data.timers {
                let id = timer.id.clone();
                match timer.kind {
                    TimerKind::Recurring { days, time, .. } => {
                        if let Some(delta) =
                            seconds_until_next(civil.weekday, civil.secs_into_day, &days, time)
                        {
                            if let TimerKind::Recurring { next_fire_unix, .. } = &mut timer.kind {
                                *next_fire_unix = now + delta;
                            }
                        }
                        timer.status = TimerStatus::Active;
                        let fires_at = timer.fires_at_unix();
                        state.timers.insert(id.clone(), timer);
                        Self::arm(self, &mut state, id, fires_at);
                    }
                    TimerKind::OneShot { end_unix } => {
                        if end_unix > now {
                            timer.status = TimerStatus::Active;
                            state.timers.insert(id.clone(), timer);
                            Self::arm(self, &mut state, id, end_unix);
                        } else {
                            timer.status = TimerStatus::Expired;
                            state.timers.insert(id, timer);
                        }
                    }
                }
            }
        }

        let data = {
            let state = self.state.lock().await;
            Self::snapshot(&state)
        };
        self.broadcast.publish(&data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CreateTimerSpec, EditChanges, TimerService, TimerStart};
    use quantum_domain::{
        CivilNow, Clock, NotifyConfig, Point, TimerBroadcast, TimerError, TimerKind, TimerNotifier,
        TimerStore, TimerStoreData, Timer, TimerId, TimerStatus, TimeOfDay, VisualConfig, Weekday,
        WeekdaySet,
    };
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// Clock with interior mutability on `now` so a test can advance time
    /// between operations. `civil` is fixed; the recurring-timer arithmetic is
    /// pure on the civil projection, so advancing only `now` is sufficient to
    /// prove a re-arm produces a later instant.
    struct FakeClock {
        now: AtomicU64,
        civil: CivilNow,
    }

    impl FakeClock {
        fn set_now(&self, value: u64) {
            self.now.store(value, Ordering::SeqCst);
        }
    }

    impl Clock for FakeClock {
        fn now_unix(&self) -> u64 {
            self.now.load(Ordering::SeqCst)
        }
        fn local_civil(&self) -> CivilNow {
            self.civil
        }
    }

    struct FakeStore {
        data: Mutex<Option<TimerStoreData>>,
        save_count: AtomicUsize,
        /// When set, `save` yields to the scheduler before recording. This
        /// mimics the real `JsonTimerStore`, whose disk write yields, so a task
        /// aborted earlier in the same poll is cancelled at this point.
        yield_on_save: AtomicBool,
    }

    #[async_trait]
    impl TimerStore for FakeStore {
        async fn load(&self) -> Result<TimerStoreData, TimerError> {
            Ok(self.data.lock().unwrap().clone().unwrap_or_default())
        }
        async fn save(&self, data: &TimerStoreData) -> Result<(), TimerError> {
            if self.yield_on_save.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
            self.save_count.fetch_add(1, Ordering::SeqCst);
            *self.data.lock().unwrap() = Some(data.clone());
            Ok(())
        }
    }

    struct FakeNotifier {
        count: AtomicUsize,
    }

    #[async_trait]
    impl TimerNotifier for FakeNotifier {
        async fn notify_complete(&self, _timer: &Timer) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct FakeBroadcast {
        count: AtomicUsize,
        last: Mutex<Option<TimerStoreData>>,
    }

    impl TimerBroadcast for FakeBroadcast {
        fn publish(&self, data: &TimerStoreData) {
            self.count.fetch_add(1, Ordering::SeqCst);
            *self.last.lock().unwrap() = Some(data.clone());
        }
    }

    struct Fakes {
        clock: Arc<FakeClock>,
        store: Arc<FakeStore>,
        notifier: Arc<FakeNotifier>,
        broadcast: Arc<FakeBroadcast>,
    }

    fn build_service(now: u64, civil: CivilNow) -> (TimerService, Fakes) {
        let clock = Arc::new(FakeClock {
            now: AtomicU64::new(now),
            civil,
        });
        let store = Arc::new(FakeStore {
            data: Mutex::new(None),
            save_count: AtomicUsize::new(0),
            yield_on_save: AtomicBool::new(false),
        });
        let notifier = Arc::new(FakeNotifier {
            count: AtomicUsize::new(0),
        });
        let broadcast = Arc::new(FakeBroadcast {
            count: AtomicUsize::new(0),
            last: Mutex::new(None),
        });
        let service = TimerService::new(
            clock.clone(),
            store.clone(),
            notifier.clone(),
            broadcast.clone(),
        );
        (
            service,
            Fakes {
                clock,
                store,
                notifier,
                broadcast,
            },
        )
    }

    fn civil(weekday: Weekday, secs_into_day: u32) -> CivilNow {
        CivilNow {
            weekday,
            secs_into_day,
        }
    }

    fn one_shot_timer(id: &str, end_unix: u64, status: TimerStatus) -> Timer {
        Timer {
            id: TimerId::from(id),
            label: id.to_string(),
            kind: TimerKind::OneShot { end_unix },
            visual: VisualConfig::default(),
            notify: NotifyConfig::default(),
            status,
            scatter_pos: None,
        }
    }

    fn recurring_timer(
        id: &str,
        days: WeekdaySet,
        time: TimeOfDay,
        next_fire_unix: u64,
    ) -> Timer {
        Timer {
            id: TimerId::from(id),
            label: id.to_string(),
            kind: TimerKind::Recurring {
                days,
                time,
                next_fire_unix,
            },
            visual: VisualConfig::default(),
            notify: NotifyConfig::default(),
            status: TimerStatus::Active,
            scatter_pos: None,
        }
    }

    #[tokio::test]
    async fn create_duration_timer_persists_and_lists() {
        let (service, fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        let id = service
            .create(CreateTimerSpec {
                label: "Tea".to_string(),
                start: TimerStart::Duration { secs: 300 },
                visual: None,
                notify: None,
            })
            .await
            .expect("create");

        let listed = service.list().await;
        assert_eq!(listed.timers.len(), 1);
        let timer = &listed.timers[0];
        assert_eq!(timer.id, id);
        assert_eq!(timer.kind, TimerKind::OneShot { end_unix: 1_000_300 });
        assert!(fakes.store.save_count.load(Ordering::SeqCst) >= 1);
        assert!(fakes.broadcast.count.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn create_recurring_rejects_empty_days() {
        let (service, _fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        let result = service
            .create(CreateTimerSpec {
                label: "Standup".to_string(),
                start: TimerStart::Recurring {
                    days: WeekdaySet::from_days(&[]),
                    time: TimeOfDay::new(9, 0).unwrap(),
                },
                visual: None,
                notify: None,
            })
            .await;
        assert!(matches!(result, Err(TimerError::EmptyWeekdaySet)));
    }

    #[tokio::test]
    async fn create_at_resolves_future_instant() {
        // Monday 09:00 now, target 17:00 today -> 8 hours later.
        let (service, _fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        let id = service
            .create(CreateTimerSpec {
                label: "Meeting".to_string(),
                start: TimerStart::At {
                    time: TimeOfDay::new(17, 0).unwrap(),
                },
                visual: None,
                notify: None,
            })
            .await
            .expect("create");

        let listed = service.list().await;
        let timer = listed
            .timers
            .iter()
            .find(|t| t.id == id)
            .expect("timer present");
        assert_eq!(
            timer.kind,
            TimerKind::OneShot {
                end_unix: 1_000_000 + 8 * 3600
            }
        );
    }

    #[tokio::test]
    async fn fire_oneshot_marks_expired_and_notifies() {
        let (service, fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        let id = service
            .create(CreateTimerSpec {
                label: "Tea".to_string(),
                start: TimerStart::Duration { secs: 300 },
                visual: None,
                notify: None,
            })
            .await
            .expect("create");

        service.fire(id.clone()).await;

        let listed = service.list().await;
        let timer = listed
            .timers
            .iter()
            .find(|t| t.id == id)
            .expect("still listed");
        assert_eq!(timer.status, TimerStatus::Expired);
        assert_eq!(fakes.notifier.count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fire_recurring_rearms_and_stays_active() {
        let (service, fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        let id = service
            .create(CreateTimerSpec {
                label: "Standup".to_string(),
                start: TimerStart::Recurring {
                    days: WeekdaySet::from_days(&[Weekday::Monday]),
                    time: TimeOfDay::new(17, 0).unwrap(),
                },
                visual: None,
                notify: None,
            })
            .await
            .expect("create");

        let before = service
            .list()
            .await
            .timers
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.fires_at_unix())
            .expect("present");

        // Advance the clock so the recompute lands strictly later.
        fakes.clock.set_now(1_000_500);
        service.fire(id.clone()).await;

        let listed = service.list().await;
        let timer = listed
            .timers
            .iter()
            .find(|t| t.id == id)
            .expect("present");
        assert_eq!(timer.status, TimerStatus::Active);
        assert!(
            timer.fires_at_unix() > before,
            "next fire {} should be greater than {}",
            timer.fires_at_unix(),
            before
        );
        assert_eq!(fakes.notifier.count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancel_removes_and_notfound() {
        let (service, _fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        let id = service
            .create(CreateTimerSpec {
                label: "Tea".to_string(),
                start: TimerStart::Duration { secs: 300 },
                visual: None,
                notify: None,
            })
            .await
            .expect("create");

        service.cancel(id.clone()).await.expect("cancel");
        assert!(service.list().await.timers.is_empty());

        let missing = service.cancel(TimerId::from("nope")).await;
        assert!(matches!(missing, Err(TimerError::NotFound(_))));
    }

    #[tokio::test]
    async fn dismiss_removes() {
        let (service, _fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        let id = service
            .create(CreateTimerSpec {
                label: "Tea".to_string(),
                start: TimerStart::Duration { secs: 300 },
                visual: None,
                notify: None,
            })
            .await
            .expect("create");
        // Make it expired first.
        service.fire(id.clone()).await;

        service.dismiss(id.clone()).await.expect("dismiss");
        assert!(service.list().await.timers.is_empty());
    }

    #[tokio::test]
    async fn edit_label_persists() {
        let (service, _fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        let id = service
            .create(CreateTimerSpec {
                label: "Tea".to_string(),
                start: TimerStart::Duration { secs: 300 },
                visual: None,
                notify: None,
            })
            .await
            .expect("create");

        service
            .edit(
                id.clone(),
                EditChanges {
                    label: Some("Coffee".to_string()),
                    visual: None,
                    notify: None,
                    time: None,
                    days: None,
                    scatter_pos: None,
                },
            )
            .await
            .expect("edit");

        let listed = service.list().await;
        let timer = listed.timers.iter().find(|t| t.id == id).expect("present");
        assert_eq!(timer.label, "Coffee");
    }

    #[tokio::test]
    async fn edit_recurring_reschedules() {
        let (service, _fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        let id = service
            .create(CreateTimerSpec {
                label: "Standup".to_string(),
                start: TimerStart::Recurring {
                    days: WeekdaySet::from_days(&[Weekday::Monday]),
                    time: TimeOfDay::new(17, 0).unwrap(),
                },
                visual: None,
                notify: None,
            })
            .await
            .expect("create");

        // Change the time to 10:00; from Monday 09:00 that is 1 hour out.
        service
            .edit(
                id.clone(),
                EditChanges {
                    label: None,
                    visual: None,
                    notify: None,
                    time: Some(TimeOfDay::new(10, 0).unwrap()),
                    days: None,
                    scatter_pos: None,
                },
            )
            .await
            .expect("edit");

        let listed = service.list().await;
        let timer = listed.timers.iter().find(|t| t.id == id).expect("present");
        assert_eq!(timer.fires_at_unix(), 1_000_000 + 3600);
    }

    #[tokio::test]
    async fn edit_scatter_pos_does_not_rearm() {
        let (service, _fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        let id = service
            .create(CreateTimerSpec {
                label: "Standup".to_string(),
                start: TimerStart::Recurring {
                    days: WeekdaySet::from_days(&[Weekday::Monday]),
                    time: TimeOfDay::new(17, 0).unwrap(),
                },
                visual: None,
                notify: None,
            })
            .await
            .expect("create");

        let before = service
            .list()
            .await
            .timers
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.fires_at_unix())
            .expect("present");

        service
            .edit(
                id.clone(),
                EditChanges {
                    label: None,
                    visual: None,
                    notify: None,
                    time: None,
                    days: None,
                    scatter_pos: Some(Point { x: 5.0, y: 6.0 }),
                },
            )
            .await
            .expect("edit");

        let listed = service.list().await;
        let timer = listed.timers.iter().find(|t| t.id == id).expect("present");
        assert_eq!(timer.fires_at_unix(), before);
        assert_eq!(timer.scatter_pos, Some(Point { x: 5.0, y: 6.0 }));
    }

    #[tokio::test]
    async fn load_and_arm_expires_past_oneshot() {
        let (service, fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        *fakes.store.data.lock().unwrap() = Some(TimerStoreData {
            settings: Default::default(),
            timers: vec![one_shot_timer("past", 999_000, TimerStatus::Active)],
        });

        service.load_and_arm().await.expect("load");

        let listed = service.list().await;
        let timer = listed
            .timers
            .iter()
            .find(|t| t.id == TimerId::from("past"))
            .expect("present");
        assert_eq!(timer.status, TimerStatus::Expired);
        assert_eq!(fakes.notifier.count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn load_and_arm_arms_future_oneshot() {
        let (service, _fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        *_fakes.store.data.lock().unwrap() = Some(TimerStoreData {
            settings: Default::default(),
            timers: vec![one_shot_timer("future", 1_000_500, TimerStatus::Active)],
        });

        service.load_and_arm().await.expect("load");

        let listed = service.list().await;
        let timer = listed
            .timers
            .iter()
            .find(|t| t.id == TimerId::from("future"))
            .expect("present");
        assert_eq!(timer.status, TimerStatus::Active);
        assert_eq!(timer.kind, TimerKind::OneShot { end_unix: 1_000_500 });
    }

    #[tokio::test]
    async fn load_and_arm_rearms_recurring() {
        let (service, _fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        *_fakes.store.data.lock().unwrap() = Some(TimerStoreData {
            settings: Default::default(),
            timers: vec![recurring_timer(
                "weekly",
                WeekdaySet::from_days(&[Weekday::Monday]),
                TimeOfDay::new(17, 0).unwrap(),
                42, // stale next_fire from a previous run
            )],
        });

        service.load_and_arm().await.expect("load");

        let listed = service.list().await;
        let timer = listed
            .timers
            .iter()
            .find(|t| t.id == TimerId::from("weekly"))
            .expect("present");
        assert_eq!(timer.status, TimerStatus::Active);
        // Monday 09:00 -> 17:00 today is 8 hours out.
        assert_eq!(timer.fires_at_unix(), 1_000_000 + 8 * 3600);
    }

    /// Regression: a recurring fire that runs inside the arming task (whose
    /// `AbortHandle` is registered under the timer id) must still broadcast.
    ///
    /// This drives `fire` from a spawned task whose handle is registered under
    /// the id exactly as `arm` does in production. With a `FakeStore` that
    /// yields inside `save`, the old ordering (re-arm before persist) aborted
    /// the running task at the save yield point, so `broadcast.publish` never
    /// ran. The fix persists and broadcasts before re-arming.
    #[tokio::test]
    async fn recurring_fire_from_spawned_task_still_broadcasts() {
        let (service, fakes) = build_service(1_000_000, civil(Weekday::Monday, 9 * 3600));
        fakes.store.yield_on_save.store(true, Ordering::SeqCst);

        let id = service
            .create(CreateTimerSpec {
                label: "Standup".to_string(),
                start: TimerStart::Recurring {
                    days: WeekdaySet::from_days(&[Weekday::Monday]),
                    time: TimeOfDay::new(17, 0).unwrap(),
                },
                visual: None,
                notify: None,
            })
            .await
            .expect("create");

        let before_publishes = fakes.broadcast.count.load(Ordering::SeqCst);
        let before_fire = service
            .list()
            .await
            .timers
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.fires_at_unix())
            .expect("present");

        // Advance the clock so the recompute lands strictly later.
        fakes.clock.set_now(1_000_500);

        let service = Arc::new(service);
        let service_for_task = service.clone();
        let id_for_task = id.clone();

        // Hold the state lock so the spawned task blocks on its first
        // `state.lock().await`. While holding it, register the task's handle
        // under the id (mimicking `arm`) and abort the original arming task so
        // only our task remains. Releasing the lock lets `fire` proceed.
        let join = {
            let mut state = service.inner.state.lock().await;
            let handle =
                tokio::spawn(async move { service_for_task.fire(id_for_task).await });
            if let Some(previous) = state.handles.insert(id.clone(), handle.abort_handle()) {
                previous.abort();
            }
            handle
        };

        // Under the old ordering the task is aborted at the save yield point, so
        // this returns a cancellation error; under the fix it completes. Either
        // way we then assert on the observable broadcast.
        let _ = join.await;

        assert!(
            fakes.broadcast.count.load(Ordering::SeqCst) > before_publishes,
            "recurring fire must broadcast after re-arming (publishes: {} -> {})",
            before_publishes,
            fakes.broadcast.count.load(Ordering::SeqCst)
        );

        let listed = service.list().await;
        let timer = listed.timers.iter().find(|t| t.id == id).expect("present");
        assert_eq!(timer.status, TimerStatus::Active);
        assert!(
            timer.fires_at_unix() > before_fire,
            "next fire {} should be greater than {}",
            timer.fires_at_unix(),
            before_fire
        );
    }
}
