use crate::Result;
use quantum_domain::{WindowHost, WindowInputRegion, WindowMode};
use std::sync::Arc;

pub struct OpenViewUseCase {
    window_host: Arc<dyn WindowHost>,
}

impl OpenViewUseCase {
    pub fn new(window_host: Arc<dyn WindowHost>) -> Self {
        Self { window_host }
    }

    pub async fn execute(
        &self,
        view_name: String,
        mode: WindowMode,
        args: Option<serde_json::Value>,
    ) -> Result<()> {
        self.window_host
            .open(&view_name, mode, args)
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

    /// Forward a runtime input-region request to the window host. Used by the
    /// `view.set_input_region` IPC method so the bar can clip its full-height
    /// surface's pointer input to the visible strip (plus any open menu).
    /// `None` resets the region to the strip-only default.
    pub async fn set_input_region(
        &self,
        view_name: String,
        region: Option<WindowInputRegion>,
    ) -> Result<()> {
        self.window_host
            .set_view_input_region(&view_name, region)
            .await
            .map_err(crate::ApplicationError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::{DomainError, WindowInputRegion};
    use std::sync::Mutex;

    type InputRegionCalls = Arc<Mutex<Vec<(String, Option<WindowInputRegion>)>>>;

    #[derive(Clone, Default)]
    struct FakeWindowHost {
        calls: Arc<Mutex<Vec<(String, String)>>>,
        input_region_calls: InputRegionCalls,
        should_fail: bool,
    }

    #[async_trait]
    impl WindowHost for FakeWindowHost {
        async fn open(
            &self,
            view: &str,
            mode: WindowMode,
            _args: Option<serde_json::Value>,
        ) -> std::result::Result<(), DomainError> {
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

        async fn set_view_height(
            &self,
            _view: &str,
            _height: u32,
        ) -> std::result::Result<(), DomainError> {
            Ok(())
        }

        async fn set_view_input_region(
            &self,
            view: &str,
            region: Option<WindowInputRegion>,
        ) -> std::result::Result<(), DomainError> {
            self.input_region_calls
                .lock()
                .unwrap()
                .push((view.to_string(), region));
            Ok(())
        }
    }

    #[tokio::test]
    async fn open_view_calls_window_host_open() {
        let host = FakeWindowHost {
            calls: Arc::new(Mutex::new(Vec::new())),
            should_fail: false,
            ..Default::default()
        };

        let uc = OpenViewUseCase::new(Arc::new(host.clone()));
        let result = uc
            .execute("launcher".to_string(), WindowMode::Show, None)
            .await;

        assert!(result.is_ok());
        let calls = host.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "launcher".to_string());
        assert!(calls[0].1.contains("Show"));
    }

    #[tokio::test]
    async fn set_input_region_forwards_to_window_host() {
        let host = FakeWindowHost::default();

        let uc = OpenViewUseCase::new(Arc::new(host.clone()));
        let region = WindowInputRegion {
            x: 0,
            y: 0,
            width: 300,
            height: 32,
        };
        let result = uc
            .set_input_region("plugin/bar/bar".to_string(), Some(region))
            .await;

        assert!(result.is_ok());
        let calls = host.input_region_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "plugin/bar/bar".to_string());
        assert_eq!(calls[0].1, Some(region));
    }

    #[tokio::test]
    async fn execute_with_hide_mode_calls_window_host_open_with_hide() {
        let host = FakeWindowHost {
            calls: Arc::new(Mutex::new(Vec::new())),
            should_fail: false,
            ..Default::default()
        };

        let uc = OpenViewUseCase::new(Arc::new(host.clone()));
        let result = uc
            .execute("launcher".to_string(), WindowMode::Hide, None)
            .await;

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
            ..Default::default()
        };

        let uc = OpenViewUseCase::new(Arc::new(host.clone()));
        let result = uc
            .execute("launcher".to_string(), WindowMode::Toggle, None)
            .await;

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
            ..Default::default()
        };

        let uc = OpenViewUseCase::new(Arc::new(host.clone()));
        let result = uc
            .execute("launcher".to_string(), WindowMode::Show, None)
            .await;

        assert!(result.is_err());
    }
}
