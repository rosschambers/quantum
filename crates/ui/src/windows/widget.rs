//! Widget window - background-layer window for clock and other widgets.

use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use quantum_domain::ports::ThemeStore;
use std::sync::Arc;
use webkit6::{prelude::*, WebView};

/// A widget window - positioned on-screen as a background-layer widget.
pub struct WidgetWindow {
    window: gtk4::ApplicationWindow,
    #[allow(dead_code)]
    webview: WebView,
}

impl WidgetWindow {
    /// Create a new widget window.
    pub fn new(
        app: &gtk4::Application,
        view_name: String,
        _theme_store: Arc<dyn ThemeStore>,
    ) -> Self {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .decorated(false)
            .build();

        // Initialize layer shell for background
        window.init_layer_shell();
        window.set_layer(Layer::Background);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Right, true);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_exclusive_zone(0);
        window.set_namespace(&format!("quantum-widget-{}", view_name.replace('/', "-")));

        // Create and embed WebView
        let webview = webkit6::WebView::new();
        let uri = format!("quantum://theme/default/{}/index.html", view_name);
        webview.load_uri(&uri);
        window.set_child(Some(&webview));

        window.set_visible(true);

        Self { window, webview }
    }
}

impl crate::registry::WindowOps for WidgetWindow {
    fn show(&mut self) {
        self.window.set_visible(true);
    }

    fn hide(&mut self) {
        self.window.set_visible(false);
    }

    fn toggle(&mut self) {
        let v = self.window.is_visible();
        self.window.set_visible(!v);
    }
}
