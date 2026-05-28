use async_trait::async_trait;
use quantum_domain::{ports::WindowHost, DomainError, WindowMode};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::messages::WindowRequest;

pub struct GtkWindowHost {
    tx: UnboundedSender<WindowRequest>,
}

impl GtkWindowHost {
    pub fn new() -> (Self, UnboundedReceiver<WindowRequest>) {
        let (tx, rx) = unbounded_channel();
        (Self { tx }, rx)
    }
}

#[async_trait]
impl WindowHost for GtkWindowHost {
    async fn open(&self, view: &str, mode: WindowMode) -> Result<(), DomainError> {
        self.tx
            .send(WindowRequest::Open {
                view: view.to_string(),
                mode,
            })
            .map_err(|_| DomainError::Unsupported("window host receiver dropped".into()))
    }
}

/// Dummy window host for headless mode.
/// Does nothing but satisfies the WindowHost trait.
pub struct DummyWindowHost;

impl DummyWindowHost {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DummyWindowHost {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WindowHost for DummyWindowHost {
    async fn open(&self, _view: &str, _mode: WindowMode) -> Result<(), DomainError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dummy_window_host_creates() {
        let _host = DummyWindowHost::new();
    }

    #[tokio::test]
    async fn dummy_window_host_open() {
        let host = DummyWindowHost::new();
        let result = host.open("launcher", WindowMode::Toggle).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn sends_open_request_on_channel() {
        let (host, mut rx) = GtkWindowHost::new();
        host.open("launcher", WindowMode::Toggle).await.unwrap();
        let msg = rx.recv().await.expect("message");
        match msg {
            WindowRequest::Open { view, mode } => {
                assert_eq!(view, "launcher");
                assert!(matches!(mode, WindowMode::Toggle));
            }
        }
    }

    #[tokio::test]
    async fn open_fails_when_receiver_dropped() {
        let (host, rx) = GtkWindowHost::new();
        drop(rx);
        let result = host.open("launcher", WindowMode::Show).await;
        assert!(result.is_err());
    }
}
