use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, ProviderId, ProviderSource, Query, SystemStats,
};
use quantum_processes::procfs_parse::{parse_proc_meminfo, parse_proc_stat_cpu};

pub struct ProcStatsProvider {
    id: ProviderId,
    tx: broadcast::Sender<serde_json::Value>,
}

impl ProcStatsProvider {
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        let id = ProviderId::from("system.stats");
        let (tx, _rx) = broadcast::channel::<serde_json::Value>(16);
        let tx_for_task = tx.clone();
        runtime.spawn(async move {
            let mut prev: Option<(u64, u64)> = None;
            // Last payload pushed onto the broadcast channel. Used to
            // suppress repeat publishes when the rounded cpu/mem numbers
            // didn't actually move; subscribers (and through them, the
            // GTK + WebKit render path) then stay idle between real
            // changes instead of waking up once a second.
            let mut last_published: Option<serde_json::Value> = None;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                // No subscriber (no bar connected): skip the /proc read and
                // parse entirely for this tick. The broadcast sender reports
                // zero receivers, so nothing would consume the payload anyway.
                // When a subscriber appears, receiver_count() goes positive
                // again and polling resumes on the next tick. `prev` may be
                // stale after an idle gap, but the busy-over-total ratio stays
                // a valid percentage across any interval length, so the first
                // resumed payload remains correct.
                if tx_for_task.receiver_count() == 0 {
                    continue;
                }
                let stat = match tokio::fs::read_to_string("/proc/stat").await {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let meminfo = match tokio::fs::read_to_string("/proc/meminfo").await {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let (busy, total) = match parse_proc_stat_cpu(&stat) {
                    Some(p) => p,
                    None => continue,
                };
                let (used, total_mem) = match parse_proc_meminfo(&meminfo) {
                    Some(m) => m,
                    None => continue,
                };
                let (busy_delta, total_delta) = match prev {
                    Some((pb, pt)) => (busy.saturating_sub(pb), total.saturating_sub(pt)),
                    None => (0, 0),
                };
                prev = Some((busy, total));
                let payload = build_payload(busy_delta, total_delta, used, total_mem);
                send_if_changed(&tx_for_task, &mut last_published, payload);
            }
        });
        Self { id, tx }
    }
}

/// Build a `SystemStats` JSON payload from raw `/proc` deltas.
///
/// `cpu_percent` is rounded to the nearest integer (then re-cast to the f32
/// the DTO requires). Rounding is what makes the per-second dedup pay off:
/// without it sub-percent jitter in the deltas would flip the payload every
/// tick and the change-gate would never fire. The bar's CPU meter renders at
/// integer precision (see `src/ui/themes/.../SystemMeters.svelte`), so one-decimal
/// rounding would only chase float noise that nobody can see.
pub(crate) fn build_payload(
    busy_delta: u64,
    total_delta: u64,
    mem_used_bytes: u64,
    mem_total_bytes: u64,
) -> serde_json::Value {
    let raw = if total_delta > 0 {
        (busy_delta as f64 / total_delta as f64) * 100.0
    } else {
        0.0
    };
    // f64 is used through the round to avoid the f32->f64 widening bias that
    // makes halfway cases like 12.35 land at 12.349... and round the wrong
    // way. The result is integer-valued so the final f32 cast is exact.
    let cpu_percent = raw.round() as f32;
    serde_json::to_value(&SystemStats {
        cpu_percent,
        mem_used_bytes,
        mem_total_bytes,
    })
    .unwrap_or(serde_json::Value::Null)
}

/// Forward `candidate` on `tx` only when it differs from `last`. Updates
/// `last` to the latest sent value when a send happens.
pub(crate) fn send_if_changed(
    tx: &broadcast::Sender<serde_json::Value>,
    last: &mut Option<serde_json::Value>,
    candidate: serde_json::Value,
) {
    if last.as_ref() == Some(&candidate) {
        return;
    }
    let _ = tx.send(candidate.clone());
    *last = Some(candidate);
}

#[async_trait]
impl ProviderSource for ProcStatsProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, _: &Query) -> Result<Vec<Match>, DomainError> {
        Ok(vec![])
    }

    async fn invoke(&self, _: &Action) -> Result<ActionOutcome, DomainError> {
        Err(DomainError::Unsupported(
            "system.stats does not handle actions".into(),
        ))
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        let rx = self.tx.subscribe();
        Some(
            BroadcastStream::new(rx)
                .filter_map(|res| async move { res.ok() })
                .boxed(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_payload_rounds_cpu_percent_to_integer() {
        // 12.3456% rounds down to 12.
        let v = build_payload(123_456, 1_000_000, 4096, 8192);
        assert_eq!(v["cpu_percent"], serde_json::json!(12.0));
    }

    #[test]
    fn build_payload_rounds_cpu_percent_halfway_up() {
        // 12.6% rounds up to 13.
        let v = build_payload(126_000, 1_000_000, 4096, 8192);
        assert_eq!(v["cpu_percent"], serde_json::json!(13.0));
    }

    #[test]
    fn build_payload_dedup_sub_percent_jitter_produces_identical_payload() {
        // 12.3% and 12.4% both round to 12 -> identical payloads. This is
        // the entire point of rounding: kill sub-percent jitter so the
        // change-gate fires at most once a second on a steady system.
        let a = build_payload(123_000, 1_000_000, 4096, 8192);
        let b = build_payload(124_000, 1_000_000, 4096, 8192);
        assert_eq!(a, b);
    }

    #[test]
    fn build_payload_dedup_distinct_values_differ() {
        let a = build_payload(100_000, 1_000_000, 4096, 8192);
        let b = build_payload(200_000, 1_000_000, 4096, 8192);
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn change_gated_send_skips_duplicate_payload() {
        // Drive two identical payloads through `send_if_changed`; the
        // second one must NOT reach the broadcast receiver.
        let (tx, mut rx) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let mut last: Option<serde_json::Value> = None;
        let p1 = build_payload(100_000, 1_000_000, 4096, 8192);
        let p2 = build_payload(100_000, 1_000_000, 4096, 8192);

        send_if_changed(&tx, &mut last, p1.clone());
        send_if_changed(&tx, &mut last, p2.clone());

        // First payload arrives.
        let got = rx.try_recv().expect("first payload arrives");
        assert_eq!(got, p1);
        // Second one was deduped.
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn change_gated_send_forwards_distinct_payload() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let mut last: Option<serde_json::Value> = None;
        let p1 = build_payload(100_000, 1_000_000, 4096, 8192);
        let p2 = build_payload(200_000, 1_000_000, 4096, 8192);

        send_if_changed(&tx, &mut last, p1.clone());
        send_if_changed(&tx, &mut last, p2.clone());

        assert_eq!(rx.try_recv().expect("first"), p1);
        assert_eq!(rx.try_recv().expect("second"), p2);
    }
}
