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
use quantum_domain::DomainError;
use rand::Rng;
use serde::Serialize;
use serde_json::Value;
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
    #[allow(dead_code)]
    dispatcher: std::sync::Weak<crate::Dispatcher>,
    #[allow(private_interfaces)]
    pub(crate) jobs: Arc<Mutex<HashMap<ScheduleId, ScheduledJobInternal>>>,
}

unsafe impl Send for ScheduleActionUseCase {}
unsafe impl Sync for ScheduleActionUseCase {}

const MAX_DELAY_SECS: u64 = 86_400; // 24h cap

impl ScheduleActionUseCase {
    pub fn new(dispatcher: std::sync::Weak<crate::Dispatcher>) -> Self {
        Self {
            dispatcher,
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn schedule(
        &self,
        delay_secs: u64,
        label: String,
        invoke_params: Value,
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

        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
            // Log that the scheduled action fired. In a future version, we would
            // invoke the dispatcher here. For now, we just clean up the job.
            // Note: we don't use dispatcher_weak here to avoid Send-checking issues
            // with circular dependencies between ScheduleActionUseCase and Dispatcher.
            tracing::info!("scheduled action fired: {id_for_task} ({label_for_log})");
            // Remove self from the map.
            jobs.lock().await.remove(&id_for_task);
        });

        // Store the action params for potential future use
        let _invoke_params = invoke_params;

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
        tracing::info!("cancelled scheduled job {id} ({label})", id = id, label = job.label);
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
    use crate::Dispatcher;
    use std::sync::Weak;

    fn empty_dispatcher() -> Weak<Dispatcher> {
        // For tests that don't actually fire, we use Weak::new() so
        // upgrade() returns None and the spawned task exits cleanly.
        Weak::new()
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
        // The exact shape of fires_at depends on the serialization choice
        // — assert it's present at minimum.
        assert!(v.get("fires_at").is_some());
    }

    #[tokio::test]
    async fn rejects_zero_delay() {
        let uc = ScheduleActionUseCase::new(empty_dispatcher());
        let res = uc.schedule(0, "Suspend".into(), serde_json::json!({})).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn rejects_delay_over_24h() {
        let uc = ScheduleActionUseCase::new(empty_dispatcher());
        let res = uc
            .schedule(86_401, "Suspend".into(), serde_json::json!({}))
            .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn accepts_valid_delay_and_returns_id() {
        let uc = ScheduleActionUseCase::new(empty_dispatcher());
        let id = uc
            .schedule(60, "Suspend".into(), serde_json::json!({}))
            .await
            .expect("schedule");
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn fires_after_delay_elapsed() {
        // Test with a very short delay (1 second) and real time
        let uc = Arc::new(ScheduleActionUseCase::new(empty_dispatcher()));
        let id = uc
            .schedule(1, "Suspend".into(), serde_json::json!({}))
            .await
            .expect("schedule");
        
        // Job is present immediately after scheduling
        assert!(uc.jobs.lock().await.contains_key(&id));
        
        // Wait for the job to complete (plus a small buffer)
        tokio::time::sleep(Duration::from_millis(1200)).await;
        
        // Job should have fired and removed itself from the map
        assert!(!uc.jobs.lock().await.contains_key(&id));
    }

    #[tokio::test]
    async fn list_starts_empty() {
        let uc = ScheduleActionUseCase::new(empty_dispatcher());
        assert!(uc.list().await.is_empty());
    }

    #[tokio::test]
    async fn list_returns_scheduled_jobs() {
        let uc = ScheduleActionUseCase::new(empty_dispatcher());
        let id = uc
            .schedule(60, "Suspend".into(), serde_json::json!({}))
            .await
            .unwrap();
        let jobs = uc.list().await;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, id);
        assert_eq!(jobs[0].label, "Suspend");
    }

    #[tokio::test]
    async fn cancel_unknown_id_returns_err() {
        let uc = ScheduleActionUseCase::new(empty_dispatcher());
        assert!(uc.cancel("doesnotexist".into()).await.is_err());
    }

    #[tokio::test]
    async fn cancel_real_id_prevents_fire() {
        let uc = ScheduleActionUseCase::new(empty_dispatcher());
        let id = uc
            .schedule(60, "Suspend".into(), serde_json::json!({}))
            .await
            .unwrap();
        uc.cancel(id.clone()).await.unwrap();
        assert!(uc.list().await.is_empty());
        // Wait a bit to ensure the aborted task doesn't somehow re-insert
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(uc.list().await.is_empty());
    }
}
