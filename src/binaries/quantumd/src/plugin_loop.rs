//! Polling-script loops for declarative plugins.
//!
//! `quantumd::main` calls `quantum_plugins::walk` at startup, then spawns
//! one task per `PolledScript` running [`run_polling_script_loop`] below.
//! The loop runs the script on its configured interval, parses stdout,
//! and publishes to the broadcast event bus on the script's channel.
//!
//! Lifecycle rules (all match the post-audit pattern used by built-in
//! providers):
//! - Tick uses `MissedTickBehavior::Skip` so a transient stall does not
//!   burst-fire missed ticks.
//! - Timeout per tick is `max(interval - 1s, 4s)` so a script can never
//!   block the next tick.
//! - Stdout > 1 MiB is truncated with a warning.
//! - Empty stdout = no publish.
//! - Non-zero exit = log warning, no publish.
//! - Publish is change-gated: identical payloads in succession are
//!   suppressed.
//! - Environment: inherits daemon env plus `QUANTUM_PLUGIN_NAME` and
//!   `QUANTUM_PLUGIN_DIR`.
//! - Working directory: the plugin's folder.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::MissedTickBehavior;

/// Run the polling loop for one plugin script. Returns once the
/// `event_bus` Arc is the last reference (i.e. the daemon is shutting
/// down) or never \u2014 the task is expected to run for the daemon's
/// lifetime.
pub async fn run_polling_script_loop(
    channel: String,
    interval: Duration,
    command: PathBuf,
    plugin_dir: PathBuf,
    plugin_name: String,
    event_bus: Arc<dyn quantum_domain::EventBus>,
) {
    const MAX_STDOUT: usize = 1024 * 1024;
    let timeout = interval
        .saturating_sub(Duration::from_secs(1))
        .max(Duration::from_secs(4));

    let mut tick = tokio::time::interval(interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_published: Option<serde_json::Value> = None;

    loop {
        tick.tick().await;

        let mut cmd = tokio::process::Command::new(&command);
        cmd.current_dir(&plugin_dir)
            .env("QUANTUM_PLUGIN_NAME", &plugin_name)
            .env("QUANTUM_PLUGIN_DIR", &plugin_dir)
            .stdin(std::process::Stdio::null());

        let output = match tokio::time::timeout(timeout, cmd.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                tracing::warn!("plugin '{plugin_name}' channel '{channel}' spawn error: {e}");
                continue;
            }
            Err(_) => {
                tracing::warn!(
                    "plugin '{plugin_name}' channel '{channel}' timed out after {:?}",
                    timeout
                );
                continue;
            }
        };

        if !output.status.success() {
            tracing::warn!(
                "plugin '{plugin_name}' channel '{channel}' exit {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
            continue;
        }

        let stdout: &[u8] = if output.stdout.len() > MAX_STDOUT {
            tracing::warn!(
                "plugin '{plugin_name}' channel '{channel}' stdout exceeds 1 MiB; truncating"
            );
            &output.stdout[..MAX_STDOUT]
        } else {
            &output.stdout
        };
        if stdout.is_empty() {
            continue;
        }

        let value: serde_json::Value = serde_json::from_slice(stdout).unwrap_or_else(|_| {
            serde_json::Value::String(String::from_utf8_lossy(stdout).into_owned())
        });

        if last_published.as_ref() == Some(&value) {
            continue;
        }
        let payload_str = value.to_string();
        if let Err(e) = event_bus.publish(&channel, &payload_str).await {
            tracing::warn!("plugin '{plugin_name}' publish on '{channel}' failed: {e}");
            continue;
        }
        last_published = Some(value);
    }
}
