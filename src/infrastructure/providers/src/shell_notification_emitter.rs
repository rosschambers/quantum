//! Emits launcher command-capture output as an in-process notification.

use std::sync::Arc;

use async_trait::async_trait;

use quantum_domain::NotificationEmitter;

use crate::notifications::NotificationsProvider;

/// Auto-dismiss delay, in milliseconds, applied to a command-output
/// notification so its toast fades on its own. A finite, non-zero value keeps
/// the toast transient rather than pinning it in the center forever.
const COMMAND_OUTPUT_TIMEOUT_MS: u64 = 5000;

/// [`NotificationEmitter`] backed by the in-process notification store. Pushes
/// a normal-urgency, self-dismissing notification carrying the launcher
/// command's summary and captured output.
pub struct ProviderNotificationEmitter {
    notifications: Arc<NotificationsProvider>,
}

impl ProviderNotificationEmitter {
    /// Build an emitter from the shared notifications provider, the same handle
    /// the timer notifier holds.
    pub fn new(notifications: Arc<NotificationsProvider>) -> Self {
        Self { notifications }
    }
}

#[async_trait]
impl NotificationEmitter for ProviderNotificationEmitter {
    async fn emit(&self, summary: &str, body: &str) {
        self.notifications
            .add_internal_notification(
                "quantum".to_string(),
                summary.to_string(),
                body.to_string(),
                Some("utilities-terminal".to_string()),
                COMMAND_OUTPUT_TIMEOUT_MS,
            )
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emit_pushes_notification() {
        let notifications = Arc::new(NotificationsProvider::new());
        let before = notifications.count().await;
        let emitter = ProviderNotificationEmitter::new(notifications.clone());

        emitter.emit("my summary", "my body").await;

        let after = notifications.count().await;
        assert_eq!(after, before + 1);
        let created = notifications
            .get_all()
            .await
            .into_iter()
            .find(|notification| notification.summary == "my summary")
            .expect("created notification present");
        assert_eq!(created.summary, "my summary");
        assert_eq!(created.body, "my body");
    }
}
