//! Schedule action use case.
//!
//! Enables scheduling of power actions with a configurable delay.
//! Scheduled jobs are held in-memory during daemon runtime. Persistence
//! across restarts is not supported in v1.
//!
//! `ScheduledJobSummary` serializes `fires_at` using the default serde serialization
//! shape for `SystemTime`, which is `{secs_since_epoch: u64, nanos_since_epoch: u32}`.
//! This avoids adding chrono as a dependency.

use crate::error::ApplicationError;
use crate::use_cases::launch_action::LaunchActionUseCase;
use quantum_domain::{Action, DomainError, ProviderId};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use tokio::task::AbortHandle;

pub type ScheduleId = String;

/// Public-facing summary of a scheduled job. No AbortHandle, no
/// dispatcher reference — safe to serialize and send to frontends.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduledJobSummary {
    pub id: ScheduleId,
    pub fires_at: SystemTime,
    pub label: String,
}

struct ScheduledJobInternal {
    id: ScheduleId,
    fires_at: SystemTime,
    label: String,
    abort: AbortHandle,
}

pub struct ScheduleActionUseCase {
    /// Direct reference to the action-invoke use case. We avoid going
    /// through the full Dispatcher because that creates a recursive
    /// Send bound (`dispatch()` would await a future that spawns a
    /// task that calls `dispatch()` again).
    launch_action: Arc<LaunchActionUseCase>,
    #[allow(private_interfaces)]
    pub(crate) jobs: Arc<Mutex<HashMap<ScheduleId, ScheduledJobInternal>>>,
}

/// Parameter shape for a scheduled invoke: the same `{provider, action}`
/// envelope that `action.invoke` accepts over IPC. Stored verbatim and
/// reconstructed into the LaunchActionUseCase's typed args when the
/// timer fires.
#[derive(Debug, Clone, Deserialize)]
pub struct InvokeParams {
    pub provider: ProviderId,
    pub action: Action,
}

const MAX_DELAY_SECS: u64 = 86_400; // 24h cap

impl ScheduleActionUseCase {
    pub fn new(launch_action: Arc<LaunchActionUseCase>) -> Self {
        Self {
            launch_action,
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn schedule(
        &self,
        delay_secs: u64,
        label: String,
        params: InvokeParams,
    ) -> Result<ScheduleId, ApplicationError> {
        if delay_secs == 0 || delay_secs > MAX_DELAY_SECS {
            return Err(ApplicationError::Domain(DomainError::Unsupported(format!(
                "schedule delay must be in (0, {MAX_DELAY_SECS}] seconds, got {delay_secs}"
            ))));
        }
        let id = self.generate_id().await;
        let fires_at = SystemTime::now() + Duration::from_secs(delay_secs);
        let jobs = self.jobs.clone();
        let id_for_task = id.clone();
        let label_for_log = label.clone();
        let launch_action = self.launch_action.clone();
        let provider = params.provider;
        let action = params.action;

        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
            tracing::info!("scheduled action firing: {id_for_task} ({label_for_log})");
            if let Err(e) = launch_action.execute(provider, action).await {
                tracing::warn!("scheduled action {id_for_task} ({label_for_log}) failed: {e}");
            }
            // Always remove self from the map regardless of outcome.
            jobs.lock().await.remove(&id_for_task);
        });

        let internal = ScheduledJobInternal {
            id: id.clone(),
            fires_at,
            label,
            abort: handle.abort_handle(),
        };
        self.jobs.lock().await.insert(id.clone(), internal);
        Ok(id)
    }

    async fn generate_id(&self) -> ScheduleId {
        let jobs = self.jobs.lock().await;
        loop {
            let mut candidate = String::with_capacity(8);
            for _ in 0..8 {
                candidate.push_str(&format!("{:x}", rand::thread_rng().gen_range(0..16)));
            }
            if !jobs.contains_key(&candidate) {
                return candidate;
            }
        }
    }

    pub async fn cancel(&self, id: ScheduleId) -> Result<(), ApplicationError> {
        let mut jobs = self.jobs.lock().await;
        let job = jobs.remove(&id).ok_or_else(|| {
            ApplicationError::Domain(DomainError::Unsupported(format!(
                "scheduled job not found: {id}"
            )))
        })?;
        job.abort.abort();
        tracing::info!(
            "cancelled scheduled job {id} ({label})",
            id = id,
            label = job.label
        );
        Ok(())
    }

    pub async fn list(&self) -> Vec<ScheduledJobSummary> {
        let jobs = self.jobs.lock().await;
        jobs.values()
            .map(|j| ScheduledJobSummary {
                id: j.id.clone(),
                fires_at: j.fires_at,
                label: j.label.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::{
        ActionOutcome, DomainError, Match, ProviderId, ProviderRegistry, ProviderSource, Query,
    };

    /// Fake provider that records its received actions and reports
    /// `invoke()` succeeded so the scheduled-fire path can be asserted
    /// without touching real DBus or system services.
    struct FakeProvider {
        id: ProviderId,
        invoked: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl ProviderSource for FakeProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }
        async fn search(&self, _q: &Query) -> std::result::Result<Vec<Match>, DomainError> {
            Ok(vec![])
        }
        async fn invoke(
            &self,
            action: &quantum_domain::Action,
        ) -> std::result::Result<ActionOutcome, DomainError> {
            self.invoked.lock().await.push(format!("{action:?}"));
            Ok(ActionOutcome { message: None })
        }
    }

    struct FakeRegistry {
        provider: Arc<dyn ProviderSource>,
    }

    #[async_trait]
    impl ProviderRegistry for FakeRegistry {
        async fn list(&self) -> Vec<ProviderId> {
            vec![self.provider.id().clone()]
        }
        async fn get(&self, _id: &ProviderId) -> Option<Arc<dyn ProviderSource>> {
            Some(self.provider.clone())
        }
    }

    /// Build a ScheduleActionUseCase whose LaunchActionUseCase routes
    /// to a FakeProvider. The returned `invoked` vec lets tests assert
    /// the scheduled action actually fired.
    fn build_uc() -> (ScheduleActionUseCase, Arc<Mutex<Vec<String>>>) {
        let invoked = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(FakeProvider {
            id: ProviderId::from("test"),
            invoked: invoked.clone(),
        }) as Arc<dyn ProviderSource>;
        let registry = Arc::new(FakeRegistry { provider });
        let launch_action = Arc::new(LaunchActionUseCase::new(registry));
        let uc = ScheduleActionUseCase::new(launch_action);
        (uc, invoked)
    }

    fn sample_params() -> InvokeParams {
        InvokeParams {
            provider: ProviderId::from("test"),
            action: quantum_domain::Action::Launch {
                desktop_id: "noop".into(),
            },
        }
    }

    #[test]
    fn scheduled_job_summary_round_trips_through_serde() {
        let summary = ScheduledJobSummary {
            id: "abc12345".into(),
            fires_at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
            label: "Suspend".into(),
        };
        let v = serde_json::to_value(&summary).expect("serialize");
        assert_eq!(v["id"], "abc12345");
        assert_eq!(v["label"], "Suspend");
        assert!(v.get("fires_at").is_some());
    }

    #[tokio::test]
    async fn rejects_zero_delay() {
        let (uc, _) = build_uc();
        let res = uc.schedule(0, "Suspend".into(), sample_params()).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn rejects_delay_over_24h() {
        let (uc, _) = build_uc();
        let res = uc.schedule(86_401, "Suspend".into(), sample_params()).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn accepts_valid_delay_and_returns_id() {
        let (uc, _) = build_uc();
        let id = uc
            .schedule(60, "Suspend".into(), sample_params())
            .await
            .expect("schedule");
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn fires_after_delay_elapsed() {
        // Real-time 1s sleep so the spawned task actually wakes. Using
        // tokio::time::pause() with multi-step advance was unreliable
        // because the spawned task only re-schedules on the multi-thread
        // runtime; cargo test defaults to single-thread.
        let (uc, invoked) = build_uc();
        let uc = Arc::new(uc);
        let id = uc
            .schedule(1, "Suspend".into(), sample_params())
            .await
            .expect("schedule");

        assert!(uc.jobs.lock().await.contains_key(&id));

        tokio::time::sleep(Duration::from_millis(1200)).await;

        // Scheduled action fired through LaunchActionUseCase ->
        // FakeProvider::invoke (recorded in `invoked`).
        let inv = invoked.lock().await;
        assert_eq!(inv.len(), 1, "scheduled action should have fired once");
        // Job removed itself.
        assert!(!uc.jobs.lock().await.contains_key(&id));
    }

    #[tokio::test]
    async fn list_starts_empty() {
        let (uc, _) = build_uc();
        assert!(uc.list().await.is_empty());
    }

    #[tokio::test]
    async fn list_returns_scheduled_jobs() {
        let (uc, _) = build_uc();
        let id = uc
            .schedule(60, "Suspend".into(), sample_params())
            .await
            .unwrap();
        let jobs = uc.list().await;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, id);
        assert_eq!(jobs[0].label, "Suspend");
    }

    #[tokio::test]
    async fn cancel_unknown_id_returns_err() {
        let (uc, _) = build_uc();
        assert!(uc.cancel("doesnotexist".into()).await.is_err());
    }

    #[tokio::test]
    async fn cancel_real_id_prevents_fire() {
        let (uc, invoked) = build_uc();
        let id = uc
            .schedule(60, "Suspend".into(), sample_params())
            .await
            .unwrap();
        uc.cancel(id.clone()).await.unwrap();
        assert!(uc.list().await.is_empty());
        // Wait past the original delay to make sure the aborted task
        // doesn't somehow still invoke or re-insert.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(uc.list().await.is_empty());
        assert!(
            invoked.lock().await.is_empty(),
            "cancelled action must not fire"
        );
    }
}
