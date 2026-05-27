//! GTK window host implementation bridging Tokio and GTK main loops.

use std::sync::Arc;
use std::collections::HashMap;
use std::cell::RefCell;
use crate::windows::LauncherWindow;
use crate::scheme::ThemePort;
use quantum_application::dispatcher::Dispatcher;

/// Window open mode.
#[derive(Debug, Clone, Copy)]
pub enum WindowMode {
    Toggle,
    Show,
    Hide,
}

/// GTK-based window host implementation.
/// 
/// This type bridges the Tokio async runtime (where application code runs)
/// and the GTK main loop (which is single-threaded). Window operations are
/// dispatched onto the GTK main context via `glib::MainContext::default().spawn_local()`.
pub struct GtkWindowHost {
    windows: RefCell<HashMap<String, Arc<LauncherWindow>>>,
    dispatcher: Arc<Dispatcher>,
    theme_store: Arc<dyn ThemePort>,
    gtk_app: gtk4::Application,
}

impl GtkWindowHost {
    /// Create a new window host.
    pub fn new(
        gtk_app: gtk4::Application,
        dispatcher: Arc<Dispatcher>,
        theme_store: Arc<dyn ThemePort>,
    ) -> Self {
        Self {
            windows: RefCell::new(HashMap::new()),
            dispatcher,
            theme_store,
            gtk_app,
        }
    }

    /// Open or toggle a window by name.
    /// This is safe to call from async code; it dispatches onto the GTK main context.
    pub async fn open(&self, view_name: &str, mode: WindowMode) -> Result<(), String> {
        let view = view_name.to_string();
        let dispatcher = self.dispatcher.clone();
        let theme_store = self.theme_store.clone();
        let gtk_app = self.gtk_app.clone();
        let windows = self.windows.clone();

        // Dispatch onto GTK main context
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let view_clone = view.clone();
        glib::MainContext::default().spawn_local(async move {
            let mut windows_mut = windows.borrow_mut();

            // Get or create the window
            let window = windows_mut
                .entry(view_clone.clone())
                .or_insert_with(|| {
                    Arc::new(LauncherWindow::new(&gtk_app, dispatcher, theme_store))
                })
                .clone();

            // Apply the mode
            match mode {
                WindowMode::Toggle => window.toggle(),
                WindowMode::Show => window.show(),
                WindowMode::Hide => window.hide(),
            }

            let _ = tx.send(()).await;
        });

        // Wait for the GTK operation to complete
        rx.recv().await.ok_or_else(|| "window operation failed".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_mode_enum_exists() {
        let _show = WindowMode::Show;
        let _hide = WindowMode::Hide;
        let _toggle = WindowMode::Toggle;
    }
}
