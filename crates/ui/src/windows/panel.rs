//! Panel window with gtk4-layer-shell anchoring.

use crate::bridge::json_to_js_expression;
use crate::dispatcher::IpcDispatcher;
use gtk4::gio;
use gtk4::prelude::*;
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use quantum_domain::ports::ThemeStore;
use quantum_domain::EventEnvelope;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use webkit6::{prelude::*, WebView};

/// The panel window - a top-layer panel window anchored on-demand.
///
/// The `dispatcher` and `theme_store` constructor arguments are consumed
/// during construction — `dispatcher` is handed to `register_bridge` and
/// `theme_store` is owned by the quantum:// scheme handler registered on
/// the GTK default context — so neither is stored on the struct.
pub struct PanelWindow {
    window: gtk4::ApplicationWindow,
    webview: WebView,
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

impl PanelWindow {
    /// Create a new panel window.
    pub fn new(
        app: &gtk4::Application,
        dispatcher: Arc<dyn IpcDispatcher>,
        _theme_store: Arc<dyn ThemeStore>,
        runtime: Handle,
        event_tx: broadcast::Sender<EventEnvelope>,
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
            window.set_namespace("quantum-panel");
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

        // Register the bridge to wire JS messages to the dispatcher. The
        // dispatcher Arc is consumed here; the bridge keeps its own clone
        // inside the script message handler closure.
        crate::bridge::register_bridge(&webview, dispatcher, runtime.clone());

        // Subscribe to broadcast events and forward them to the WebView as
        // `window.__quantum_notify(channel, payload)` calls.
        //
        // Threading: `webkit6::WebView` is not `Send`, so the broadcast
        // subscription runs on Tokio and forwards pre-serialized JS strings
        // through an mpsc channel to a `spawn_local` task on the GLib main
        // context that owns a clone of the WebView. The Tokio side never
        // touches GTK objects; the GLib side never blocks on broadcast recv.
        let (js_tx, mut js_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let mut event_rx = event_tx.subscribe();
        runtime.spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(env) => {
                        let channel =
                            serde_json::to_string(&env.channel).unwrap_or_else(|_| "\"\"".into());
                        let payload =
                            serde_json::to_string(&env.payload).unwrap_or_else(|_| "null".into());
                        // Guard against notifications arriving before the JS
                        // client has installed `window.__quantum_notify`.
                        let raw = format!(
                            "if (typeof window.__quantum_notify === 'function') {{ window.__quantum_notify({channel}, {payload}); }}"
                        );
                        let js = json_to_js_expression(&raw);
                        if js_tx.send(js).is_err() {
                            // GLib forwarder has gone away — webview dropped.
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            "launcher notify subscription lagged: {skipped} events dropped"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let webview_for_notify = webview.clone();
        glib::MainContext::default().spawn_local(async move {
            while let Some(js) = js_rx.recv().await {
                webview_for_notify.evaluate_javascript(
                    &js,
                    None,
                    None,
                    None::<&gio::Cancellable>,
                    |_| {},
                );
            }
        });

        Self {
            window,
            webview,
            visible: false,
            layer_shell,
        }
    }
}

impl crate::registry::WindowOps for PanelWindow {
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
