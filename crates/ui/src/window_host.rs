//! GTK window host implementation bridging Tokio and GTK main loops.
//!
//! `GtkWindowHost` is intentionally not `Send`/`Sync`: GTK objects are
//! single-thread-affine and must only be touched from the GTK main context.
//! All cross-thread coordination here goes through `glib::MainContext::default()
//! .spawn_local`, which posts work back to the GTK thread. The `clippy::arc_with_non_send_sync`
//! lints below are therefore acknowledged design constraints rather than bugs;
//! we silence them at the use sites where the `Arc` only ever travels via
//! `spawn_local` (i.e. stays on the GTK thread).

use crate::scheme::ThemePort;
use crate::windows::LauncherWindow;
use async_trait::async_trait;
use quantum_application::dispatcher::Dispatcher;
use quantum_domain::{DomainError, WindowHost};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

/// GTK-based window host implementation (for manual testing only).
///
/// This type bridges the Tokio async runtime (where application code runs)
/// and the GTK main loop (which is single-threaded). However, GTK types
/// are not Send+Sync, so this cannot be used in production Arc<dyn WindowHost>.
/// For headless/e2e testing, use DummyWindowHost instead.
pub struct GtkWindowHost {
    windows: Arc<RwLock<HashMap<String, Arc<LauncherWindow>>>>,
    dispatcher: Arc<Dispatcher>,
    theme_store: Arc<dyn ThemePort>,
    gtk_app: gtk4::Application,
}

impl GtkWindowHost {
    /// Create a new window host (manual/UI testing only).
    pub fn new(
        gtk_app: gtk4::Application,
        dispatcher: Arc<Dispatcher>,
        theme_store: Arc<dyn ThemePort>,
    ) -> Self {
        // Holding GTK objects in `Arc` is intentional: this host runs only
        // from the GTK thread (see module docs), so non-Send/Sync `Arc` use is
        // safe in this scoped context.
        #[allow(clippy::arc_with_non_send_sync)]
        let windows = Arc::new(RwLock::new(HashMap::new()));
        Self {
            windows,
            dispatcher,
            theme_store,
            gtk_app,
        }
    }

    /// Show a window by name.
    pub async fn show(&self, view_name: &str) -> Result<(), DomainError> {
        let view = view_name.to_string();
        let dispatcher = self.dispatcher.clone();
        let theme_store = self.theme_store.clone();
        let gtk_app = self.gtk_app.clone();
        let windows = self.windows.clone();

        // Dispatch onto GTK main context
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let view_clone = view.clone();
        glib::MainContext::default().spawn_local(async move {
            let window = {
                let mut windows_mut = windows.write().unwrap();
                windows_mut
                    .entry(view_clone.clone())
                    .or_insert_with(|| {
                        // `LauncherWindow` is not Send/Sync (GTK affinity);
                        // this Arc stays on the GTK main thread.
                        #[allow(clippy::arc_with_non_send_sync)]
                        Arc::new(LauncherWindow::new(&gtk_app, dispatcher, theme_store))
                    })
                    .clone()
            };

            window.show();
            let _ = tx.send(()).await;
        });

        // Wait for the GTK operation to complete
        rx.recv()
            .await
            .ok_or_else(|| DomainError::Unsupported("window operation failed".to_string()))
    }

    /// Hide a window by name.
    pub async fn hide(&self, view_name: &str) -> Result<(), DomainError> {
        let view = view_name.to_string();
        let windows = self.windows.clone();

        // Dispatch onto GTK main context
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let view_clone = view.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Ok(windows_read) = windows.read() {
                if let Some(window) = windows_read.get(&view_clone) {
                    window.hide();
                }
            }
            let _ = tx.send(()).await;
        });

        // Wait for the GTK operation to complete
        rx.recv()
            .await
            .ok_or_else(|| DomainError::Unsupported("window operation failed".to_string()))
    }

    /// Toggle a window by name.
    pub async fn toggle(&self, view_name: &str) -> Result<(), DomainError> {
        let view = view_name.to_string();
        let dispatcher = self.dispatcher.clone();
        let theme_store = self.theme_store.clone();
        let gtk_app = self.gtk_app.clone();
        let windows = self.windows.clone();

        // Dispatch onto GTK main context
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let view_clone = view.clone();
        glib::MainContext::default().spawn_local(async move {
            let window = {
                let mut windows_mut = windows.write().unwrap();
                windows_mut
                    .entry(view_clone.clone())
                    .or_insert_with(|| {
                        // `LauncherWindow` is not Send/Sync (GTK affinity);
                        // this Arc stays on the GTK main thread.
                        #[allow(clippy::arc_with_non_send_sync)]
                        Arc::new(LauncherWindow::new(&gtk_app, dispatcher, theme_store))
                    })
                    .clone()
            };

            window.toggle();
            let _ = tx.send(()).await;
        });

        // Wait for the GTK operation to complete
        rx.recv()
            .await
            .ok_or_else(|| DomainError::Unsupported("window operation failed".to_string()))
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
    async fn open(&self, _view: &str) -> Result<(), DomainError> {
        Ok(())
    }

    async fn hide(&self, _view: &str) -> Result<(), DomainError> {
        Ok(())
    }

    async fn toggle(&self, _view: &str) -> Result<(), DomainError> {
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
        let result = host.open("launcher").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dummy_window_host_hide() {
        let host = DummyWindowHost::new();
        let result = host.hide("launcher").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dummy_window_host_toggle() {
        let host = DummyWindowHost::new();
        let result = host.toggle("launcher").await;
        assert!(result.is_ok());
    }
}
