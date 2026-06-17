//! Delivers timer-completion notifications and optional completion sounds.

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
                let _ = Command::new("canberra-gtk-play")
                    .arg("-i")
                    .arg(Self::canberra_event(sound))
                    .spawn();
            }
            "paplay" => {
                let path = format!(
                    "/usr/share/sounds/freedesktop/stereo/{}.oga",
                    Self::file_stem(sound)
                );
                let _ = Command::new("paplay").arg(path).spawn();
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
        let notifier =
            NotificationTimerNotifier::new(notifications.clone(), SoundPlayer::none());
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
}
