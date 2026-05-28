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

        async fn hide(&self, view: &str) -> std::result::Result<(), DomainError> {
            if self.should_fail {
                Err(DomainError::ActionFailed {
                    reason: "hide failed".to_string(),
                })
            } else {
                self.calls
                    .lock()
                    .unwrap()
                    .push((view.to_string(), "hide".to_string()));
                Ok(())
            }
        }

        async fn toggle(&self, view: &str) -> std::result::Result<(), DomainError> {
            if self.should_fail {
                Err(DomainError::ActionFailed {
                    reason: "toggle failed".to_string(),
                })
            } else {
                self.calls
                    .lock()
                    .unwrap()
                    .push((view.to_string(), "toggle".to_string()));
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
    async fn hide_view_calls_window_host_hide() {
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
    async fn toggle_view_calls_window_host_toggle() {
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
