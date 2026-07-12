//! Subtree process killer with quantumd self-protection.
//!
//! [`LibcProcessKiller`] resolves a target's process subtree from the monitor's
//! freshest [`ProcessSnapshot`] (a shared cell handed over by
//! [`crate::monitor::TokioProcessMonitor::latest`]) and delivers a signal to
//! every member, children before parents. It refuses, defence-in-depth, to
//! signal a subtree that contains any protected process (quantumd and its
//! ancestors), never sending a single signal in that case.
//!
//! Signal delivery is abstracted behind [`SignalSender`] so the resolution and
//! protection logic is unit-testable without touching a real process. The
//! production [`LibcSignalSender`] wraps `libc::kill`.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use quantum_domain::{
    collect_subtree_pids, KillSignal, ProcessKiller, ProcessNode, ProcessSnapshot, ProcessesError,
};

/// Delivers a single signal to a single process. Abstracting the raw syscall
/// behind this trait lets the killer's resolution and protection logic be
/// exercised in tests without signalling any real process.
///
/// Implementations map the host's `errno` to a typed [`ProcessesError`]:
/// `EPERM` becomes [`ProcessesError::PermissionDenied`] and `ESRCH` (the
/// process already vanished) becomes [`ProcessesError::NotFound`]. The killer
/// decides how to treat each outcome depending on whether the process is the
/// target or a descendant.
pub trait SignalSender: Send + Sync {
    fn send(&self, pid: i32, signal: KillSignal) -> Result<(), ProcessesError>;
}

/// The production [`SignalSender`]: a thin wrapper over `libc::kill` that sends
/// `SIGTERM` for [`KillSignal::Term`] and `SIGKILL` for [`KillSignal::Kill`].
pub struct LibcSignalSender;

impl SignalSender for LibcSignalSender {
    fn send(&self, pid: i32, signal: KillSignal) -> Result<(), ProcessesError> {
        let raw_signal = match signal {
            KillSignal::Term => libc::SIGTERM,
            KillSignal::Kill => libc::SIGKILL,
        };
        // Safety: `kill` is a plain libc call with no memory concerns; it only
        // reads the pid and signal number.
        let result = unsafe { libc::kill(pid as libc::pid_t, raw_signal) };
        if result == 0 {
            return Ok(());
        }
        let errno = std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or_default();
        match errno {
            libc::EPERM => Err(ProcessesError::PermissionDenied(pid)),
            // The process already exited; map to NotFound so the caller can
            // treat a vanished target and a vanished descendant differently.
            libc::ESRCH => Err(ProcessesError::NotFound(pid)),
            other => Err(ProcessesError::Sampling(format!(
                "signalling process {pid} failed: errno {other}"
            ))),
        }
    }
}

/// Terminates a process subtree resolved from the monitor's freshest snapshot,
/// refusing to signal any subtree that contains a protected process.
pub struct LibcProcessKiller {
    latest: Arc<Mutex<Option<ProcessSnapshot>>>,
    sender: Box<dyn SignalSender>,
}

impl LibcProcessKiller {
    /// Construct a killer over a shared snapshot cell and an injected sender.
    /// The cell is the same `Arc` returned by
    /// [`crate::monitor::TokioProcessMonitor::latest`], so the killer always
    /// resolves against the most recent sample.
    pub fn new(latest: Arc<Mutex<Option<ProcessSnapshot>>>, sender: Box<dyn SignalSender>) -> Self {
        Self { latest, sender }
    }

    /// Construct a killer that signals real processes via [`LibcSignalSender`].
    pub fn with_libc(latest: Arc<Mutex<Option<ProcessSnapshot>>>) -> Self {
        Self::new(latest, Box::new(LibcSignalSender))
    }
}

/// Collect every protected pid anywhere in a forest into `out`.
fn collect_protected_pids(roots: &[ProcessNode], out: &mut HashSet<i32>) {
    for node in roots {
        if node.protected {
            out.insert(node.pid);
        }
        collect_protected_pids(&node.children, out);
    }
}

#[async_trait]
impl ProcessKiller for LibcProcessKiller {
    async fn kill_subtree(&self, pid: i32, signal: KillSignal) -> Result<(), ProcessesError> {
        // 1. Read the freshest snapshot. Clone it out and drop the lock before
        //    doing any work, so the lock is never held across signalling.
        let snapshot = {
            let guard = self
                .latest
                .lock()
                .map_err(|_| ProcessesError::Sampling("snapshot lock poisoned".to_string()))?;
            match guard.as_ref() {
                Some(snapshot) => snapshot.clone(),
                None => {
                    return Err(ProcessesError::Sampling(
                        "no snapshot available".to_string(),
                    ))
                }
            }
        };

        // 2. Resolve the subtree, searching applications first then background.
        //    The pids come back children-before-parent, ending with the target.
        let pids = match collect_subtree_pids(&snapshot.apps, pid)
            .or_else(|| collect_subtree_pids(&snapshot.background, pid))
        {
            Some(pids) => pids,
            None => return Err(ProcessesError::NotFound(pid)),
        };

        // 3. Self-protection guard: if the target or any descendant is
        //    protected, refuse without sending a single signal.
        let mut protected = HashSet::new();
        collect_protected_pids(&snapshot.apps, &mut protected);
        collect_protected_pids(&snapshot.background, &mut protected);
        if pids.iter().any(|member| protected.contains(member)) {
            return Err(ProcessesError::Protected(pid));
        }

        // Defense in depth: the check above trusts the `protected` flag baked
        // into the snapshot by `build_forest`. Independently refuse if this
        // daemon's own pid is anywhere in the resolved subtree, so a regression
        // in the marking path can never leave quantum able to signal itself.
        let own_pid = std::process::id() as i32;
        if pids.contains(&own_pid) {
            return Err(ProcessesError::Protected(pid));
        }

        // 4. Signal each member, children before the parent. A descendant that
        //    vanished mid-teardown (ESRCH -> NotFound) is ignored; any other
        //    descendant error is logged and teardown continues. A hard error on
        //    the target itself propagates.
        for &member in &pids {
            match self.sender.send(member, signal) {
                Ok(()) => {}
                Err(error) => {
                    if member == pid {
                        return Err(error);
                    }
                    match error {
                        ProcessesError::NotFound(_) => {}
                        other => {
                            tracing::warn!("failed to signal descendant process {member}: {other}");
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quantum_domain::{GlobalStats, ProcessNode};

    /// Records every `(pid, signal)` it is asked to deliver, and can be
    /// configured to fail specific pids with a chosen error.
    struct RecordingSender {
        calls: Arc<Mutex<Vec<(i32, KillSignal)>>>,
        failures: std::collections::HashMap<i32, ErrorKind>,
    }

    /// The failure a [`RecordingSender`] should return for a given pid, mirroring
    /// the `errno` mapping a real sender would perform.
    #[derive(Clone, Copy)]
    enum ErrorKind {
        PermissionDenied,
        NotFound,
        Other,
    }

    impl RecordingSender {
        fn new(calls: Arc<Mutex<Vec<(i32, KillSignal)>>>) -> Self {
            Self {
                calls,
                failures: std::collections::HashMap::new(),
            }
        }

        fn failing(
            calls: Arc<Mutex<Vec<(i32, KillSignal)>>>,
            failures: std::collections::HashMap<i32, ErrorKind>,
        ) -> Self {
            Self { calls, failures }
        }
    }

    impl SignalSender for RecordingSender {
        fn send(&self, pid: i32, signal: KillSignal) -> Result<(), ProcessesError> {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push((pid, signal));
            }
            match self.failures.get(&pid) {
                Some(ErrorKind::PermissionDenied) => Err(ProcessesError::PermissionDenied(pid)),
                Some(ErrorKind::NotFound) => Err(ProcessesError::NotFound(pid)),
                Some(ErrorKind::Other) => Err(ProcessesError::Sampling(format!("boom {pid}"))),
                None => Ok(()),
            }
        }
    }

    fn node(pid: i32, protected: bool, children: Vec<ProcessNode>) -> ProcessNode {
        ProcessNode {
            pid,
            name: format!("process-{pid}"),
            cpu_percent: 0.0,
            mem_bytes: 0,
            aggregate_cpu_percent: 0.0,
            aggregate_mem_bytes: 0,
            window: None,
            protected,
            children,
        }
    }

    fn snapshot(apps: Vec<ProcessNode>, background: Vec<ProcessNode>) -> ProcessSnapshot {
        ProcessSnapshot {
            global: GlobalStats {
                cpu_percent: 0.0,
                mem_used_bytes: 0,
                mem_total_bytes: 0,
                net_rx_bytes_per_second: 0,
                net_tx_bytes_per_second: 0,
            },
            apps,
            background,
        }
    }

    fn cell(snapshot: Option<ProcessSnapshot>) -> Arc<Mutex<Option<ProcessSnapshot>>> {
        Arc::new(Mutex::new(snapshot))
    }

    // Acceptance criterion 1: a protected node anywhere in the subtree refuses
    // the whole kill and sends nothing.
    #[tokio::test]
    async fn protected_subtree_refuses_and_sends_nothing() {
        // Target 100 has a nested protected quantumd (300) under child 200.
        let tree = node(
            100,
            false,
            vec![node(200, false, vec![node(300, true, vec![])])],
        );
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sender = Box::new(RecordingSender::new(Arc::clone(&calls)));
        let killer = LibcProcessKiller::new(cell(Some(snapshot(vec![], vec![tree]))), sender);

        let result = killer.kill_subtree(100, KillSignal::Term).await;

        assert!(matches!(result, Err(ProcessesError::Protected(100))));
        assert!(
            calls.lock().expect("calls").is_empty(),
            "no signal may be sent when the subtree is protected"
        );
    }

    // Defense in depth: even if the snapshot wrongly marks this daemon's own
    // pid as unprotected, the killer independently refuses to signal itself.
    #[tokio::test]
    async fn own_pid_refused_even_when_snapshot_marks_it_unprotected() {
        let own_pid = std::process::id() as i32;
        // The snapshot lies: quantum's own pid is present but `protected: false`.
        let tree = node(own_pid, false, vec![]);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sender = Box::new(RecordingSender::new(Arc::clone(&calls)));
        let killer = LibcProcessKiller::new(cell(Some(snapshot(vec![], vec![tree]))), sender);

        let result = killer.kill_subtree(own_pid, KillSignal::Term).await;

        assert!(matches!(result, Err(ProcessesError::Protected(_))));
        assert!(
            calls.lock().expect("calls").is_empty(),
            "the daemon must never signal its own pid, snapshot flag notwithstanding"
        );
    }

    // Acceptance criterion 2: no snapshot yet yields a sampling error.
    #[tokio::test]
    async fn missing_snapshot_yields_sampling_error() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sender = Box::new(RecordingSender::new(Arc::clone(&calls)));
        let killer = LibcProcessKiller::new(cell(None), sender);

        let result = killer.kill_subtree(100, KillSignal::Term).await;

        assert!(matches!(result, Err(ProcessesError::Sampling(_))));
        assert!(calls.lock().expect("calls").is_empty());
    }

    // Acceptance criterion 3: a target absent from both forests yields NotFound.
    #[tokio::test]
    async fn absent_target_yields_not_found() {
        let tree = node(100, false, vec![]);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sender = Box::new(RecordingSender::new(Arc::clone(&calls)));
        let killer = LibcProcessKiller::new(cell(Some(snapshot(vec![], vec![tree]))), sender);

        let result = killer.kill_subtree(9999, KillSignal::Term).await;

        assert!(matches!(result, Err(ProcessesError::NotFound(9999))));
        assert!(calls.lock().expect("calls").is_empty());
    }

    // Acceptance criterion 4: a non-protected subtree is signalled children
    // before parent, with the requested signal.
    #[tokio::test]
    async fn happy_path_signals_children_before_parent() {
        // Parent 100 with two children 200 and 300.
        let tree = node(
            100,
            false,
            vec![node(200, false, vec![]), node(300, false, vec![])],
        );
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sender = Box::new(RecordingSender::new(Arc::clone(&calls)));
        let killer = LibcProcessKiller::new(cell(Some(snapshot(vec![], vec![tree]))), sender);

        let result = killer.kill_subtree(100, KillSignal::Kill).await;

        assert!(result.is_ok());
        let recorded = calls.lock().expect("calls").clone();
        assert_eq!(
            recorded,
            vec![
                (200, KillSignal::Kill),
                (300, KillSignal::Kill),
                (100, KillSignal::Kill),
            ]
        );
    }

    // A descendant that vanished mid-teardown (ESRCH -> NotFound) is ignored and
    // the parent is still signalled.
    #[tokio::test]
    async fn vanished_descendant_is_ignored() {
        let tree = node(100, false, vec![node(200, false, vec![])]);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut failures = std::collections::HashMap::new();
        failures.insert(200, ErrorKind::NotFound);
        let sender = Box::new(RecordingSender::failing(Arc::clone(&calls), failures));
        let killer = LibcProcessKiller::new(cell(Some(snapshot(vec![], vec![tree]))), sender);

        let result = killer.kill_subtree(100, KillSignal::Term).await;

        assert!(
            result.is_ok(),
            "a vanished child must not fail the teardown"
        );
        let recorded = calls.lock().expect("calls").clone();
        assert_eq!(
            recorded,
            vec![(200, KillSignal::Term), (100, KillSignal::Term)]
        );
    }

    // A permission error on the target itself propagates as PermissionDenied.
    #[tokio::test]
    async fn target_permission_error_propagates() {
        let tree = node(100, false, vec![]);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut failures = std::collections::HashMap::new();
        failures.insert(100, ErrorKind::PermissionDenied);
        let sender = Box::new(RecordingSender::failing(Arc::clone(&calls), failures));
        let killer = LibcProcessKiller::new(cell(Some(snapshot(vec![], vec![tree]))), sender);

        let result = killer.kill_subtree(100, KillSignal::Term).await;

        assert!(matches!(result, Err(ProcessesError::PermissionDenied(100))));
    }

    // A non-ESRCH descendant error is logged and swallowed; teardown continues
    // to the parent.
    #[tokio::test]
    async fn other_descendant_error_is_swallowed() {
        let tree = node(100, false, vec![node(200, false, vec![])]);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut failures = std::collections::HashMap::new();
        failures.insert(200, ErrorKind::Other);
        let sender = Box::new(RecordingSender::failing(Arc::clone(&calls), failures));
        let killer = LibcProcessKiller::new(cell(Some(snapshot(vec![], vec![tree]))), sender);

        let result = killer.kill_subtree(100, KillSignal::Term).await;

        assert!(result.is_ok());
        let recorded = calls.lock().expect("calls").clone();
        assert_eq!(
            recorded,
            vec![(200, KillSignal::Term), (100, KillSignal::Term)]
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    use std::path::Path;
    use std::time::Duration;

    use quantum_domain::GlobalStats;

    use crate::sampler::parse_pid_stat;

    /// Read the child pids of `parent` by scanning `/proc/<pid>/stat` for a
    /// matching ppid. Uses blocking std input/output; only called in tests.
    fn child_pids_of(parent: i32) -> Vec<i32> {
        let mut children = Vec::new();
        let entries = match std::fs::read_dir("/proc") {
            Ok(entries) => entries,
            Err(_) => return children,
        };
        for entry in entries.flatten() {
            let pid: i32 = match entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse().ok())
            {
                Some(pid) => pid,
                None => continue,
            };
            let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Ok(text) => text,
                Err(_) => continue,
            };
            if let Some(parsed) = parse_pid_stat(&stat) {
                if parsed.ppid == parent {
                    children.push(pid);
                }
            }
        }
        children
    }

    fn process_alive(pid: i32) -> bool {
        Path::new(&format!("/proc/{pid}")).exists()
    }

    fn build_snapshot(parent: i32, children: &[i32]) -> ProcessSnapshot {
        let child_nodes: Vec<ProcessNode> = children
            .iter()
            .map(|&pid| ProcessNode {
                pid,
                name: "sleep".to_string(),
                cpu_percent: 0.0,
                mem_bytes: 0,
                aggregate_cpu_percent: 0.0,
                aggregate_mem_bytes: 0,
                window: None,
                protected: false,
                children: vec![],
            })
            .collect();
        ProcessSnapshot {
            global: GlobalStats {
                cpu_percent: 0.0,
                mem_used_bytes: 0,
                mem_total_bytes: 0,
                net_rx_bytes_per_second: 0,
                net_tx_bytes_per_second: 0,
            },
            apps: vec![],
            background: vec![ProcessNode {
                pid: parent,
                name: "sh".to_string(),
                cpu_percent: 0.0,
                mem_bytes: 0,
                aggregate_cpu_percent: 0.0,
                aggregate_mem_bytes: 0,
                window: None,
                protected: false,
                children: child_nodes,
            }],
        }
    }

    // Acceptance criterion 5: spawn a real subtree, resolve it, and kill it with
    // the real libc sender; the whole subtree must be gone shortly after.
    #[tokio::test]
    async fn kills_a_real_subtree() {
        // A shell that spawns two backgrounded sleeps and waits on them, giving
        // a parent with two real children.
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 60 & sleep 60 & wait")
            .spawn()
            .expect("spawn shell subtree");
        let parent = child.id().expect("child pid") as i32;

        // Give the backgrounded sleeps a moment to appear under the shell.
        let mut children = Vec::new();
        for _ in 0..50 {
            children = child_pids_of(parent);
            if children.len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            children.len(),
            2,
            "expected two sleep children under the shell"
        );

        let snapshot = build_snapshot(parent, &children);
        let latest = Arc::new(Mutex::new(Some(snapshot)));
        let killer = LibcProcessKiller::with_libc(Arc::clone(&latest));

        let kill_result = killer.kill_subtree(parent, KillSignal::Term).await;

        // Reap the shell (our direct child) so it does not linger as a zombie,
        // and force-clean the whole subtree if anything is still alive.
        let cleanup = || {
            for &pid in &children {
                // SIGKILL ignoring errors; best-effort leak prevention.
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
            }
            unsafe {
                libc::kill(parent as libc::pid_t, libc::SIGKILL);
            }
        };

        if kill_result.is_err() {
            cleanup();
            let _ = child.wait().await;
        }
        assert!(kill_result.is_ok(), "kill_subtree failed: {kill_result:?}");

        // Reap the shell so /proc/<parent> is released.
        let _ = child.wait().await;

        // The two sleeps are reparented to init on the shell's exit; poll until
        // init reaps them and their /proc entries disappear.
        let mut all_gone = false;
        for _ in 0..100 {
            if children.iter().all(|&pid| !process_alive(pid)) {
                all_gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        if !all_gone {
            cleanup();
        }
        assert!(
            all_gone,
            "subtree processes still alive after kill: {children:?}"
        );
    }
}
