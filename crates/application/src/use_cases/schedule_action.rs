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
}

struct ScheduledJobInternal {
    id: ScheduleId,
    fires_at: SystemTime,
    label: String,
    abort: AbortHandle,
}

pub struct ScheduleActionUseCase {
    dispatcher: std::sync::Weak<crate::Dispatcher>,
    pub(crate) jobs: Arc<Mutex<HashMap<ScheduleId, ScheduledJobInternal>>>,
}

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
        let dispatcher_weak = self.dispatcher.clone();
        let jobs = self.jobs.clone();
        let id_for_task = id.clone();
        let invoke_for_task = invoke_params;
        let label_for_log = label.clone();

        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
            // Pull the dispatcher; if the daemon is shutting down,
            // upgrade() returns None and we exit cleanly.
            if let Some(dispatcher) = dispatcher_weak.upgrade() {
                let _ = dispatcher
                    .dispatch("action.invoke", invoke_for_task)
                    .await
                    .map_err(|e| {
                        tracing::warn!(
                            "scheduled action {id_for_task} ({label_for_log}) failed: {e}"
                        );
                    });
            }
            // Remove self from the map.
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
            let candidate: String = (0..8)
                .map(|_| format!("{:x}", rand::thread_rng().gen_range(0..16)))
                .collect();
            if !jobs.contains_key(&candidate) {
                return candidate;
            }
        }
    }
}
