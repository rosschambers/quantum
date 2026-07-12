//! Sampler for the Linux `/proc` filesystem that produces per-process resource
//! samples and machine-wide global statistics for the task manager.
//!
//! The pure parsing functions ([`parse_pid_stat`], [`parse_pid_status_rss`],
//! [`parse_net_dev`]) take raw file contents as input so they can be unit-tested
//! without touching the real filesystem. [`ProcfsSampler`] walks the live
//! `/proc` tree, retaining the previous tick's counters so it can turn the
//! monotonically increasing kernel counters into per-interval rates and
//! percentages.

use std::collections::HashMap;
use std::time::Instant;

use quantum_domain::{GlobalStats, ProcessesError, RawProcess};

use crate::procfs_parse::{parse_proc_meminfo, parse_proc_stat_cpu};

/// The fields extracted from a single `/proc/<pid>/stat` line that the sampler
/// needs. `cpu_ticks` is the sum of the process's user and system jiffies
/// (`utime + stime`); it is a monotonically increasing counter, so a percentage
/// is only meaningful as a delta between two samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PidStat {
    /// The parent process identifier (field 4 of `/proc/<pid>/stat`).
    pub ppid: i32,
    /// The process command name (field 2), taken verbatim from between the
    /// first `(` and the last `)` so a name containing spaces or `)` survives.
    pub comm: String,
    /// The sum of the user and system jiffies the process has accumulated
    /// (`utime + stime`, fields 14 and 15).
    pub cpu_ticks: u64,
}

/// Parse a `/proc/<pid>/stat` line, extracting the parent pid, command name, and
/// accumulated CPU ticks (`utime + stime`). Returns `None` when the line does
/// not have the expected shape.
///
/// The command name in field 2 is wrapped in parentheses and may itself contain
/// spaces and `)` (for example a thread renamed `((odd) name)`), so the comm is
/// taken as the substring between the FIRST `(` and the LAST `)`. Every field
/// after the closing `)` is whitespace-separated, so the remaining numeric
/// fields are indexed from there.
pub fn parse_pid_stat(text: &str) -> Option<PidStat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open {
        return None;
    }
    let comm = text.get(open + 1..close)?.to_string();

    // Every field after the closing parenthesis is whitespace-separated. The
    // first is the process state (field 3), so ppid is the second (field 4) and
    // utime/stime are the twelfth and thirteenth (fields 14 and 15).
    let rest = text.get(close + 1..)?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    if fields.len() < 13 {
        return None;
    }
    let ppid: i32 = fields[1].parse().ok()?;
    let utime: u64 = fields[11].parse().ok()?;
    let stime: u64 = fields[12].parse().ok()?;

    Some(PidStat {
        ppid,
        comm,
        cpu_ticks: utime.saturating_add(stime),
    })
}

/// Parse the resident set size (`VmRSS`) from a `/proc/<pid>/status` file,
/// returning it in BYTES. The file reports the value in kibibytes, so it is
/// multiplied by 1024. Returns `None` when the file has no `VmRSS` line (for
/// example a kernel thread, which owns no user memory).
pub fn parse_pid_status_rss(text: &str) -> Option<u64> {
    let rest = text.lines().find_map(|line| line.strip_prefix("VmRSS:"))?;
    let kibibytes: u64 = rest.split_whitespace().next()?.parse().ok()?;
    Some(kibibytes.saturating_mul(1024))
}

/// Sum the received and transmitted byte counters across every interface in a
/// `/proc/net/dev` file EXCEPT the loopback interface `lo`, returning
/// `(rx_bytes, tx_bytes)`. Loopback traffic never leaves the machine, so it is
/// excluded from the machine-wide network totals.
pub fn parse_net_dev(text: &str) -> (u64, u64) {
    let mut rx_total: u64 = 0;
    let mut tx_total: u64 = 0;
    for line in text.lines() {
        // A data line is `interface: rx_bytes rx_packets ... tx_bytes ...`.
        // The two header lines have no colon in the interface position, so a
        // missing colon skips them.
        let (name, counters) = match line.split_once(':') {
            Some(parts) => parts,
            None => continue,
        };
        if name.trim() == "lo" {
            continue;
        }
        let numbers: Vec<u64> = counters
            .split_whitespace()
            .filter_map(|field| field.parse().ok())
            .collect();
        // Received bytes is the first counter; transmitted bytes is the ninth.
        if numbers.len() < 9 {
            continue;
        }
        rx_total = rx_total.saturating_add(numbers[0]);
        tx_total = tx_total.saturating_add(numbers[8]);
    }
    (rx_total, tx_total)
}

/// Samples the live `/proc` filesystem, retaining the previous sample's counters
/// so it can compute per-process CPU percentages and machine-wide network
/// rates. Owned mutably by the monitor task, which calls [`sample`] in a loop.
///
/// [`sample`]: ProcfsSampler::sample
#[derive(Debug, Default)]
pub struct ProcfsSampler {
    previous: Option<PreviousSample>,
}

#[derive(Debug)]
struct PreviousSample {
    busy_ticks: u64,
    total_ticks: u64,
    pid_ticks: HashMap<i32, u64>,
    net_rx_bytes: u64,
    net_tx_bytes: u64,
    taken_at: Instant,
}

impl ProcfsSampler {
    /// Construct a sampler with no previous baseline. The first [`sample`] call
    /// therefore reports every process at 0% CPU and zero network rates.
    ///
    /// [`sample`]: ProcfsSampler::sample
    pub fn new() -> Self {
        Self::default()
    }

    /// Read one full snapshot of the machine's processes and global statistics.
    ///
    /// Walks `/proc/[0-9]*`, reading each process's `stat` and `status`. A
    /// process that vanishes mid-read (any input/output error on its files) is
    /// skipped rather than failing the whole sample. CPU percentages and network
    /// rates are computed against the previous sample's counters; the first
    /// sample reports 0% CPU and zero network rates because there is no baseline.
    ///
    /// CPU percentage convention: a process's percentage is its jiffy delta
    /// divided by the machine-wide total jiffy delta (the `total` from
    /// `parse_proc_stat_cpu`, aggregated across all CPUs) times 100 — the same
    /// denominator `proc_stats.rs` uses for the global figure. A process pinning
    /// one core of an N-core machine therefore reads `100 / N`, and the process
    /// percentages sum to the global CPU percentage.
    pub async fn sample(&mut self) -> Result<(Vec<RawProcess>, GlobalStats), ProcessesError> {
        let taken_at = Instant::now();

        let stat = read_proc("/proc/stat").await?;
        let meminfo = read_proc("/proc/meminfo").await?;
        let net_dev = read_proc("/proc/net/dev").await?;

        let (busy_ticks, total_ticks) = parse_proc_stat_cpu(&stat)
            .ok_or_else(|| ProcessesError::Sampling("could not parse /proc/stat".to_string()))?;
        let (mem_used_bytes, mem_total_bytes) = parse_proc_meminfo(&meminfo)
            .ok_or_else(|| ProcessesError::Sampling("could not parse /proc/meminfo".to_string()))?;
        let (net_rx_bytes, net_tx_bytes) = parse_net_dev(&net_dev);

        // The machine-wide jiffy delta is the denominator for every per-process
        // and the global CPU percentage. Zero on the first sample (no baseline).
        let total_delta = self
            .previous
            .as_ref()
            .map(|previous| total_ticks.saturating_sub(previous.total_ticks))
            .unwrap_or(0);

        let mut processes = Vec::new();
        let mut pid_ticks = HashMap::new();
        for pid in read_pids().await? {
            let stat_text = match read_proc(&format!("/proc/{pid}/stat")).await {
                Ok(text) => text,
                // The process vanished between enumeration and read; skip it.
                Err(_) => continue,
            };
            let status_text = match read_proc(&format!("/proc/{pid}/status")).await {
                Ok(text) => text,
                Err(_) => continue,
            };
            let parsed = match parse_pid_stat(&stat_text) {
                Some(parsed) => parsed,
                None => continue,
            };
            // A kernel thread has no VmRSS line; treat its user memory as zero.
            let mem_bytes = parse_pid_status_rss(&status_text).unwrap_or(0);

            let cpu_percent = self.pid_cpu_percent(pid, parsed.cpu_ticks, total_delta);
            pid_ticks.insert(pid, parsed.cpu_ticks);

            processes.push(RawProcess {
                pid,
                ppid: parsed.ppid,
                name: parsed.comm,
                cpu_percent,
                mem_bytes,
            });
        }

        let cpu_percent = match &self.previous {
            Some(previous) if total_delta > 0 => {
                let busy_delta = busy_ticks.saturating_sub(previous.busy_ticks);
                (busy_delta as f64 / total_delta as f64 * 100.0) as f32
            }
            _ => 0.0,
        };

        let (net_rx_bytes_per_second, net_tx_bytes_per_second) = match &self.previous {
            Some(previous) => {
                let elapsed = taken_at.duration_since(previous.taken_at).as_secs_f64();
                let rate = |current: u64, prior: u64| -> u64 {
                    if elapsed <= 0.0 {
                        return 0;
                    }
                    (current.saturating_sub(prior) as f64 / elapsed) as u64
                };
                (
                    rate(net_rx_bytes, previous.net_rx_bytes),
                    rate(net_tx_bytes, previous.net_tx_bytes),
                )
            }
            None => (0, 0),
        };

        self.previous = Some(PreviousSample {
            busy_ticks,
            total_ticks,
            pid_ticks,
            net_rx_bytes,
            net_tx_bytes,
            taken_at,
        });

        let global = GlobalStats {
            cpu_percent,
            mem_used_bytes,
            mem_total_bytes,
            net_rx_bytes_per_second,
            net_tx_bytes_per_second,
        };
        Ok((processes, global))
    }

    /// Compute a process's CPU percentage from its jiffy delta against the
    /// machine-wide total jiffy delta. Returns 0.0 when there is no previous
    /// reading for the process or no elapsed machine ticks.
    fn pid_cpu_percent(&self, pid: i32, cpu_ticks: u64, total_delta: u64) -> f32 {
        if total_delta == 0 {
            return 0.0;
        }
        let previous_ticks = match self
            .previous
            .as_ref()
            .and_then(|previous| previous.pid_ticks.get(&pid))
        {
            Some(ticks) => *ticks,
            None => return 0.0,
        };
        let pid_delta = cpu_ticks.saturating_sub(previous_ticks);
        (pid_delta as f64 / total_delta as f64 * 100.0) as f32
    }
}

/// Read a `/proc` file to a string, mapping any input/output error to a
/// [`ProcessesError::Sampling`].
async fn read_proc(path: &str) -> Result<String, ProcessesError> {
    tokio::fs::read_to_string(path)
        .await
        .map_err(|error| ProcessesError::Sampling(format!("{path}: {error}")))
}

/// Enumerate the numeric process identifiers under `/proc`. Non-numeric entries
/// (`self`, `net`, and the rest) are skipped. A directory entry that vanishes
/// while iterating is ignored rather than failing the whole enumeration.
async fn read_pids() -> Result<Vec<i32>, ProcessesError> {
    let mut reader = tokio::fs::read_dir("/proc")
        .await
        .map_err(|error| ProcessesError::Sampling(format!("/proc: {error}")))?;
    let mut pids = Vec::new();
    loop {
        let entry = match reader.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(_) => continue,
        };
        if let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        {
            pids.push(pid);
        }
    }
    Ok(pids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pid_stat_extracts_ppid_comm_and_summed_ticks() {
        // pid 1234, comm "bash", state S, ppid 1000, then fields up to utime
        // (field 14) = 100 and stime (field 15) = 50.
        let line = "1234 (bash) S 1000 1234 1234 0 -1 4194304 100 0 0 0 100 50 0 0 20 0 1 0 0\n";
        let parsed = parse_pid_stat(line).expect("parse pid stat");
        assert_eq!(parsed.ppid, 1000);
        assert_eq!(parsed.comm, "bash");
        assert_eq!(parsed.cpu_ticks, 150);
    }

    #[test]
    fn parse_pid_stat_handles_comm_with_spaces_and_parentheses() {
        // A comm containing both a space and a closing parenthesis. The name
        // must be taken between the first '(' and the LAST ')'.
        let line = "42 (weird (name) here) R 7 42 42 0 -1 0 3 4 0 0 11 22 0 0 20 0 1 0 0\n";
        let parsed = parse_pid_stat(line).expect("parse pid stat");
        assert_eq!(parsed.ppid, 7);
        assert_eq!(parsed.comm, "weird (name) here");
        assert_eq!(parsed.cpu_ticks, 33);
    }

    #[test]
    fn parse_pid_stat_rejects_malformed_line() {
        assert!(parse_pid_stat("").is_none());
        assert!(parse_pid_stat("no parentheses here").is_none());
        // Missing the numeric fields after the comm.
        assert!(parse_pid_stat("1 (init)").is_none());
    }

    #[test]
    fn parse_pid_status_rss_returns_bytes() {
        let text = concat!(
            "Name:\tbash\n",
            "State:\tS (sleeping)\n",
            "VmRSS:\t    2048 kB\n",
            "Threads:\t1\n",
        );
        // 2048 kB * 1024 = 2_097_152 bytes.
        assert_eq!(parse_pid_status_rss(text), Some(2_097_152));
    }

    #[test]
    fn parse_pid_status_rss_absent_for_kernel_thread() {
        let text = concat!("Name:\tkworker\n", "State:\tI (idle)\n", "Threads:\t1\n");
        assert_eq!(parse_pid_status_rss(text), None);
    }

    #[test]
    fn parse_net_dev_sums_all_interfaces_except_loopback() {
        let text = concat!(
            "Inter-|   Receive                                                |  Transmit\n",
            " face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n",
            "    lo: 1000      10    0    0    0     0          0         0     1000      10    0    0    0     0       0          0\n",
            "  eth0: 5000      50    0    0    0     0          0         0     2000      20    0    0    0     0       0          0\n",
            "  wlan0: 300       3    0    0    0     0          0         0      100       1    0    0    0     0       0          0\n",
        );
        // rx = 5000 + 300 = 5300 (lo excluded); tx = 2000 + 100 = 2100.
        assert_eq!(parse_net_dev(text), (5300, 2100));
    }

    #[test]
    fn parse_net_dev_empty_yields_zero() {
        assert_eq!(parse_net_dev(""), (0, 0));
    }
}
