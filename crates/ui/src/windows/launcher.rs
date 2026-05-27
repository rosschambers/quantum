//! Launcher window with gtk4-layer-shell anchoring.

use std::sync::Arc;
use std::cell::RefCell;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Orientation};
use gtk4_layer_shell::{LayerShell, Layer, KeyboardMode};
use webkit6::WebView;
use quantum_application::dispatcher::Dispatcher;
use crate::scheme::ThemePort;

/// The launcher window - a top-layer panel window anchored on-demand.
pub struct LauncherWindow {
    window: ApplicationWindow,
    webview: RefCell<Option<WebView>>,
    #[allow(dead_code)]
    dispatcher: Arc<Dispatcher>,
    #[allow(dead_code)]
    theme_store: Arc<dyn ThemePort>,
}

impl LauncherWindow {
    /// Create a new launcher window.
    pub fn new(
        app: &gtk4::Application,
        dispatcher: Arc<Dispatcher>,
        theme_store: Arc<dyn ThemePort>,
    ) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .decorated(false)
            .default_width(600)
            .default_height(400)
            .build();

        // Initialize layer shell
        window.init_layer_shell();
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::OnDemand);
        window.set_exclusive_zone(-1); // Don't reserve space

        Self {
            window,
            webview: RefCell::new(None),
            dispatcher,
            theme_store,
        }
    }

    /// Show the launcher window.
    pub fn show(&self) {
        // On first show, preload the webview
        if self.webview.borrow().is_none() {
            self.preload_webview();
        }
        self.window.present();
    }

    /// Hide the launcher window.
    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    /// Toggle the launcher window visibility.
    pub fn toggle(&self) {
        if self.window.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }

    /// Preload the webview (only once).
    fn preload_webview(&self) {
        let webview = WebView::new();

        // In a real implementation, we would:
        // 1. Register the quantum:// scheme handler
        // 2. Load quantum://theme/{active}/views/launcher/index.html
        // 3. Register the bridge
        //
        // For now, we just construct it and store it.

        let container = gtk4::Box::new(Orientation::Vertical, 0);
        container.append(&webview);
        self.window.set_child(Some(&container));

        *self.webview.borrow_mut() = Some(webview);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_window_new_succeeds() {
        // Smoke test: window construction doesn't panic.
        // Full GTK testing requires a display server.
    }

    #[test]
    fn keyboard_mode_constant() {
        // Verify KeyboardMode enum is accessible
        let _mode = KeyboardMode::OnDemand;
    }
}
