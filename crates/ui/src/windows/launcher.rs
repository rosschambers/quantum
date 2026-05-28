//! Launcher window with gtk4-layer-shell anchoring.

use crate::dispatcher::IpcDispatcher;
use gtk4::prelude::*;
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use quantum_domain::ports::ThemeStore;
use std::sync::Arc;
use tokio::runtime::Handle;
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
    visible: bool,
}

impl LauncherWindow {
    /// Create a new launcher window.
    pub fn new(
        app: &gtk4::Application,
        dispatcher: Arc<dyn IpcDispatcher>,
        theme_store: Arc<dyn ThemeStore>,
        runtime: Handle,
    ) -> Self {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .decorated(false)
            .default_width(600)
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

        // Register the bridge to wire JS messages to the dispatcher
        crate::bridge::register_bridge(&webview, dispatcher.clone(), runtime);

        Self {
            window,
            webview,
            dispatcher,
            theme_store,
            visible: false,
        }
    }
}

impl crate::registry::WindowOps for LauncherWindow {
    /// Show the launcher window.
    fn show(&mut self) {
        // Use exclusive keyboard mode when shown so Hyprland routes all keystrokes to the launcher.
        self.window.set_keyboard_mode(KeyboardMode::Exclusive);
        self.window.set_visible(true);
        // Focus the WebView so typing immediately reaches the search input.
        self.webview.grab_focus();
        self.visible = true;
    }

    /// Hide the launcher window.
    fn hide(&mut self) {
        // Revert to on-demand keyboard mode before hiding so the next compositor focus isn't redirected.
        self.window.set_keyboard_mode(KeyboardMode::OnDemand);
        self.window.set_visible(false);
        self.visible = false;
    }

    /// Toggle the launcher window visibility.
    fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }
}
