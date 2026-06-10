//! Panel window with gtk4-layer-shell anchoring.

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

/// Pick the GDK monitor that fullscreen-overlay panels should anchor
/// to. Returns the first monitor in `gdk::Display::monitors()`, which
/// is the user's primary display under Wayland. Returning `None`
/// lets the compositor pick (current behaviour).
fn pick_overlay_monitor() -> Option<gdk::Monitor> {
    let display = gdk::Display::default()?;
    let monitors = display.monitors();
    let n = monitors.n_items();
    if n == 0 {
        return None;
    }
    monitors
        .item(0)
        .and_then(|obj| obj.downcast::<gdk::Monitor>().ok())
}

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
    /// True for the power-menu (and any future overlay) — `show`/`hide`
    /// skip the dynamic OnDemand <-> Exclusive keyboard toggle in that
    /// case, because the overlay's keyboard mode was set Exclusive at
    /// construction and must stay Exclusive so Escape and click-outside
    /// reach the page.
    fullscreen_overlay: bool,
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
        view_name: impl Into<String>,
        dispatcher: Arc<dyn IpcDispatcher>,
        _theme_store: Arc<dyn ThemeStore>,
        runtime: Handle,
        event_tx: broadcast::Sender<EventEnvelope>,
        monitor: Option<gdk::Monitor>,
    ) -> Self {
        let view_name: String = view_name.into();
        let layer_shell = use_layer_shell();

        // The power-menu is a fullscreen overlay: it anchors all four
        // edges of the active output so the page's own backdrop covers
        // the whole screen, click-outside dismissal reaches the modal
        // card, and the surrounding "bars above and below the card"
        // (artefacts of a fixed-size centered surface) disappear. Other
        // panels (launcher today) stay as fixed-size centered surfaces.
        let is_fullscreen_overlay = matches!(
            view_name.as_str(),
            "widgets/power-menu" | "widgets/power-profile-menu"
        );

        let (width, height) = match view_name.as_str() {
            "launcher" => (600, 420),
            "widgets/power-menu" => (440, 320),
            "widgets/power-profile-menu" => (440, 320),
            _ => (480, 320),
        };

        let mut builder = gtk4::ApplicationWindow::builder().application(app);

        if is_fullscreen_overlay {
            // gtk4-layer-shell sizes the surface from the GTK widget's
            // preferred size, NOT from the anchors alone. With an empty
            // WebView the preferred size is 0x0 and the compositor falls
            // back to a tiny placeholder (~200x200). Set generous
            // defaults so the surface spans even very large outputs;
            // layer-shell clamps to the output rectangle when all four
            // edges are anchored.
            builder = builder
                .default_width(8192)
                .default_height(8192)
                .resizable(false);
        } else {
            builder = builder
                .default_width(width)
                .default_height(height)
                .resizable(!layer_shell);
        }

        // In layer-shell mode we want a chromeless surface. In windowed (test)
        // mode we keep decorations so the user has a close button and can
        // drag the window.
        if layer_shell {
            builder = builder.decorated(false);
        } else {
            builder = builder.decorated(true).title("Quantum");
        }

        let window = builder.build();

        // For the fullscreen overlay the GTK window itself must paint
        // transparent so the regions outside the modal card don't show
        // GTK's default opaque background. Matches the widget.rs
        // pattern (see widget.rs:57-69). The WebView's own background
        // is set to transparent further down for the same reason.
        if is_fullscreen_overlay {
            let css = gtk4::CssProvider::new();
            css.load_from_string("window.background, window { background: transparent; }");
            gtk4::style_context_add_provider_for_display(
                &gtk4::prelude::WidgetExt::display(&window),
                &css,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let namespace = format!("quantum-panel-{}", view_name.replace('/', "-"));
        if layer_shell {
            window.init_layer_shell();
            window.set_layer(Layer::Top);
            window.set_namespace(&namespace);
            window.set_exclusive_zone(-1); // Don't reserve space.

            if is_fullscreen_overlay {
                // Pin to the monitor supplied by the caller (typically
                // the bar widget's `@<monitor>` suffix on the view key)
                // so the overlay opens on the same display as the bar
                // that triggered it. Fall back to the primary monitor
                // when no monitor is supplied (e.g. direct quantumctl
                // calls without a suffix).
                if let Some(m) = monitor.as_ref().cloned().or_else(pick_overlay_monitor) {
                    window.set_monitor(&m);
                }
                // Anchor all four edges so the surface spans the whole
                // output; the page renders a centered card on a dark
                // backdrop that fills the entire surface. Keyboard goes
                // Exclusive so Escape and click-outside reach the JS.
                window.set_anchor(Edge::Top, true);
                window.set_anchor(Edge::Bottom, true);
                window.set_anchor(Edge::Left, true);
                window.set_anchor(Edge::Right, true);
                window.set_keyboard_mode(KeyboardMode::Exclusive);
            } else {
                // Default centered panel: no anchors, let the
                // compositor center it on the focused output.
                window.set_keyboard_mode(KeyboardMode::OnDemand);
            }
        }
        // In windowed mode we let the compositor place it like any other
        // xdg-toplevel — Hyprland's default behavior is fine for testing.

        // Create and embed WebView
        let webview = webkit6::WebView::new();

        // For the fullscreen overlay the WebView must paint transparent
        // too, so the page's `.backdrop` controls the dim, not WebKit's
        // own default white. Without this you see white bands above and
        // below the modal card.
        if is_fullscreen_overlay {
            let transparent = gdk::RGBA::new(0.0, 0.0, 0.0, 0.0);
            webkit6::prelude::WebViewExt::set_background_color(&webview, &transparent);
        }

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

        let url = format!("quantum://theme/default/views/{}/index.html", view_name);
        webview.load_uri(&url);
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
                        // `env.payload` is `Box<RawValue>` carrying raw JSON
                        // text. `.get()` returns that text without any
                        // re-serialization, which is exactly what we need to
                        // inline into the `window.__quantum_notify` call.
                        let payload = env.payload.get();
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

        Self {
            window,
            webview,
            visible: false,
            layer_shell,
            fullscreen_overlay: is_fullscreen_overlay,
        }
    }
}

impl crate::registry::WindowOps for PanelWindow {
    /// Show the panel window.
    fn show(&mut self) {
        if self.layer_shell && !self.fullscreen_overlay {
            // Exclusive keyboard so the compositor routes all keystrokes to
            // the panel while it is visible. The fullscreen overlay was
            // already set Exclusive at construction and must stay that
            // way across hide/show cycles.
            self.window.set_keyboard_mode(KeyboardMode::Exclusive);
        }
        self.window.set_visible(true);
        // Focus the WebView so typing immediately reaches the input.
        self.webview.grab_focus();
        self.visible = true;
    }

    /// Hide the panel window.
    fn hide(&mut self) {
        if self.layer_shell && !self.fullscreen_overlay {
            // Revert to on-demand keyboard mode before hiding so the next
            // compositor focus isn't redirected to a hidden surface. The
            // fullscreen overlay keeps Exclusive — when hidden the
            // surface releases keyboard focus naturally.
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
