//! Widget window - background-layer window for clock and other widgets.

use crate::bridge::json_to_js_expression;
use crate::dispatcher::IpcDispatcher;
use crate::windows::WindowContext;
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use quantum_domain::{ports::ThemeStore, EventEnvelope, ViewAnchor, ViewPosition, WindowInputRegion};
use std::cell::Cell;
use std::rc::Rc;
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
    /// Pixel height of the visible bar strip. The full-height bar surface
    /// clips its pointer input region to `(0, 0, surface_width, bar_height)`
    /// so the transparent area below the strip never captures clicks. Zero
    /// for non-bar widgets, which do not manage an input region.
    bar_height: i32,
    /// The current "extra" input rectangle (an open dropdown menu) unioned
    /// into the strip. Shared with the GTK `map` and surface `notify::width`
    /// handlers so a surface-width change re-applies strip ∪ menu rather than
    /// dropping the menu's clickable region. `None` means strip-only.
    input_region: Rc<Cell<Option<WindowInputRegion>>>,
}

impl WidgetWindow {
    /// Create a new widget window.
    ///
    /// `ctx` carries the shared host context (application, dispatcher, theme
    /// store, runtime, event sender, target monitor). `anchor` selects the
    /// layout: [`ViewAnchor::Top`] produces a bar-style top-anchored surface
    /// with an exclusive zone of `height` (defaulting to [`BAR_HEIGHT`] when
    /// `None`); [`ViewAnchor::None`] (and `Bottom`, which widgets do not yet
    /// special-case) produces a background-layer widget anchored per
    /// `position` (with [`ViewPosition::Center`] preserved as top-right for the
    /// clock). `position` is ignored on the top-anchored bar path.
    ///
    /// When `fill_output` is true on the background-layer path, the surface
    /// anchors all four edges so it fills the entire monitor (used by the
    /// timers widget for free placement and dragging); `position` is then
    /// ignored. `fill_output` has no effect on the top-anchored bar path.
    pub(crate) fn new(
        ctx: WindowContext<'_>,
        view_name: String,
        anchor: ViewAnchor,
        position: ViewPosition,
        fill_output: bool,
        height: Option<u32>,
    ) -> Self {
        // The theme store backs the quantum:// scheme handler registered on
        // the GTK default context, and is also cloned into the event
        // subscriber below so a `theme.reloaded` event can push freshly
        // resolved tokens into this already-open WebView.
        let WindowContext {
            app,
            dispatcher,
            theme_store,
            runtime,
            event_tx,
            monitor,
        } = ctx;
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
        // Shared current "extra" input rectangle (an open menu). Read by the
        // map / surface-width handlers and updated by `set_input_region`.
        let input_region: Rc<Cell<Option<WindowInputRegion>>> = Rc::new(Cell::new(None));

        if top_anchored {
            // Bar widget: top layer, full monitor height.
            //
            // The surface anchors all four edges so it spans the whole
            // monitor; the exclusive zone stays at `bar_height` so other
            // windows only avoid the visible strip and do not reflow when a
            // dropdown opens below it. The visible 32px row comes from the
            // view's `.bar { height: var(--bar-height) }` CSS over the
            // transparent body, not from the surface size — so opening a
            // menu no longer resizes the surface (no flicker).
            //
            // Because a full-height Top surface would otherwise capture every
            // click on the monitor, the pointer input region is clipped to
            // the visible strip on map (see below) and expanded to include an
            // open menu via `set_input_region`.
            window.set_layer(Layer::Top);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Bottom, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
            // OnDemand (not None) so an interactive dropdown rendered inside
            // the bar surface can take keyboard focus while open, without
            // locking the user out the way Exclusive would.
            window.set_keyboard_mode(KeyboardMode::OnDemand);
            window.set_exclusive_zone(bar_height);

            // Clip the input region to the visible strip as soon as the
            // GdkSurface exists. `connect_map` fires once the surface is
            // mapped; if the surface is somehow not yet available the helper
            // is a no-op and a later map fires again. Re-apply on surface
            // width changes (monitor geometry / hotplug) so the strip always
            // spans the full bar width, preserving any open-menu rectangle.
            let window_for_map = window.clone();
            let region_for_map = input_region.clone();
            window.connect_map(move |win| {
                apply_input_region(win, bar_height, region_for_map.get());
                if let Some(surface) = gtk4::prelude::NativeExt::surface(win) {
                    let window_for_width = window_for_map.clone();
                    let region_for_width = region_for_map.clone();
                    surface.connect_width_notify(move |_| {
                        apply_input_region(&window_for_width, bar_height, region_for_width.get());
                    });
                }
            });
        } else if fill_output {
            // Fill-output widgets (the timers scatter surface): anchored on all
            // four edges so the transparent surface covers the whole monitor.
            // The Svelte view positions its content absolutely within this
            // full-screen area. `position` is ignored.
            //
            // Uses the Bottom layer (not Background): it still sits behind
            // normal application windows, but wlroots routes pointer and
            // keyboard input to Bottom-layer surfaces, whereas Background-layer
            // surfaces are treated as non-interactive wallpaper and do not
            // receive button clicks. The interactive per-timer controls (edit /
            // dismiss) and the edit form need that input.
            window.set_layer(Layer::Bottom);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Bottom, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
            // On-demand keyboard so interactive content on the fill-output
            // surface (the per-timer edit form's text fields) can receive
            // keystrokes when focused. On-demand only grabs the keyboard while a
            // focusable element is focused and releases it otherwise, so it does
            // not lock the user out the way Exclusive would.
            window.set_keyboard_mode(KeyboardMode::OnDemand);
            window.set_exclusive_zone(0);
        } else {
            // Other widgets (clock, etc.): background layer, anchored
            // per the descriptor's `position`, mirroring `new_toast`.
            window.set_layer(Layer::Background);
            match position {
                ViewPosition::TopRight => {
                    window.set_anchor(Edge::Top, true);
                    window.set_anchor(Edge::Right, true);
                }
                ViewPosition::TopLeft => {
                    window.set_anchor(Edge::Top, true);
                    window.set_anchor(Edge::Left, true);
                }
                ViewPosition::TopCenter => {
                    // Anchor only to the top edge: with no left/right anchor the
                    // compositor centers the surface horizontally.
                    window.set_anchor(Edge::Top, true);
                }
                ViewPosition::Center => {
                    // Center maps to top-right for backward compatibility with
                    // existing background widgets (the clock is Center via the
                    // fallback path and must keep its historical top-right
                    // placement).
                    window.set_anchor(Edge::Top, true);
                    window.set_anchor(Edge::Right, true);
                }
            }
            window.set_keyboard_mode(KeyboardMode::None);
            window.set_exclusive_zone(0);
        }

        window.set_namespace(&format!("quantum-widget-{}", view_name.replace('/', "-")));

        // Create and embed the WebView, wire the bridge, and start
        // forwarding broadcast events. Shared with `new_toast`.
        let webview = build_webview(
            &window,
            &view_name,
            dispatcher,
            theme_store,
            runtime,
            event_tx,
            monitor.as_ref(),
        );

        window.set_visible(true);

        Self {
            window,
            webview,
            top_anchored,
            // Only the bar manages an input region; non-bar widgets store 0
            // and ignore `set_input_region`.
            bar_height: if top_anchored { bar_height } else { 0 },
            input_region,
        }
    }

    /// Create a toast window: a small, non-modal `Layer::Overlay` surface
    /// anchored per `position`, constructed hidden and shown on demand via
    /// `view.show`.
    ///
    /// Unlike [`WidgetWindow::new`], the toast never reserves an exclusive
    /// zone, takes no keyboard focus, and is not top-anchored (so runtime
    /// `set_height` requests are ignored). It reuses the same transparent
    /// background and WebView/bridge/event-forwarding setup.
    pub(crate) fn new_toast(
        ctx: WindowContext<'_>,
        view_name: String,
        position: ViewPosition,
    ) -> Self {
        let WindowContext {
            app,
            dispatcher,
            theme_store,
            runtime,
            event_tx,
            monitor,
        } = ctx;
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .decorated(false)
            .build();

        // Transparent background, mirroring `new`: a window-scoped CSS
        // provider so the layer-shell surface's overflow region passes
        // through instead of GTK's opaque theme background.
        let css = gtk4::CssProvider::new();
        css.load_from_string("window.background, window { background: transparent; }");
        gtk4::style_context_add_provider_for_display(
            &gtk4::prelude::WidgetExt::display(&window),
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        window.init_layer_shell();

        // Pin to the target monitor before anchor configuration so the
        // compositor places the surface on the right output the first time.
        if let Some(m) = monitor.as_ref() {
            window.set_monitor(m);
        }

        // Toasts live on the overlay layer, anchored per `position`, with no
        // exclusive zone and no keyboard focus.
        window.set_layer(Layer::Overlay);
        match position {
            ViewPosition::TopRight => {
                window.set_anchor(Edge::Top, true);
                window.set_anchor(Edge::Right, true);
            }
            ViewPosition::TopLeft => {
                window.set_anchor(Edge::Top, true);
                window.set_anchor(Edge::Left, true);
            }
            ViewPosition::TopCenter => {
                window.set_anchor(Edge::Top, true);
            }
            ViewPosition::Center => {
                window.set_anchor(Edge::Top, true);
            }
        }
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_exclusive_zone(0);

        // gtk4-layer-shell sizes the surface from the GTK widget's preferred
        // size, not from the anchors alone (see panel.rs:113-123 for the same
        // behavior on overlays). A two-edge-anchored Overlay with an empty
        // WebView has a preferred size of 0x0, so the compositor would map it
        // at a tiny placeholder size before the page lays out. Set a default
        // width matching the view's fixed 380px card stack (plus margin) so the
        // surface is never zero; height is left to follow the content.
        //
        // TODO: while shown, this fixed-width surface can capture pointer
        // events over its area. Implement input-region passthrough (so only the
        // opaque toast cards are clickable) as a future improvement; the plan
        // defers it. For now the window is hidden whenever it is empty, which
        // bounds the impact.
        window.set_default_width(400);

        window.set_namespace(&format!("quantum-toast-{}", view_name.replace('/', "-")));

        let webview = build_webview(
            &window,
            &view_name,
            dispatcher,
            theme_store,
            runtime,
            event_tx,
            monitor.as_ref(),
        );

        // Shown on demand via `view.show`; constructed hidden so a toast
        // surface never flashes before its content is requested.
        window.set_visible(false);

        Self {
            window,
            webview,
            // Toasts are never top-anchored: runtime `set_height` is ignored.
            top_anchored: false,
            // Toasts do not manage an input region.
            bar_height: 0,
            input_region: Rc::new(Cell::new(None)),
        }
    }
}

/// Create the WebView for a widget-style window, configure it, wire the
/// IPC bridge, embed it in `window`, and start forwarding broadcast events
/// to `window.__quantum_notify`. Shared by [`WidgetWindow::new`] and
/// [`WidgetWindow::new_toast`].
#[allow(clippy::too_many_arguments)]
fn build_webview(
    window: &gtk4::ApplicationWindow,
    view_name: &str,
    dispatcher: Arc<dyn IpcDispatcher>,
    theme_store: Arc<dyn ThemeStore>,
    runtime: Handle,
    event_tx: broadcast::Sender<EventEnvelope>,
    monitor: Option<&gdk::Monitor>,
) -> WebView {
    let webview = webkit6::WebView::new();

    // Transparent WebView background so the layer-shell surface's
    // overflow region stays see-through. Without this WebKit paints
    // opaque white over the entire surface, defeating the purpose of
    // sizing the bar surface larger than its visible chrome.
    let transparent = gdk::RGBA::new(0.0, 0.0, 0.0, 0.0);
    webkit6::prelude::WebViewExt::set_background_color(&webview, &transparent);

    // Pipe JS console + enable developer inspector (gated by env var).
    let inspector_enabled = std::env::var("QUANTUM_INSPECTOR").as_deref() == Ok("1");
    let settings: webkit6::Settings =
        webkit6::prelude::WebViewExt::settings(&webview).unwrap_or_default();
    settings.set_enable_write_console_messages_to_stdout(inspector_enabled);
    settings.set_enable_developer_extras(inspector_enabled);
    webkit6::prelude::WebViewExt::set_settings(&webview, &settings);

    // Replace WebKit's browser right-click menu (back/forward/reload) with
    // nothing; views provide their own DOM contextmenu where useful. Kept only
    // under the inspector flag so "Inspect Element" is available in dev.
    crate::windows::suppress_browser_context_menu(&webview, inspector_enabled);

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
    if let Some(m) = monitor {
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
    let uri = crate::windows::resolve_view_uri(view_name);
    webview.load_uri(&uri);
    window.set_child(Some(&webview));

    // Register the bridge to wire JS messages to the dispatcher.
    crate::bridge::register_bridge(&webview, dispatcher, runtime.clone());

    // Subscribe to broadcast events and forward them to the WebView as
    // `window.__quantum_notify(channel, payload)` calls.
    let (js_tx, mut js_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mut event_rx = event_tx.subscribe();
    let theme_store_for_notify = theme_store.clone();
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
                    // On a theme reload, also push the freshly resolved
                    // tokens into the live `#quantum-tokens` stylesheet so
                    // the page recolors without a reload. The window
                    // re-resolves from the theme store rather than trusting
                    // the event payload, so a theme switch (where
                    // `ThemeStore::reload` updates the active theme) and an
                    // in-place token edit (where the watcher only
                    // invalidates the cache) are handled identically.
                    if env.channel == "theme.reloaded" {
                        let push = crate::windows::theme_reload_push_js(&theme_store_for_notify);
                        if js_tx.send(push).is_err() {
                            break;
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("widget notify subscription lagged: {skipped} events dropped");
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

    webview
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

    fn set_input_region(&mut self, region: Option<WindowInputRegion>) {
        // Only the top-anchored bar manages an input region; other widgets
        // ignore the request (mirroring the `set_height` guard) to avoid
        // accidentally clipping a full-surface widget's input.
        if !self.top_anchored {
            tracing::debug!("set_input_region ignored for non-bar widget");
            return;
        }
        // Remember the extra rect so the map / surface-width handlers
        // re-apply strip ∪ menu rather than dropping the menu on a width
        // change, then apply it now.
        self.input_region.set(region);
        apply_input_region(&self.window, self.bar_height, region);
    }
}

/// Clip the bar surface's pointer input region to the visible strip
/// `(0, 0, surface_width, bar_height)`, optionally unioned with an `extra`
/// rectangle (an open dropdown menu). A no-op if the `GdkSurface` does not
/// yet exist (the caller re-runs once it is mapped).
///
/// Coordinates are surface-local pixels at scale 1. HiDPI surfaces report a
/// `scale_factor()` greater than one; if on-device testing shows the region
/// is mis-scaled, convert here at the single marked point. This is left at
/// scale 1 for now per the plan (HiDPI verification is a later manual step).
fn apply_input_region(
    window: &gtk4::ApplicationWindow,
    bar_height: i32,
    extra: Option<WindowInputRegion>,
) {
    let Some(surface) = gtk4::prelude::NativeExt::surface(window) else {
        return;
    };
    // Single scale-conversion point (currently scale 1; see doc comment).
    let width = surface.width().max(1);
    let region = gtk4::cairo::Region::create_rectangle(&gtk4::cairo::RectangleInt::new(
        0, 0, width, bar_height,
    ));
    if let Some(rect) = extra {
        let _ = region.union_rectangle(&gtk4::cairo::RectangleInt::new(
            rect.x,
            rect.y,
            rect.width,
            rect.height,
        ));
    }
    gtk4::prelude::SurfaceExt::set_input_region(&surface, &region);
}
