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

    /// Clone of the underlying sender. Used by the GTK loop to install
    /// the `ViewMultiplexer`, which needs to push `WindowRequest`s on
    /// the same channel that `GtkWindowHost::open` uses.
    pub fn sender(&self) -> UnboundedSender<WindowRequest> {
        self.tx.clone()
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

    async fn set_view_height(&self, view: &str, height: u32) -> Result<(), DomainError> {
        self.tx
            .send(WindowRequest::SetHeight {
                view: view.to_string(),
                height,
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

    async fn set_view_height(&self, _view: &str, _height: u32) -> Result<(), DomainError> {
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
            other => panic!("expected Open, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sends_set_height_request_on_channel() {
        let (host, mut rx) = GtkWindowHost::new();
        host.set_view_height("widgets/bar", 200).await.unwrap();
        let msg = rx.recv().await.expect("message");
        match msg {
            WindowRequest::SetHeight { view, height } => {
                assert_eq!(view, "widgets/bar");
                assert_eq!(height, 200);
            }
            other => panic!("expected SetHeight, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn open_fails_when_receiver_dropped() {
        let (host, rx) = GtkWindowHost::new();
        drop(rx);
        let result = host.open("launcher", WindowMode::Show).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn sender_clone_delivers_on_original_receiver() {
        let (host, mut rx) = GtkWindowHost::new();
        let tx = host.sender();
        tx.send(WindowRequest::Open {
            view: "widgets/bar@DP-1".to_string(),
            mode: WindowMode::Show,
        })
        .expect("send via cloned sender");
        let msg = rx.recv().await.expect("message");
        match msg {
            WindowRequest::Open { view, mode } => {
                assert_eq!(view, "widgets/bar@DP-1");
                assert!(matches!(mode, WindowMode::Show));
            }
            other => panic!("expected Open, got {other:?}"),
        }
    }
}
