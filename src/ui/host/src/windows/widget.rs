//! Widget window - background-layer window for clock and other widgets.

use crate::bridge::json_to_js_expression;
use crate::dispatcher::IpcDispatcher;
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use quantum_domain::ports::ThemeStore;
use quantum_domain::{EventEnvelope, ViewAnchor};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use webkit6::{prelude::*, WebView};

/// Default bar height in CSS pixels. Matches the value in
/// `src/ui/themes/default/views/widgets/bar/src/styles.css`. The
/// exclusive-zone reserved for window layout always equals this; the
/// surface itself may grow taller when a popover is open so its
/// children have room to render without being clipped by Wayland.
pub const BAR_HEIGHT: i32 = 32;

/// Wayland connector name for a `gdk::Monitor`. Hyprland uses the
/// same name (both come from Wayland's `wl_output` protocol).
/// Returns `None` if the platform doesn't expose a connector (e.g.
/// virtual outputs, headless mode) — callers should skip such
/// monitors rather than spawn untracked bars.
pub fn monitor_name(monitor: &gdk::Monitor) -> Option<String> {
    monitor.connector().map(|s| s.to_string())
}

/// A widget window - positioned on-screen as a background-layer widget.
pub struct WidgetWindow {
    window: gtk4::ApplicationWindow,
    #[allow(dead_code)]
    webview: WebView,
    /// Whether this widget anchors as a top bar (Layer::Top, anchored
    /// top/left/right, reserving an exclusive zone) and may grow at runtime
    /// via `set_height`. Derived from `anchor == ViewAnchor::Top`. Non-anchored
    /// widgets (clock, manifest-less plugins) sit on the background layer and
    /// ignore runtime resize requests.
    top_anchored: bool,
}

impl WidgetWindow {
    /// Create a new widget window.
    ///
    /// `anchor` selects the layout: [`ViewAnchor::Top`] produces a bar-style
    /// top-anchored surface with an exclusive zone of `height` (defaulting to
    /// [`BAR_HEIGHT`] when `None`); [`ViewAnchor::None`] (and `Bottom`, which
    /// widgets do not yet special-case) produces a background-layer, top-right
    /// widget like the clock.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        app: &gtk4::Application,
        view_name: String,
        anchor: ViewAnchor,
        height: Option<u32>,
        dispatcher: Arc<dyn IpcDispatcher>,
        _theme_store: Arc<dyn ThemeStore>,
        runtime: Handle,
        event_tx: broadcast::Sender<EventEnvelope>,
        monitor: Option<gdk::Monitor>,
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

        // Pin the layer-shell surface to a specific monitor before any
        // anchor/exclusive-zone configuration so the compositor places
        // it correctly the first time. Without this the bar lands on
        // whichever output the compositor picks (usually the focused
        // one) regardless of the `widgets/bar@<connector>` suffix.
        if let Some(m) = monitor.as_ref() {
            window.set_monitor(m);
        }

        // Layout is driven by the descriptor's anchor: a top-anchored widget
        // is a bar; everything else sits on the background layer. The exclusive
        // zone / initial height come from the descriptor's `height` (falling
        // back to BAR_HEIGHT).
        let top_anchored = anchor == ViewAnchor::Top;
        let bar_height = height.map(|h| h as i32).unwrap_or(BAR_HEIGHT);

        if top_anchored {
            // Bar widget: top layer, full width.
            //
            // Surface starts the same height as the visible bar
            // (bar_height) so the unused area below cannot intercept
            // input. Apps cover that area normally and remain clickable.
            // When the frontend opens a popover it calls the
            // `view.set_height` IPC method to grow the surface; the
            // exclusive zone stays at bar_height so other windows do
            // not reflow.
            window.set_layer(Layer::Top);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
            window.set_keyboard_mode(KeyboardMode::None);
            window.set_exclusive_zone(bar_height);
            window.set_default_height(bar_height);
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

        // If this window is pinned to a specific monitor, expose that
        // monitor's Wayland connector name to the Svelte view as
        // `window.__quantum_monitor` before any page script runs. The
        // ActiveWindow widget reads it to filter Hyprland events to
        // the bar's own output. Inject on LoadEvent::Committed: at that
        // point the document object exists but no page script has run.
        if let Some(m) = monitor.as_ref() {
            if let Some(name) = monitor_name(m) {
                // Quote the name as a valid JS string literal. The
                // Wayland connector names we see in practice ("DP-1",
                // "eDP-1", "HDMI-A-1") don't need escaping, but going
                // through serde_json::to_string covers
                // backslashes/quotes/control characters defensively.
                let quoted = serde_json::to_string(&name).unwrap_or_else(|_| "null".into());
                let js = format!("window.__quantum_monitor = {};", quoted);
                let webview_for_handler = webview.clone();
                webview.connect_load_changed(move |_view, event| {
                    if event == webkit6::LoadEvent::Committed {
                        webkit6::prelude::WebViewExt::evaluate_javascript(
                            &webview_for_handler,
                            &js,
                            None,
                            None,
                            gtk4::gio::Cancellable::NONE,
                            |_| {},
                        );
                    }
                });
            }
        }

        // Resolve the URL: plugin views load `quantum://plugin/...`, theme
        // views load `quantum://theme/...` (see `resolve_view_uri`).
        let uri = crate::windows::resolve_view_uri(&view_name);
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
                        // `env.payload` is `Box<RawValue>` carrying raw JSON
                        // text. `.get()` returns that text without any
                        // re-serialization, which is exactly what we need to
                        // inline into the `window.__quantum_notify` call.
                        let payload = env.payload.get();
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
            // Coalesce per main-loop turn: when the first item arrives, drain
            // any others already queued and submit a single
            // `evaluate_javascript` call. Each item is a self-contained
            // `if (...) { window.__quantum_notify(...); }` statement, so
            // joining with `;` is syntactically safe. This cuts the JIT
            // entry rate by the burst size when providers fan out (mpris
            // position ticks, workspace events, audio level changes).
            while let Some(first) = js_rx.recv().await {
                let mut batch = first;
                while let Ok(more) = js_rx.try_recv() {
                    batch.push(';');
                    batch.push_str(&more);
                }
                webview_for_notify.evaluate_javascript(
                    &batch,
                    None,
                    None,
                    None::<&gio::Cancellable>,
                    |_| {},
                );
            }
        });

        window.set_visible(true);

        Self {
            window,
            webview,
            top_anchored,
        }
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

    fn set_height(&mut self, height: u32) {
        // Only top-anchored (bar) widgets are meant to resize at runtime.
        // Other widgets ignore the request to avoid accidental geometry
        // changes from misuse of the IPC method.
        if !self.top_anchored {
            tracing::debug!("set_height ignored for non-bar widget");
            return;
        }
        let h = height.max(BAR_HEIGHT as u32) as i32;
        self.window.set_default_height(h);
        // Force a re-layout so the layer-shell surface picks up the new
        // height immediately; without this the change waits until the
        // next GTK redraw cycle, which can lag visibly.
        gtk4::prelude::WidgetExt::queue_resize(&self.window);
    }
}
