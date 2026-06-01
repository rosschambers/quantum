use crate::Result;
use quantum_domain::{WindowHost, WindowMode};
use std::sync::Arc;

pub struct OpenViewUseCase {
    window_host: Arc<dyn WindowHost>,
}

impl OpenViewUseCase {
    pub fn new(window_host: Arc<dyn WindowHost>) -> Self {
        Self { window_host }
    }

    pub async fn execute(&self, view_name: String, mode: WindowMode) -> Result<()> {
        self.window_host
            .open(&view_name, mode)
            .await
            .map_err(crate::ApplicationError::Domain)
    }

    /// Forward a runtime resize request to the window host. Used by the
    /// `view.set_height` IPC method so the bar can grow when a popover
    /// opens.
    pub async fn set_height(&self, view_name: String, height: u32) -> Result<()> {
        self.window_host
            .set_view_height(&view_name, height)
            .await
            .map_err(crate::ApplicationError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::DomainError;
    use std::sync::Mutex;

    #[derive(Clone)]
    struct FakeWindowHost {
        calls: Arc<Mutex<Vec<(String, String)>>>,
        should_fail: bool,
    }

    #[async_trait]
    impl WindowHost for FakeWindowHost {
        async fn open(&self, view: &str, mode: WindowMode) -> std::result::Result<(), DomainError> {
            if self.should_fail {
                Err(DomainError::ActionFailed {
                    reason: "open failed".to_string(),
                })
            } else {
                let mode_str = format!("{:?}", mode);
                self.calls
                    .lock()
                    .unwrap()
                    .push((view.to_string(), mode_str));
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn open_view_calls_window_host_open() {
        let host = FakeWindowHost {
            calls: Arc::new(Mutex::new(Vec::new())),
            should_fail: false,
        };

        let uc = OpenViewUseCase::new(Arc::new(host.clone()));
        let result = uc.execute("launcher".to_string(), WindowMode::Show).await;

        assert!(result.is_ok());
        let calls = host.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "launcher".to_string());
        assert!(calls[0].1.contains("Show"));
    }

    #[tokio::test]
    async fn execute_with_hide_mode_calls_window_host_open_with_hide() {
        let host = FakeWindowHost {
            calls: Arc::new(Mutex::new(Vec::new())),
            should_fail: false,
        };

        let uc = OpenViewUseCase::new(Arc::new(host.clone()));
        let result = uc.execute("launcher".to_string(), WindowMode::Hide).await;

        assert!(result.is_ok());
        let calls = host.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "launcher".to_string());
        assert!(calls[0].1.contains("Hide"));
    }

    #[tokio::test]
    async fn execute_with_toggle_mode_calls_window_host_open_with_toggle() {
        let host = FakeWindowHost {
            calls: Arc::new(Mutex::new(Vec::new())),
            should_fail: false,
        };

        let uc = OpenViewUseCase::new(Arc::new(host.clone()));
        let result = uc.execute("launcher".to_string(), WindowMode::Toggle).await;

        assert!(result.is_ok());
        let calls = host.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "launcher".to_string());
        assert!(calls[0].1.contains("Toggle"));
    }

    #[tokio::test]
    async fn returns_error_on_window_host_failure() {
        let host = FakeWindowHost {
            calls: Arc::new(Mutex::new(Vec::new())),
            should_fail: true,
        };

        let uc = OpenViewUseCase::new(Arc::new(host.clone()));
        let result = uc.execute("launcher".to_string(), WindowMode::Show).await;

        assert!(result.is_err());
    }
}
