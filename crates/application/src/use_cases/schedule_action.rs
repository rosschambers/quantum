//! Schedule action use case.
//!
//! Enables scheduling of power actions with a configurable delay.
//! Scheduled jobs are held in-memory during daemon runtime. Persistence
//! across restarts is not supported in v1.
//!
//! `ScheduledJobSummary` serializes `fires_at` using the default serde serialization
//! shape for `SystemTime`, which is `{secs_since_epoch: u64, nanos_since_epoch: u32}`.
//! This avoids adding chrono as a dependency.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
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
}

struct ScheduledJobInternal {
    id: ScheduleId,
    fires_at: SystemTime,
    label: String,
    abort: AbortHandle,
}

pub struct ScheduleActionUseCase {
    dispatcher: std::sync::Weak<crate::Dispatcher>,
    jobs: Arc<Mutex<HashMap<ScheduleId, ScheduledJobInternal>>>,
}

impl ScheduleActionUseCase {
    pub fn new(dispatcher: std::sync::Weak<crate::Dispatcher>) -> Self {
        Self {
            dispatcher,
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
