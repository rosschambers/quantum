//! Widget window - background-layer window for clock and other widgets.

use crate::bridge::json_to_js_expression;
use crate::dispatcher::IpcDispatcher;
use gtk4::gdk;
use gtk4::gio;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use quantum_domain::ports::ThemeStore;
use quantum_domain::EventEnvelope;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::broadcast;
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
        dispatcher: Arc<dyn IpcDispatcher>,
        _theme_store: Arc<dyn ThemeStore>,
        runtime: Handle,
        event_tx: broadcast::Sender<EventEnvelope>,
    ) -> Self {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .decorated(false)
            .build();

        // Make the GTK ApplicationWindow background transparent so the
        // layer-shell surface's overflow region passes through. Without
        // this, GTK paints its theme's default opaque background
        // (typically white) behind the WebView regardless of the
        // WebView's own background color. CSS applies via a window-
        // scoped provider so we don't pollute the global GTK style.
        let css = gtk4::CssProvider::new();
        css.load_from_string("window.background, window { background: transparent; }");
        gtk4::style_context_add_provider_for_display(
            &gtk4::prelude::WidgetExt::display(&window),
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        // Initialize layer shell
        window.init_layer_shell();

        // Determine layout based on view name.
        // TODO: Long-term, this should be per-view config in theme.toml.
        let is_bar = view_name == "widgets/bar" || view_name.starts_with("widgets/bar/");

        if is_bar {
            // Bar widget: top layer, full width with exclusive zone.
            //
            // The surface is intentionally taller than the visible bar
            // (300px vs the 32px exclusive zone). Popovers like the
            // PowerMenuIndicator render below the bar row and would be
            // clipped by a 32px surface. The extra 268px is transparent
            // overflow space; only the top 32px is opaque, and the
            // exclusive zone tells Hyprland to reserve only those 32px
            // for window layout.
            window.set_layer(Layer::Top);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
            window.set_keyboard_mode(KeyboardMode::None);
            window.set_exclusive_zone(32);
            window.set_default_height(300);
        } else {
            // Other widgets (clock, etc.): background layer, top-right.
            window.set_layer(Layer::Background);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Right, true);
            window.set_keyboard_mode(KeyboardMode::None);
            window.set_exclusive_zone(0);
        }

        window.set_namespace(&format!("quantum-widget-{}", view_name.replace('/', "-")));

        // Create and embed WebView
        let webview = webkit6::WebView::new();

        // Transparent WebView background so the layer-shell surface's
        // overflow region stays see-through. Without this WebKit paints
        // opaque white over the entire surface, defeating the purpose of
        // sizing the bar surface larger than its visible chrome.
        let transparent = gdk::RGBA::new(0.0, 0.0, 0.0, 0.0);
        webkit6::prelude::WebViewExt::set_background_color(&webview, &transparent);

        // Pipe JS console + enable developer inspector (gated by env var).
        let settings: webkit6::Settings =
            webkit6::prelude::WebViewExt::settings(&webview).unwrap_or_default();
        settings.set_enable_write_console_messages_to_stdout(true);
        settings
            .set_enable_developer_extras(std::env::var("QUANTUM_INSPECTOR").as_deref() == Ok("1"));
        webkit6::prelude::WebViewExt::set_settings(&webview, &settings);

        webview.connect_load_failed(|_view, event, uri, err| {
            tracing::error!(
                "widget WebView load_failed: event={:?} uri={} err={}",
                event,
                uri,
                err
            );
            false
        });

        // The scheme handler resolves paths against `views/<view_name>/...`
        // so the URL must include the `views/` segment. ThemeStore's
        // candidate_paths helper then maps to the Vite `dist/` subdir.
        let uri = format!("quantum://theme/default/views/{}/index.html", view_name);
        webview.load_uri(&uri);
        window.set_child(Some(&webview));

        // Register the bridge to wire JS messages to the dispatcher.
        crate::bridge::register_bridge(&webview, dispatcher, runtime.clone());

        // Subscribe to broadcast events and forward them to the WebView as
        // `window.__quantum_notify(channel, payload)` calls.
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
                        // client has installed `window.__quantum_notify`. This
                        // happens at daemon startup because providers begin
                        // publishing immediately while the WebView is still
                        // loading its bundle.
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
                            "widget notify subscription lagged: {skipped} events dropped"
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
