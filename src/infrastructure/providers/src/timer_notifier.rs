//! Delivers timer-completion notifications and optional completion sounds.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;

use quantum_domain::{SoundName, Timer, TimerNotifier};

use crate::notifications::NotificationsProvider;

/// Whether a binary named `name` is resolvable through the shell's `PATH`
/// lookup. Never panics; any failure to run the probe is reported as `false`.
fn binary_exists(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Locate a freedesktop stereo sound-theme file (`<stem>.oga`) by searching the
/// XDG data directories, returning the first existing candidate.
///
/// The `paplay` fallback needs an absolute file path (unlike `canberra-gtk-play`,
/// which resolves theme events itself). Hardcoding `/usr/share` is wrong on
/// distributions that install the sound theme elsewhere (notably NixOS, where it
/// lives under a `/nix/store` path exported via `XDG_DATA_DIRS`), so search the
/// data directories in preference order and only then fall back to the classic
/// `/usr/share` location. `data_dirs` is the colon-separated `XDG_DATA_DIRS`
/// value (or `None` when unset); passing it in keeps this function pure and
/// testable.
fn resolve_sound_file(stem: &str, data_dirs: Option<&str>) -> Option<PathBuf> {
    let relative = format!("sounds/freedesktop/stereo/{stem}.oga");
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(dirs) = data_dirs {
        for dir in dirs.split(':').filter(|d| !d.is_empty()) {
            roots.push(PathBuf::from(dir));
        }
    }
    // Classic fallbacks for distributions that do not export the theme through
    // XDG_DATA_DIRS.
    roots.push(PathBuf::from("/usr/share"));
    roots.push(PathBuf::from("/usr/local/share"));
    roots
        .into_iter()
        .map(|root| root.join(&relative))
        .find(|candidate| candidate.exists())
}

/// Spawn `command` without blocking, and reap it once it exits.
///
/// Dropping a `std::process::Child` neither waits for nor reaps it, so a
/// player that was only `spawn()`ed stays a zombie for as long as quantumd
/// runs: one leaked process per chime. A short-lived thread blocks on
/// `wait()` instead; it works whether or not a tokio runtime is current.
/// Returns the child's process id.
fn spawn_and_reap(command: &mut Command) -> std::io::Result<u32> {
    let mut child = command.spawn()?;
    let process_id = child.id();
    std::thread::Builder::new()
        .name("sound-player-reaper".to_string())
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(process_id)
}

/// Fire-and-forget completion-sound player. Wraps an optional external player
/// binary discovered in `PATH`. When no player is available every `play` call
/// is a no-op.
pub struct SoundPlayer {
    command: Option<String>,
}

impl SoundPlayer {
    /// Probe `PATH` for a usable player binary, preferring `canberra-gtk-play`
    /// then falling back to `paplay`. Returns a disabled player when neither is
    /// present.
    pub fn detect() -> Self {
        let command = if binary_exists("canberra-gtk-play") {
            Some("canberra-gtk-play".to_string())
        } else if binary_exists("paplay") {
            Some("paplay".to_string())
        } else {
            None
        };
        Self { command }
    }

    /// A disabled player. Every `play` call is a no-op.
    pub fn none() -> Self {
        Self { command: None }
    }

    /// The freedesktop sound-theme event name for `canberra-gtk-play -i`.
    fn canberra_event(sound: SoundName) -> &'static str {
        match sound {
            SoundName::Complete => "complete",
            SoundName::Bell => "bell",
            SoundName::Chime => "message",
            SoundName::Alarm => "alarm-clock-elapsed",
        }
    }

    /// The freedesktop sound file stem played from the stereo theme directory.
    fn file_stem(sound: SoundName) -> &'static str {
        match sound {
            SoundName::Complete => "complete",
            SoundName::Bell => "bell",
            SoundName::Chime => "message",
            SoundName::Alarm => "alarm-clock-elapsed",
        }
    }

    /// Play the given completion sound. Spawns the player without blocking and
    /// silently ignores every failure. A no-op when no player is configured.
    pub fn play(&self, sound: SoundName) {
        let Some(command) = self.command.as_deref() else {
            return;
        };
        match command {
            "canberra-gtk-play" => {
                let _ = spawn_and_reap(
                    Command::new("canberra-gtk-play")
                        .arg("-i")
                        .arg(Self::canberra_event(sound)),
                );
            }
            "paplay" => {
                let data_dirs = std::env::var("XDG_DATA_DIRS").ok();
                if let Some(path) = resolve_sound_file(Self::file_stem(sound), data_dirs.as_deref())
                {
                    let _ = spawn_and_reap(Command::new("paplay").arg(path));
                }
            }
            _ => {}
        }
    }
}

/// `TimerNotifier` backed by the in-process notification store, with optional
/// completion-sound playback.
pub struct NotificationTimerNotifier {
    notifications: Arc<NotificationsProvider>,
    player: SoundPlayer,
}

impl NotificationTimerNotifier {
    /// Build a notifier from the shared notifications provider and a sound
    /// player.
    pub fn new(notifications: Arc<NotificationsProvider>, player: SoundPlayer) -> Self {
        Self {
            notifications,
            player,
        }
    }
}

#[async_trait]
impl TimerNotifier for NotificationTimerNotifier {
    async fn notify_complete(&self, timer: &Timer) {
        if timer.notify.notification {
            self.notifications
                .add_internal_notification(
                    "Quantum Timer".to_string(),
                    timer.label.clone(),
                    "Timer complete".to_string(),
                    None,
                    0,
                )
                .await;
        }
        if let Some(sound) = timer.notify.sound {
            self.player.play(sound);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use quantum_domain::{
        NotifyConfig, ProviderSource, Timer, TimerId, TimerKind, TimerStatus, VisualConfig,
    };
    use std::time::Duration;

    fn timer_with_notify(notify: NotifyConfig) -> Timer {
        Timer {
            id: TimerId::from("t1"),
            label: "Tea".to_string(),
            kind: TimerKind::OneShot { end_unix: 1700 },
            visual: VisualConfig::default(),
            notify,
            status: TimerStatus::Active,
            scatter_pos: None,
        }
    }

    #[tokio::test]
    async fn notify_complete_pushes_notification() {
        let notifications = Arc::new(NotificationsProvider::new());
        let mut stream = notifications.subscribe().expect("subscribe stream");
        let notifier = NotificationTimerNotifier::new(notifications.clone(), SoundPlayer::none());
        let timer = timer_with_notify(NotifyConfig {
            notification: true,
            sound: None,
            ..Default::default()
        });

        notifier.notify_complete(&timer).await;

        let envelope = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("stream item before timeout")
            .expect("envelope present");
        let notifications = envelope["notifications"]
            .as_array()
            .expect("notifications array");
        assert!(!notifications.is_empty());
    }

    #[tokio::test]
    async fn notify_complete_with_sound_no_player_does_not_panic() {
        let notifications = Arc::new(NotificationsProvider::new());
        let notifier = NotificationTimerNotifier::new(notifications, SoundPlayer::none());
        let timer = timer_with_notify(NotifyConfig {
            notification: false,
            sound: Some(SoundName::Complete),
            ..Default::default()
        });

        notifier.notify_complete(&timer).await;
    }

    /// The process state letter from `/proc/<pid>/stat` (the field after the
    /// parenthesised command name), or `None` once the process is fully gone.
    fn process_state(process_id: u32) -> Option<char> {
        let stat = std::fs::read_to_string(format!("/proc/{process_id}/stat")).ok()?;
        let after_name = &stat[stat.rfind(')')? + 1..];
        after_name.trim_start().chars().next()
    }

    #[test]
    fn spawned_player_is_reaped_after_it_exits() {
        // A spawned child that is never waited on stays a zombie (state `Z`)
        // until its parent exits; quantumd is long-lived, so every chime used
        // to leak one. After the player exits it must vanish from /proc.
        let process_id = spawn_and_reap(&mut Command::new("true")).expect("spawn true");

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut last_state = process_state(process_id);
        while last_state.is_some() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            last_state = process_state(process_id);
        }

        assert_eq!(
            last_state, None,
            "player process {process_id} was not reaped (state {last_state:?})"
        );
    }

    #[test]
    fn resolve_sound_file_prefers_xdg_data_dirs_over_usr_share() {
        let dir = tempfile::tempdir().expect("temp dir");
        let theme = dir.path().join("sounds/freedesktop/stereo");
        std::fs::create_dir_all(&theme).expect("create theme dir");
        std::fs::write(theme.join("message.oga"), b"stub").expect("write stub sound");

        let data_dirs = format!("{}:/nonexistent", dir.path().display());
        let resolved =
            resolve_sound_file("message", Some(&data_dirs)).expect("resolves from XDG_DATA_DIRS");

        assert_eq!(resolved, theme.join("message.oga"));
    }

    #[test]
    fn resolve_sound_file_returns_none_when_absent_everywhere() {
        // Use a sound name that cannot exist in system fallback directories
        // (/usr/share, /usr/local/share) either — "message" exists in the
        // freedesktop sound theme installed on Ubuntu CI runners.
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dirs = dir.path().display().to_string();

        assert!(resolve_sound_file("nonexistent-quantum-test-sound", Some(&data_dirs)).is_none());
    }

    #[test]
    fn resolve_sound_file_skips_empty_data_dir_entries() {
        let dir = tempfile::tempdir().expect("temp dir");
        let theme = dir.path().join("sounds/freedesktop/stereo");
        std::fs::create_dir_all(&theme).expect("create theme dir");
        std::fs::write(theme.join("bell.oga"), b"stub").expect("write stub sound");

        let data_dirs = format!("::{}:", dir.path().display());
        let resolved =
            resolve_sound_file("bell", Some(&data_dirs)).expect("resolves ignoring empty entries");

        assert_eq!(resolved, theme.join("bell.oga"));
    }
}
