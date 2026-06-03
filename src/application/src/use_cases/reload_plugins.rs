//! `plugin.reload` use case.
//!
//! Re-walks the plugins directory through the `PluginCatalog` port and
//! reports how many plugins were discovered. v1 limitation: this does
//! NOT atomically swap registry entries or stop in-flight polling tasks.
//! A daemon restart is still required to pick up newly-added plugins or
//! stop tasks for plugins removed since startup. The IPC method is
//! useful today to verify that the daemon *can see* a plugin folder the
//! user just dropped in, without going through a full restart cycle.

use crate::Result;
use quantum_domain::PluginCatalog;
use std::sync::Arc;

pub struct ReloadPluginsUseCase {
    catalog: Arc<dyn PluginCatalog>,
}

impl ReloadPluginsUseCase {
    pub fn new(catalog: Arc<dyn PluginCatalog>) -> Self {
        Self { catalog }
    }

    /// Discover plugins and return the count. Errors propagate from
    /// the catalog (typically I/O failures reading the plugins dir).
    pub async fn execute(&self) -> Result<usize> {
        self.catalog
            .discover()
            .await
            .map_err(crate::ApplicationError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::DomainError;

    struct FakeCatalog {
        count: usize,
    }

    #[async_trait]
    impl PluginCatalog for FakeCatalog {
        async fn discover(&self) -> std::result::Result<usize, DomainError> {
            Ok(self.count)
        }
    }

    struct FailingCatalog;

    #[async_trait]
    impl PluginCatalog for FailingCatalog {
        async fn discover(&self) -> std::result::Result<usize, DomainError> {
            Err(DomainError::Unsupported("disk read failed".into()))
        }
    }

    #[tokio::test]
    async fn returns_count_from_catalog() {
        let uc = ReloadPluginsUseCase::new(Arc::new(FakeCatalog { count: 3 }));
        let n = uc.execute().await.expect("ok");
        assert_eq!(n, 3);
    }

    #[tokio::test]
    async fn propagates_catalog_error() {
        let uc = ReloadPluginsUseCase::new(Arc::new(FailingCatalog));
        let err = uc.execute().await.expect_err("fails");
        assert!(format!("{err}").contains("disk") || format!("{err:?}").contains("disk"));
    }
}
