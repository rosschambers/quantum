use crate::Result;
use quantum_domain::{EventBus, ThemeStore};
use std::sync::Arc;

pub struct ReloadThemeUseCase {
    theme_store: Arc<dyn ThemeStore>,
    event_bus: Arc<dyn EventBus>,
}

impl ReloadThemeUseCase {
    pub fn new(theme_store: Arc<dyn ThemeStore>, event_bus: Arc<dyn EventBus>) -> Self {
        Self {
            theme_store,
            event_bus,
        }
    }

    pub async fn execute(&self) -> Result<()> {
        self.theme_store
            .reload()
            .await
            .map_err(crate::ApplicationError::Domain)?;
        self.event_bus
            .publish("theme.reloaded", "{}")
            .await
            .map_err(crate::ApplicationError::Domain)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::DomainError;
    use std::sync::Mutex;

    struct FakeThemeStore {
        reload_called: Arc<Mutex<bool>>,
        should_fail: bool,
    }

    #[async_trait]
    impl ThemeStore for FakeThemeStore {
        async fn load_theme(&self, _name: &str) -> std::result::Result<(), DomainError> {
            Ok(())
        }

        async fn reload(&self) -> std::result::Result<(), DomainError> {
            if self.should_fail {
                Err(DomainError::ActionFailed {
                    reason: "reload failed".to_string(),
                })
            } else {
                *self.reload_called.lock().unwrap() = true;
                Ok(())
            }
        }

        fn get_file(&self, _theme_name: &str, _path: &str) -> Option<Vec<u8>> {
            None
        }

        fn get_asset(&self, _path: &str) -> Option<Vec<u8>> {
            None
        }

        fn get_plugin_file(&self, _plugin_name: &str, _path: &str) -> Option<Vec<u8>> {
            None
        }

        fn resolved_tokens(&self) -> std::collections::HashMap<String, String> {
            std::collections::HashMap::new()
        }
    }

    struct FakeEventBus {
        events: Arc<Mutex<Vec<String>>>,
        should_fail: bool,
    }

    #[async_trait]
    impl EventBus for FakeEventBus {
        async fn publish(
            &self,
            event: &str,
            _payload: &str,
        ) -> std::result::Result<(), DomainError> {
            if self.should_fail {
                Err(DomainError::ActionFailed {
                    reason: "publish failed".to_string(),
                })
            } else {
                self.events.lock().unwrap().push(event.to_string());
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn reloads_theme_and_publishes_event() {
        let reload_called = Arc::new(Mutex::new(false));
        let events = Arc::new(Mutex::new(Vec::new()));

        let theme_store = Arc::new(FakeThemeStore {
            reload_called: reload_called.clone(),
            should_fail: false,
        });

        let event_bus = Arc::new(FakeEventBus {
            events: events.clone(),
            should_fail: false,
        });

        let uc = ReloadThemeUseCase::new(theme_store, event_bus);
        let result = uc.execute().await;

        assert!(result.is_ok());
        assert!(*reload_called.lock().unwrap());
        assert_eq!(events.lock().unwrap().len(), 1);
        assert_eq!(events.lock().unwrap()[0], "theme.reloaded");
    }

    #[tokio::test]
    async fn returns_error_when_theme_reload_fails() {
        let reload_called = Arc::new(Mutex::new(false));

        let theme_store = Arc::new(FakeThemeStore {
            reload_called: reload_called.clone(),
            should_fail: true,
        });

        let event_bus = Arc::new(FakeEventBus {
            events: Arc::new(Mutex::new(Vec::new())),
            should_fail: false,
        });

        let uc = ReloadThemeUseCase::new(theme_store, event_bus);
        let result = uc.execute().await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn returns_error_when_event_publish_fails() {
        let reload_called = Arc::new(Mutex::new(false));

        let theme_store = Arc::new(FakeThemeStore {
            reload_called: reload_called.clone(),
            should_fail: false,
        });

        let event_bus = Arc::new(FakeEventBus {
            events: Arc::new(Mutex::new(Vec::new())),
            should_fail: true,
        });

        let uc = ReloadThemeUseCase::new(theme_store, event_bus);
        let result = uc.execute().await;

        assert!(result.is_err());
    }
}
