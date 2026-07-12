//! quantum-processes: shared process and `/proc` filesystem parsing helpers.
//!
//! This crate is a sibling infrastructure crate. It may depend on
//! `quantum-domain` and other sibling infrastructure crates, but never on the
//! application or user-interface layers. It houses the pure parsers used by the
//! provider layer (and later tasks) to read `/proc/stat` and `/proc/meminfo`.

pub mod killer;
pub mod monitor;
pub mod procfs_parse;
pub mod sampler;
pub mod windows;

pub use killer::{LibcProcessKiller, LibcSignalSender, SignalSender};
pub use monitor::{ProcessSampleSource, TokioProcessMonitor};
pub use sampler::{parse_net_dev, parse_pid_stat, parse_pid_status_rss, PidStat, ProcfsSampler};
pub use windows::window_pid_map;
