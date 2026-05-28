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
    layer_shell: bool,
}

/// When set to `1`, the launcher anchors as a layer-shell `Top` surface with
/// exclusive keyboard, the production behavior. When unset (the default while
/// the daemon is in early testing), the launcher opens as a normal xdg-toplevel
/// window that the user can close or switch away from like any other window.
fn use_layer_shell() -> bool {
    std::env::var("QUANTUM_LAYER_SHELL")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

impl LauncherWindow {
    /// Create a new launcher window.
    pub fn new(
        app: &gtk4::Application,
        dispatcher: Arc<dyn IpcDispatcher>,
        theme_store: Arc<dyn ThemeStore>,
        runtime: Handle,
    ) -> Self {
        let layer_shell = use_layer_shell();

        let mut builder = gtk4::ApplicationWindow::builder()
            .application(app)
            .default_width(600)
            .default_height(420)
            .resizable(!layer_shell);

        // In layer-shell mode we want a chromeless surface. In windowed (test)
        // mode we keep decorations so the user has a close button and can
        // drag the window.
        if layer_shell {
            builder = builder.decorated(false);
        } else {
            builder = builder.decorated(true).title("Quantum");
        }

        let window = builder.build();

        if layer_shell {
            // Initialize layer shell as a Top surface centered on screen.
            window.init_layer_shell();
            window.set_layer(Layer::Top);
            window.set_keyboard_mode(KeyboardMode::OnDemand);
            window.set_namespace("quantum-launcher");
            window.set_exclusive_zone(-1); // Don't reserve space
        }
        // In windowed mode we let the compositor place it like any other
        // xdg-toplevel — Hyprland's default behavior is fine for testing.

        // Create and embed WebView
        let webview = webkit6::WebView::new();

        // Pipe JS console messages (log/warn/error) to stdout and enable the
        // web inspector under QUANTUM_INSPECTOR=1 so we can right-click ->
        // Inspect during smoke tests.
        let settings: webkit6::Settings =
            webkit6::prelude::WebViewExt::settings(&webview).unwrap_or_default();
        settings.set_enable_write_console_messages_to_stdout(true);
        settings
            .set_enable_developer_extras(std::env::var("QUANTUM_INSPECTOR").as_deref() == Ok("1"));
        settings.set_javascript_can_open_windows_automatically(false);
        webkit6::prelude::WebViewExt::set_settings(&webview, &settings);

        // Surface load failures so we can diagnose missing assets etc.
        webview.connect_load_failed(|_view, event, uri, err| {
            tracing::error!(
                "WebView load_failed: event={:?} uri={} err={}",
                event,
                uri,
                err
            );
            false
        });

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
            layer_shell,
        }
    }
}

impl crate::registry::WindowOps for LauncherWindow {
    /// Show the launcher window.
    fn show(&mut self) {
        if self.layer_shell {
            // Exclusive keyboard so the compositor routes all keystrokes to
            // the launcher while it is visible.
            self.window.set_keyboard_mode(KeyboardMode::Exclusive);
        }
        self.window.set_visible(true);
        // Focus the WebView so typing immediately reaches the search input.
        self.webview.grab_focus();
        self.visible = true;
    }

    /// Hide the launcher window.
    fn hide(&mut self) {
        if self.layer_shell {
            // Revert to on-demand keyboard mode before hiding so the next
            // compositor focus isn't redirected to a hidden surface.
            self.window.set_keyboard_mode(KeyboardMode::OnDemand);
        }
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
