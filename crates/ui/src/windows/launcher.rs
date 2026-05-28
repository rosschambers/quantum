//! Launcher window with gtk4-layer-shell anchoring.

use crate::dispatcher::IpcDispatcher;
use gtk4::prelude::*;
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use quantum_domain::ports::ThemeStore;
use std::sync::Arc;
use webkit6::{prelude::*, WebView};

/// The launcher window - a top-layer panel window anchored on-demand.
pub struct LauncherWindow {
    window: gtk4::ApplicationWindow,
    #[allow(dead_code)] // held to keep the webview alive with the window
    webview: WebView,
    #[allow(dead_code)]
    dispatcher: Arc<dyn IpcDispatcher>,
    #[allow(dead_code)]
    theme_store: Arc<dyn ThemeStore>,
}

impl LauncherWindow {
    /// Create a new launcher window.
    pub fn new(
        app: &gtk4::Application,
        _dispatcher: Arc<dyn IpcDispatcher>,
        _theme_store: Arc<dyn ThemeStore>,
    ) -> Self {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .decorated(false)
            .default_width(640)
            .default_height(480)
            .resizable(false)
            .build();

        // Initialize layer shell
        window.init_layer_shell();
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::OnDemand);
        window.set_namespace("quantum-launcher");
        window.set_exclusive_zone(-1); // Don't reserve space

        // Create and embed WebView
        let webview = webkit6::WebView::new();
        webview.load_uri("quantum://theme/default/views/launcher/index.html");
        window.set_child(Some(&webview));

        Self {
            window,
            webview,
            dispatcher: _dispatcher,
            theme_store: _theme_store,
        }
    }
}

impl crate::registry::WindowOps for LauncherWindow {
    /// Show the launcher window.
    fn show(&mut self) {
        self.window.set_visible(true);
    }

    /// Hide the launcher window.
    fn hide(&mut self) {
        self.window.set_visible(false);
    }

    /// Toggle the launcher window visibility.
    fn toggle(&mut self) {
        if self.window.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }
}
