//! `quantum-git`: git integration for repository review (qv diff mode).
//!
//! Shells out to the system `git` binary through `tokio::process::Command`
//! (argv arrays, no shell) rather than linking a git library, so the daemon
//! always tracks whatever git version is installed on the host.

mod contents;
mod review;
mod runner;
mod status;

pub use review::GitRepositoryReview;
pub use runner::repository_root;
