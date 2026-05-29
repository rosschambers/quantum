use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, ProviderCapabilities, ProviderId, ProviderSource,
    Query, SystemStats,
};

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
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
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
                let cpu_percent = if let Some((pb, pt)) = prev {
                    let bd = busy.saturating_sub(pb) as f32;
                    let td = total.saturating_sub(pt) as f32;
                    if td > 0.0 {
                        (bd / td) * 100.0
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
                prev = Some((busy, total));
                let (used, total_mem) = match parse_proc_meminfo(&meminfo) {
                    Some(m) => m,
                    None => continue,
                };
                let payload = serde_json::to_value(&SystemStats {
                    cpu_percent,
                    mem_used_bytes: used,
                    mem_total_bytes: total_mem,
                })
                .unwrap_or(serde_json::Value::Null);
                let _ = tx_for_task.send(payload);
            }
        });
        Self { id, tx }
    }
}

#[async_trait]
impl ProviderSource for ProcStatsProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            searchable: false,
            streamable: true,
        }
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

pub(crate) fn parse_proc_stat_cpu(input: &str) -> Option<(u64, u64)> {
    let line = input.lines().find(|l| l.starts_with("cpu "))?;
    let parts: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() < 8 {
        return None;
    }
    let user = parts[0];
    let nice = parts[1];
    let system = parts[2];
    let idle = parts[3];
    let iowait = parts[4];
    let irq = parts[5];
    let softirq = parts[6];
    let steal = parts[7];
    let busy = user + nice + system + irq + softirq + steal;
    let total = busy + idle + iowait;
    Some((busy, total))
}

pub(crate) fn parse_proc_meminfo(input: &str) -> Option<(u64, u64)> {
    let mut total_kb = None;
    let mut available_kb = None;
    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kb = rest.split_whitespace().next().and_then(|s| s.parse().ok());
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available_kb = rest.split_whitespace().next().and_then(|s| s.parse().ok());
        }
        if total_kb.is_some() && available_kb.is_some() {
            break;
        }
    }
    let t: u64 = total_kb?;
    let a: u64 = available_kb?;
    let used = t.saturating_sub(a);
    Some((used * 1024, t * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proc_stat_cpu_typical() {
        let input = "cpu  100 50 200 1000 10 5 8 0 0 0\n";
        let (busy, total) = parse_proc_stat_cpu(input).expect("parse failed");
        // busy = 100 + 50 + 200 + 5 + 8 + 0 = 363
        // total = 363 + 1000 + 10 = 1373
        assert_eq!(busy, 363);
        assert_eq!(total, 1373);
    }

    #[test]
    fn parse_proc_stat_cpu_invalid() {
        let empty = "";
        assert!(parse_proc_stat_cpu(empty).is_none());

        let malformed = "cpu0 100 50 200 1000 10 5 8 0 0 0\n";
        assert!(parse_proc_stat_cpu(malformed).is_none());

        let insufficient = "cpu 100\n";
        assert!(parse_proc_stat_cpu(insufficient).is_none());
    }

    #[test]
    fn parse_proc_meminfo_typical() {
        let input = "MemTotal:        8192 kB\nMemAvailable:    4096 kB\n";
        let (used, total) = parse_proc_meminfo(input).expect("parse failed");
        // available = 4096, total = 8192, used = 8192 - 4096 = 4096
        // multiply by 1024: used = 4194304, total = 8388608
        assert_eq!(used, 4194304);
        assert_eq!(total, 8388608);
    }

    #[test]
    fn parse_proc_meminfo_missing_fields() {
        let no_available = "MemTotal:        8192 kB\n";
        assert!(parse_proc_meminfo(no_available).is_none());

        let no_total = "MemAvailable:    4096 kB\n";
        assert!(parse_proc_meminfo(no_total).is_none());

        let empty = "";
        assert!(parse_proc_meminfo(empty).is_none());
    }
}
