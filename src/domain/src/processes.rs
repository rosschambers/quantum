//! Process task-manager domain types.
//! Pure serde-friendly data transfer objects that cross the IPC boundary. No
//! imports from other workspace crates and no input/output.

use serde::{Deserialize, Serialize};

/// Machine-wide resource usage sampled alongside a process snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalStats {
    pub cpu_percent: f32,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    pub net_rx_bytes_per_second: u64,
    pub net_tx_bytes_per_second: u64,
}

/// The window a process owns, when it has one. Used to promote a process to an
/// application root and to label it in the task manager.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub class: String,
    pub title: String,
}

/// A node in the process forest: one process plus the subtree it roots. Carries
/// both self usage and the aggregate usage of itself and all descendants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessNode {
    pub pid: i32,
    pub name: String,
    pub cpu_percent: f32,
    pub mem_bytes: u64,
    pub aggregate_cpu_percent: f32,
    pub aggregate_mem_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowInfo>,
    pub protected: bool,
    pub children: Vec<ProcessNode>,
}

/// A full sampling of the machine's processes split into windowed applications
/// and background processes, alongside global resource usage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    pub global: GlobalStats,
    pub apps: Vec<ProcessNode>,
    pub background: Vec<ProcessNode>,
}

/// Which signal to deliver when terminating a process subtree. Serializes as a
/// lowercase name (`"term"` / `"kill"`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KillSignal {
    Term,
    Kill,
}

/// Errors produced by process-monitoring and process-signalling ports. Every
/// variant carries a plain human-readable payload rather than a host-specific
/// error type so nothing leaks across the IPC boundary.
#[derive(Debug, thiserror::Error)]
pub enum ProcessesError {
    #[error("process {0} not found")]
    NotFound(i32),
    #[error("permission denied signalling process {0}")]
    PermissionDenied(i32),
    #[error("refusing to kill protected process {0}")]
    Protected(i32),
    #[error("sampling failed: {0}")]
    Sampling(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_snapshot_round_trips_through_serde() {
        let snapshot = ProcessSnapshot {
            global: GlobalStats {
                cpu_percent: 12.5,
                mem_used_bytes: 4_000_000_000,
                mem_total_bytes: 16_000_000_000,
                net_rx_bytes_per_second: 1_024,
                net_tx_bytes_per_second: 512,
            },
            apps: vec![ProcessNode {
                pid: 100,
                name: "firefox".to_string(),
                cpu_percent: 5.0,
                mem_bytes: 500_000_000,
                aggregate_cpu_percent: 7.0,
                aggregate_mem_bytes: 600_000_000,
                window: Some(WindowInfo {
                    class: "firefox".to_string(),
                    title: "Mozilla Firefox".to_string(),
                }),
                protected: false,
                children: vec![ProcessNode {
                    pid: 200,
                    name: "firefox-tab".to_string(),
                    cpu_percent: 2.0,
                    mem_bytes: 100_000_000,
                    aggregate_cpu_percent: 2.0,
                    aggregate_mem_bytes: 100_000_000,
                    window: None,
                    protected: false,
                    children: vec![],
                }],
            }],
            background: vec![ProcessNode {
                pid: 300,
                name: "quantumd".to_string(),
                cpu_percent: 1.0,
                mem_bytes: 50_000_000,
                aggregate_cpu_percent: 1.0,
                aggregate_mem_bytes: 50_000_000,
                window: None,
                protected: true,
                children: vec![],
            }],
        };

        let json = serde_json::to_value(&snapshot).expect("serialize");
        assert_eq!(json["global"]["cpu_percent"], 12.5);
        assert_eq!(json["apps"][0]["pid"], 100);
        assert_eq!(json["apps"][0]["window"]["class"], "firefox");
        assert_eq!(json["apps"][0]["children"][0]["pid"], 200);
        assert_eq!(json["background"][0]["protected"], true);

        let back: ProcessSnapshot = serde_json::from_value(json).expect("round trip");
        assert_eq!(back, snapshot);
    }

    #[test]
    fn process_node_without_window_omits_window_field() {
        let node = ProcessNode {
            pid: 1,
            name: "init".to_string(),
            cpu_percent: 0.0,
            mem_bytes: 1_000,
            aggregate_cpu_percent: 0.0,
            aggregate_mem_bytes: 1_000,
            window: None,
            protected: false,
            children: vec![],
        };
        let json = serde_json::to_value(&node).expect("serialize");
        assert!(json.get("window").is_none());
    }

    #[test]
    fn kill_signal_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&KillSignal::Term).expect("serialize"),
            "\"term\""
        );
        assert_eq!(
            serde_json::to_string(&KillSignal::Kill).expect("serialize"),
            "\"kill\""
        );
    }

    #[test]
    fn kill_signal_deserializes_lowercase() {
        let term: KillSignal = serde_json::from_str("\"term\"").expect("deserialize");
        assert_eq!(term, KillSignal::Term);
        let kill: KillSignal = serde_json::from_str("\"kill\"").expect("deserialize");
        assert_eq!(kill, KillSignal::Kill);
    }
}
