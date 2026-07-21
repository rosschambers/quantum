//! Clipboard service use case.
//!
//! A thin orchestration layer over the domain [`ClipboardStore`] port for the
//! `clipboard.*` IPC surface. Today it exposes only `clear` (the search and
//! recopy paths run through the `clipboard` provider); routing it here keeps the
//! dispatcher free of any infrastructure dependency, mirroring how
//! [`crate::TimerService`] wraps the timer store. This crate depends only on
//! `quantum_domain`; it never touches infrastructure directly.

use std::sync::Arc;

use quantum_domain::{ClipboardError, ClipboardStore};

/// Orchestrates clipboard-history operations that do not belong to the provider
/// search path. Cheap to clone (a single `Arc`).
#[derive(Clone)]
pub struct ClipboardService {
    store: Arc<dyn ClipboardStore>,
}

impl ClipboardService {
    /// Construct a service over the shared clipboard `store`.
    pub fn new(store: Arc<dyn ClipboardStore>) -> Self {
        Self { store }
    }

    /// Clear the entire clipboard history, deleting every entry and its blob.
    pub async fn clear(&self) -> Result<(), ClipboardError> {
        self.store.clear().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::{ClipboardData, ClipboardEntry};
    use std::sync::Mutex as StdMutex;

    struct FakeStore {
        cleared: StdMutex<usize>,
    }

    #[async_trait]
    impl ClipboardStore for FakeStore {
        async fn load(&self) -> Result<ClipboardData, ClipboardError> {
            Ok(ClipboardData::default())
        }
        async fn append(
            &self,
            _entry: ClipboardEntry,
            _blob: Option<Vec<u8>>,
        ) -> Result<(), ClipboardError> {
            Ok(())
        }
        async fn remove(&self, _id: &str) -> Result<(), ClipboardError> {
            Ok(())
        }
        async fn clear(&self) -> Result<(), ClipboardError> {
            *self.cleared.lock().expect("cleared") += 1;
            Ok(())
        }
    }

    #[tokio::test]
    async fn clear_delegates_to_store() {
        let store = Arc::new(FakeStore {
            cleared: StdMutex::new(0),
        });
        let service = ClipboardService::new(store.clone());
        service.clear().await.unwrap();
        assert_eq!(*store.cleared.lock().expect("cleared"), 1);
    }
}
