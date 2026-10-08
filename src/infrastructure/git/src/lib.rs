//! `quantum-git`: git integration for repository review (qv diff mode).
//!
//! Shells out to the system `git` binary through `tokio::process::Command`
//! (argv arrays, no shell) rather than linking a git library, so the daemon
//! always tracks whatever git version is installed on the host.

mod runner;
// Consumed by `review::GitRepositoryReview::changes` (added in the next
// commit); until then nothing in the crate calls these items outside their
// own tests, which `-D warnings` would otherwise reject as dead code.
#[allow(dead_code)]
mod status;

pub use runner::repository_root;
