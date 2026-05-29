use crate::{Action, DomainError, Match, ProviderId, Query};
use async_trait::async_trait;
use std::sync::Arc;

/// Capabilities of a provider.
#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    pub searchable: bool,
    pub streamable: bool,
}

/// Outcome of invoking an action.
#[derive(Debug, Clone)]
pub struct ActionOutcome {
    pub message: Option<String>,
}

/// A provider source that can search and invoke actions.
#[async_trait]
pub trait ProviderSource: Send + Sync {
    fn id(&self) -> &ProviderId;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn search(&self, q: &Query) -> Result<Vec<Match>, DomainError>;
    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError>;
    /// Optional event stream. Providers that publish state updates return a
    /// boxed stream of serialized events here. Default returns `None`,
    /// signalling the provider does not expose subscriptions.
    ///
    /// Events are opaque `serde_json::Value`s — each provider serializes its
    /// own state struct and the dispatcher forwards the JSON to subscribers.
    fn subscribe(&self) -> Option<futures::stream::BoxStream<'static, serde_json::Value>> {
        None
    }
}

/// Registry for looking up providers.
#[async_trait]
pub trait ProviderRegistry: Send + Sync {
    async fn list(&self) -> Vec<ProviderId>;
    async fn get(&self, id: &ProviderId) -> Option<Arc<dyn ProviderSource>>;
}

/// Configuration storage.
#[async_trait]
pub trait ConfigStore: Send + Sync {
    async fn load(&self) -> Result<(), DomainError>;
    async fn get(&self, key: &str) -> Option<String>;
}

/// Theme storage and resolution.
#[async_trait]
pub trait ThemeStore: Send + Sync {
    async fn load_theme(&self, name: &str) -> Result<(), DomainError>;
    async fn reload(&self) -> Result<(), DomainError>;
    /// Get a file from a theme by name and relative path. Returns None if not found.
    /// This is a synchronous method since URI handlers run on the GTK thread.
    fn get_file(&self, theme_name: &str, path: &str) -> Option<Vec<u8>>;
    /// Get an asset file from the active theme. Returns None if not found.
    /// This is a synchronous method since URI handlers run on the GTK thread.
    fn get_asset(&self, path: &str) -> Option<Vec<u8>>;
    /// Get resolved tokens for CSS variable injection.
    /// This is a synchronous method for use in URI handlers on the GTK thread.
    fn resolved_tokens(&self) -> std::collections::HashMap<String, String>;
}

/// Event bus for domain events.
#[async_trait]
pub trait EventBus: Send + Sync {
    async fn publish(&self, event: &str, payload: &str) -> Result<(), DomainError>;
    async fn subscribe(&self, event: &str) -> Result<(), DomainError>;
}

/// Shell output from command execution.
#[derive(Debug, Clone)]
pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

/// Shell execution.
#[async_trait]
pub trait ShellExecutor: Send + Sync {
    async fn execute(&self, command: &[String]) -> Result<String, DomainError>;
    async fn run_with_timeout(
        &self,
        command: &[String],
        timeout_ms: u64,
    ) -> Result<ShellOutput, DomainError>;
    async fn spawn_detached(&self, command: &[String]) -> Result<(), DomainError>;
}

/// Hyprland IPC client.
#[async_trait]
pub trait HyprlandClient: Send + Sync {
    async fn command(&self, cmd: &str) -> Result<String, DomainError>;
}

/// Window host for managing windows.
#[async_trait]
pub trait WindowHost: Send + Sync {
    async fn open(&self, view: &str, mode: crate::WindowMode) -> Result<(), DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeProvider {
        id: ProviderId,
    }

    #[async_trait]
    impl ProviderSource for FakeProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                searchable: true,
                streamable: false,
            }
        }

        async fn search(&self, _q: &Query) -> Result<Vec<Match>, DomainError> {
            Ok(vec![])
        }

        async fn invoke(&self, _a: &Action) -> Result<ActionOutcome, DomainError> {
            Ok(ActionOutcome { message: None })
        }
    }

    #[tokio::test]
    async fn fake_provider_returns_empty_search() {
        let p = FakeProvider {
            id: ProviderId::from("apps"),
        };
        let q = Query::new("x");
        let r = p.search(&q).await.unwrap();
        assert!(r.is_empty());
    }

    #[tokio::test]
    async fn fake_provider_has_correct_id() {
        let p = FakeProvider {
            id: ProviderId::from("test-provider"),
        };
        assert_eq!(p.id(), &ProviderId::from("test-provider"));
    }

    #[tokio::test]
    async fn fake_provider_capabilities() {
        let p = FakeProvider {
            id: ProviderId::from("apps"),
        };
        let caps = p.capabilities();
        assert!(caps.searchable);
        assert!(!caps.streamable);
    }
}

#[cfg(test)]
mod subscribe_tests {
    use super::*;
    use futures::stream::{self, StreamExt};

    struct FakeSubscriber {
        id: ProviderId,
    }

    #[async_trait]
    impl ProviderSource for FakeSubscriber {
        fn id(&self) -> &ProviderId {
            &self.id
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                searchable: false,
                streamable: true,
            }
        }
        async fn search(&self, _: &Query) -> Result<Vec<Match>, DomainError> {
            Ok(vec![])
        }
        async fn invoke(&self, _: &Action) -> Result<ActionOutcome, DomainError> {
            Ok(ActionOutcome { message: None })
        }
        fn subscribe(&self) -> Option<futures::stream::BoxStream<'static, serde_json::Value>> {
            Some(stream::iter(vec![serde_json::json!({"x": 1})]).boxed())
        }
    }

    #[tokio::test]
    async fn subscribe_returns_stream_when_supported() {
        let p = FakeSubscriber { id: "fake".into() };
        let mut stream = p.subscribe().expect("stream");
        let event = stream.next().await.expect("event");
        assert_eq!(event, serde_json::json!({"x": 1}));
    }
}
