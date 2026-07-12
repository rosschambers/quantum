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

/// A flat process sample as read from the host, before it is arranged into the
/// application/background forest. `ppid` is the parent process identifier.
pub struct RawProcess {
    pub pid: i32,
    pub ppid: i32,
    pub name: String,
    pub cpu_percent: f32,
    pub mem_bytes: u64,
}

/// Arrange a flat list of processes into two forests: windowed applications and
/// everything else (background).
///
/// Rules:
/// - A pid present in `windows` becomes an application root with its window
///   populated, even if its parent is alive. Its `ppid`-descendants nest under
///   it, except descendants that are themselves window pids (they start their
///   own application root).
/// - Every other process appears exactly once in the background forest, linked
///   by `ppid`. A process whose parent is inside an application (or background)
///   subtree stays under that parent; a process whose parent is absent from the
///   sample becomes a background root.
/// - `aggregate_cpu_percent` / `aggregate_mem_bytes` are the sum of a node's own
///   usage and that of all its descendants.
/// - `protected` is true for `protected_pid` and every ancestor of it (walking
///   the `ppid` chain upward).
///
/// Children are ordered by pid ascending at every level, so the output is
/// deterministic regardless of input order.
pub fn build_forest(
    processes: Vec<RawProcess>,
    windows: &std::collections::HashMap<i32, WindowInfo>,
    protected_pid: i32,
) -> (Vec<ProcessNode>, Vec<ProcessNode>) {
    use std::collections::{HashMap, HashSet};

    // Index the sample by pid, and index children by parent pid in a single
    // pass. Children lists are sorted by pid so the built forest is stable.
    let mut by_pid: HashMap<i32, &RawProcess> = HashMap::new();
    let mut children_by_ppid: HashMap<i32, Vec<i32>> = HashMap::new();
    for process in &processes {
        by_pid.insert(process.pid, process);
        children_by_ppid
            .entry(process.ppid)
            .or_default()
            .push(process.pid);
    }
    for children in children_by_ppid.values_mut() {
        children.sort_unstable();
    }

    // The set of pids that root an application (a window pid that is present in
    // the sample). A window pid stops a parent's subtree from claiming it.
    let app_roots: HashSet<i32> = windows
        .keys()
        .copied()
        .filter(|pid| by_pid.contains_key(pid))
        .collect();

    // Protected pids: the target and every ancestor up the ppid chain. Bounded
    // by the sample size to guard against a cyclic ppid chain.
    let mut protected: HashSet<i32> = HashSet::new();
    let mut current = protected_pid;
    for _ in 0..=by_pid.len() {
        match by_pid.get(&current) {
            Some(process) => {
                protected.insert(current);
                current = process.ppid;
            }
            None => break,
        }
    }

    // Recursively build a node and its subtree. Window pids are excluded from a
    // parent's children so they can start their own application root.
    fn build_node(
        pid: i32,
        by_pid: &std::collections::HashMap<i32, &RawProcess>,
        children_by_ppid: &std::collections::HashMap<i32, Vec<i32>>,
        app_roots: &std::collections::HashSet<i32>,
        protected: &std::collections::HashSet<i32>,
        windows: &std::collections::HashMap<i32, WindowInfo>,
    ) -> ProcessNode {
        let process = by_pid[&pid];
        let mut children = Vec::new();
        if let Some(child_pids) = children_by_ppid.get(&pid) {
            for &child_pid in child_pids {
                // A window pid always starts its own application root, so no
                // parent (root or otherwise) claims it as a child.
                if app_roots.contains(&child_pid) {
                    continue;
                }
                children.push(build_node(
                    child_pid,
                    by_pid,
                    children_by_ppid,
                    app_roots,
                    protected,
                    windows,
                ));
            }
        }

        let aggregate_cpu_percent = process.cpu_percent
            + children
                .iter()
                .map(|child| child.aggregate_cpu_percent)
                .sum::<f32>();
        let aggregate_mem_bytes = process.mem_bytes
            + children
                .iter()
                .map(|child| child.aggregate_mem_bytes)
                .sum::<u64>();

        ProcessNode {
            pid,
            name: process.name.clone(),
            cpu_percent: process.cpu_percent,
            mem_bytes: process.mem_bytes,
            aggregate_cpu_percent,
            aggregate_mem_bytes,
            window: windows.get(&pid).cloned(),
            protected: protected.contains(&pid),
            children,
        }
    }

    // Application forest: one root per window pid, in pid order.
    let mut app_root_pids: Vec<i32> = app_roots.iter().copied().collect();
    app_root_pids.sort_unstable();
    let apps: Vec<ProcessNode> = app_root_pids
        .iter()
        .map(|&pid| {
            build_node(
                pid,
                &by_pid,
                &children_by_ppid,
                &app_roots,
                &protected,
                windows,
            )
        })
        .collect();

    // Background forest: every process not claimed by an application subtree. A
    // background root is an unclaimed process whose parent is not an unclaimed
    // process (parent absent, or parent claimed by an application).
    let mut claimed: HashSet<i32> = HashSet::new();
    fn mark_claimed(node: &ProcessNode, claimed: &mut std::collections::HashSet<i32>) {
        claimed.insert(node.pid);
        for child in &node.children {
            mark_claimed(child, claimed);
        }
    }
    for node in &apps {
        mark_claimed(node, &mut claimed);
    }

    let mut background_root_pids: Vec<i32> = processes
        .iter()
        .map(|process| process.pid)
        .filter(|pid| !claimed.contains(pid))
        .filter(|pid| {
            let ppid = by_pid[pid].ppid;
            // Root when the parent is absent or is claimed by an application.
            !by_pid.contains_key(&ppid) || claimed.contains(&ppid)
        })
        .collect();
    background_root_pids.sort_unstable();

    // Background build skips app roots (already claimed) via the same recursion.
    let background: Vec<ProcessNode> = background_root_pids
        .iter()
        .map(|&pid| {
            build_node(
                pid,
                &by_pid,
                &children_by_ppid,
                &app_roots,
                &protected,
                windows,
            )
        })
        .collect();

    (apps, background)
}

/// Collect the pids of the subtree rooted at `target`, depth-first in
/// children-before-parent order and ending with `target` itself. Returns `None`
/// when `target` is not present anywhere in `roots`. The ordering lets a caller
/// signal leaves before their parents when tearing a subtree down.
pub fn collect_subtree_pids(roots: &[ProcessNode], target: i32) -> Option<Vec<i32>> {
    fn find<'a>(roots: &'a [ProcessNode], target: i32) -> Option<&'a ProcessNode> {
        for node in roots {
            if node.pid == target {
                return Some(node);
            }
            if let Some(found) = find(&node.children, target) {
                return Some(found);
            }
        }
        None
    }

    fn post_order(node: &ProcessNode, out: &mut Vec<i32>) {
        for child in &node.children {
            post_order(child, out);
        }
        out.push(node.pid);
    }

    let node = find(roots, target)?;
    let mut out = Vec::new();
    post_order(node, &mut out);
    Some(out)
}

#[cfg(test)]
mod forest_tests {
    use super::*;
    use std::collections::HashMap;

    fn raw(pid: i32, ppid: i32, name: &str, cpu_percent: f32, mem_bytes: u64) -> RawProcess {
        RawProcess {
            pid,
            ppid,
            name: name.to_string(),
            cpu_percent,
            mem_bytes,
        }
    }

    fn window(class: &str, title: &str) -> WindowInfo {
        WindowInfo {
            class: class.to_string(),
            title: title.to_string(),
        }
    }

    /// Find a node by pid anywhere in a forest, for assertions.
    fn find<'a>(roots: &'a [ProcessNode], pid: i32) -> Option<&'a ProcessNode> {
        for node in roots {
            if node.pid == pid {
                return Some(node);
            }
            if let Some(found) = find(&node.children, pid) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn app_root_nests_children_and_grandchild() {
        let processes = vec![
            raw(100, 1, "app", 1.0, 100),
            raw(200, 100, "child-a", 2.0, 200),
            raw(300, 100, "child-b", 3.0, 300),
            raw(400, 200, "grandchild", 4.0, 400),
        ];
        let mut windows = HashMap::new();
        windows.insert(100, window("app", "App Window"));

        let (apps, background) = build_forest(processes, &windows, -1);

        assert!(background.is_empty());
        assert_eq!(apps.len(), 1);
        let root = &apps[0];
        assert_eq!(root.pid, 100);
        assert_eq!(root.window, Some(window("app", "App Window")));
        // Children are sorted by pid ascending.
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].pid, 200);
        assert_eq!(root.children[1].pid, 300);
        // Grandchild nests under the correct child.
        assert_eq!(root.children[0].children.len(), 1);
        assert_eq!(root.children[0].children[0].pid, 400);
        assert!(root.children[1].children.is_empty());
    }

    #[test]
    fn aggregates_sum_self_plus_descendants_at_each_level() {
        let processes = vec![
            raw(100, 1, "app", 1.0, 100),
            raw(200, 100, "child-a", 2.0, 200),
            raw(300, 100, "child-b", 3.0, 300),
            raw(400, 200, "grandchild", 4.0, 400),
        ];
        let mut windows = HashMap::new();
        windows.insert(100, window("app", "App Window"));

        let (apps, _background) = build_forest(processes, &windows, -1);

        let grandchild = find(&apps, 400).expect("grandchild");
        assert_eq!(grandchild.aggregate_cpu_percent, 4.0);
        assert_eq!(grandchild.aggregate_mem_bytes, 400);

        let child_a = find(&apps, 200).expect("child-a");
        assert_eq!(child_a.aggregate_cpu_percent, 6.0);
        assert_eq!(child_a.aggregate_mem_bytes, 600);

        let child_b = find(&apps, 300).expect("child-b");
        assert_eq!(child_b.aggregate_cpu_percent, 3.0);
        assert_eq!(child_b.aggregate_mem_bytes, 300);

        let root = find(&apps, 100).expect("root");
        assert_eq!(root.aggregate_cpu_percent, 10.0);
        assert_eq!(root.aggregate_mem_bytes, 1000);
    }

    #[test]
    fn orphan_becomes_background_root() {
        // Process 500's parent 999 is absent from the set, so it is a root.
        let processes = vec![raw(500, 999, "orphan", 1.0, 100)];
        let windows = HashMap::new();

        let (apps, background) = build_forest(processes, &windows, -1);

        assert!(apps.is_empty());
        assert_eq!(background.len(), 1);
        assert_eq!(background[0].pid, 500);
    }

    #[test]
    fn child_of_app_window_stays_under_app_not_background() {
        let processes = vec![
            raw(100, 1, "app", 1.0, 100),
            raw(600, 100, "helper", 2.0, 200),
        ];
        let mut windows = HashMap::new();
        windows.insert(100, window("app", "App Window"));

        let (apps, background) = build_forest(processes, &windows, -1);

        assert!(background.is_empty());
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].children.len(), 1);
        assert_eq!(apps[0].children[0].pid, 600);
        assert!(find(&background, 600).is_none());
    }

    #[test]
    fn protection_marks_target_and_all_ancestors() {
        // Chain 10 -> 20 -> 30 with sibling 40 under 10. Protecting 30 marks
        // 30, 20, and 10, but not the unrelated sibling 40.
        let processes = vec![
            raw(10, 1, "root", 1.0, 100),
            raw(20, 10, "middle", 1.0, 100),
            raw(30, 20, "quantumd", 1.0, 100),
            raw(40, 10, "sibling", 1.0, 100),
        ];
        let windows = HashMap::new();

        let (_apps, background) = build_forest(processes, &windows, 30);

        assert!(find(&background, 30).expect("30").protected);
        assert!(find(&background, 20).expect("20").protected);
        assert!(find(&background, 10).expect("10").protected);
        assert!(!find(&background, 40).expect("40").protected);
    }

    #[test]
    fn collect_subtree_pids_returns_children_before_parent() {
        let processes = vec![
            raw(100, 1, "app", 1.0, 100),
            raw(200, 100, "child-a", 2.0, 200),
            raw(300, 100, "child-b", 3.0, 300),
            raw(400, 200, "grandchild", 4.0, 400),
        ];
        let mut windows = HashMap::new();
        windows.insert(100, window("app", "App Window"));

        let (apps, _background) = build_forest(processes, &windows, -1);

        let pids = collect_subtree_pids(&apps, 100).expect("subtree");
        assert_eq!(pids, vec![400, 200, 300, 100]);

        // A deeper target collects only its own subtree, ending with itself.
        let inner = collect_subtree_pids(&apps, 200).expect("inner subtree");
        assert_eq!(inner, vec![400, 200]);
    }

    #[test]
    fn collect_subtree_pids_returns_none_for_missing_target() {
        let processes = vec![raw(100, 1, "app", 1.0, 100)];
        let mut windows = HashMap::new();
        windows.insert(100, window("app", "App Window"));

        let (apps, _background) = build_forest(processes, &windows, -1);

        assert!(collect_subtree_pids(&apps, 9999).is_none());
    }
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
