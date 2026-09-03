//! Widget window - background-layer window for clock and other widgets.

use crate::bridge::json_to_js_expression;
use crate::dispatcher::IpcDispatcher;
use crate::windows::WindowContext;
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use quantum_domain::{
    ports::ThemeStore, EventEnvelope, ViewAnchor, ViewPosition, WindowInputRegion,
};
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
    window: gtk4::Window,
    webview: WebView,
    /// True when this widget's WebView owns its render process. Widgets share
    /// the process today (both constructors use `new_webview(true)`), so this
    /// is always false and teardown never terminates — terminating a shared
    /// (`related-view`) process would kill it for every warm sibling. The flag
    /// exists so a future isolated widget reclaims its process without a code
    /// change, and keeps the destroy/Drop paths identical to PanelWindow.
    owns_render_process: bool,
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
    /// Set once destroy() (or Drop) has torn the GTK window down, so the
    /// second teardown is skipped. Calling gtk_window_destroy after the
    /// surface is gone aborts with the gdk_surface_get_display assertion.
    is_destroyed: bool,
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
    ///
    /// `click_through` only applies to the `fill_output` path. When true, the
    /// fill-output surface is hoisted to [`Layer::Overlay`] (above application
    /// windows) with an empty pointer input region, so it renders on top yet
    /// passes every pointer event straight through to whatever is beneath it;
    /// keyboard input is disabled ([`KeyboardMode::None`]). When false, the
    /// fill-output surface keeps its interactive [`Layer::Bottom`] placement.
    /// `click_through` has no effect on the bar or background-layer paths.
    pub(crate) fn new(
        ctx: WindowContext,
        view_name: String,
        anchor: ViewAnchor,
        position: ViewPosition,
        fill_output: bool,
        click_through: bool,
        height: Option<u32>,
    ) -> Self {
        // The theme store backs the quantum:// scheme handler registered on
        // the GTK default context, and is also cloned into the event
        // subscriber below so a `theme.reloaded` event can push freshly
        // resolved tokens into this already-open WebView.
        let WindowContext {
            dispatcher,
            theme_store,
            runtime,
            event_tx,
            monitor,
            web_context,
        } = ctx;
        let window = gtk4::Window::builder().decorated(false).build();

        // Make the GTK window background transparent so the
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
            // The surface anchors the top, left, and right edges (NOT bottom)
            // and is sized to the full monitor height via `set_default_height`.
            // Anchoring a single horizontal edge keeps the exclusive zone
            // working: `bar_height` is reserved at the top so windows tile
            // below the visible strip. Anchoring BOTH top and bottom makes the
            // compositor ignore the exclusive zone (the surface spans the
            // screen), which leaves windows rendering behind the bar.
            //
            // The visible 32-pixel row comes from the view's
            // `.bar { height: var(--bar-height) }` CSS over the transparent
            // body, not from the surface size — so opening a dropdown in the
            // tall transparent area below the strip never resizes the surface
            // (no flicker).
            //
            // Because a full-height Top surface would otherwise capture every
            // click on the monitor, the pointer input region is clipped to
            // the visible strip on map (see below) and expanded to include an
            // open menu via `set_input_region`.
            let monitor_height = monitor
                .as_ref()
                .map(|m| m.geometry().height())
                .filter(|h| *h > 0)
                .unwrap_or(2160);
            window.set_layer(Layer::Top);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
            window.set_default_height(monitor_height);
            // OnDemand (not None) so an interactive dropdown rendered inside
            // the bar surface can take keyboard focus while open, without
            // locking the user out the way Exclusive would.
            window.set_keyboard_mode(KeyboardMode::OnDemand);
            window.set_exclusive_zone(bar_height);

            // Clip the input region to the visible strip on `realize`, which
            // fires once the GdkSurface exists but before it is presented, so
            // the full-height surface never has the default (full) input
            // region for even one frame. Re-apply on surface width changes
            // (monitor geometry / hotplug) so the strip always spans the full
            // bar width, preserving any open-menu rectangle.
            //
            // Both closures capture `window` WEAK (only the `Rc` cell is
            // strong): the realize handler is owned by the window and the
            // width handler by its surface, so a strong capture would form a
            // `window -> surface -> handler -> window` cycle and leak the bar
            // (and its WebView) when the registry drops it on monitor
            // disconnect. Connecting on `realize` rather than `map` also
            // avoids re-subscribing the width handler on every hide/show.
            window.connect_realize(glib::clone!(
                #[weak]
                window,
                #[strong]
                input_region,
                move |win| {
                    apply_input_region(win, bar_height, input_region.get());
                    if let Some(surface) = gtk4::prelude::NativeExt::surface(win) {
                        surface.connect_width_notify(glib::clone!(
                            #[weak]
                            window,
                            #[strong]
                            input_region,
                            move |_| apply_input_region(&window, bar_height, input_region.get())
                        ));
                    }
                }
            ));
        } else if fill_output {
            // Fill-output widgets (the timers scatter surface): anchored on all
            // four edges so the transparent surface covers the whole monitor.
            // The Svelte view positions its content absolutely within this
            // full-screen area. `position` is ignored.
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Bottom, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);

            if click_through {
                // Click-through fill-output surface (a passive overlay host):
                // the Overlay layer paints it ABOVE application windows, and an
                // empty pointer input region (installed on realize below) means
                // it captures no clicks — every pointer event passes straight
                // through to whatever is underneath. Keyboard input is disabled
                // because a passthrough overlay is display-only and must never
                // steal focus.
                window.set_layer(Layer::Overlay);
                window.set_keyboard_mode(KeyboardMode::None);
                // Exclusive zone -1 (not 0): span the ENTIRE output, including
                // the strip another surface's exclusive zone reserves (the top
                // bar). With 0 the compositor shrinks this surface to the usable
                // area below the bar, so the webview origin sits bar-height below
                // the true screen top and the cursor ripples — placed from
                // compositor-global `cursorpos` — render that many pixels too
                // low. -1 keeps the surface full-output so viewport (0,0) maps to
                // the monitor's real top-left.
                window.set_exclusive_zone(-1);

                // Install the empty input region on `realize`, when the
                // GdkSurface first exists but before it is presented, so the
                // full-screen surface never captures a pointer event for even
                // one frame; re-apply on surface-width changes (monitor
                // geometry / hotplug) so a compositor-side reset cannot restore
                // the default (full) input region. This mirrors the bar's
                // input-region timing. `window` is captured WEAK in both
                // closures (the realize handler is owned by the window and the
                // width handler by its surface) so a strong capture cannot form
                // a `window -> surface -> handler -> window` reference cycle.
                window.connect_realize(glib::clone!(
                    #[weak]
                    window,
                    move |win| {
                        apply_empty_input_region(win);
                        if let Some(surface) = gtk4::prelude::NativeExt::surface(win) {
                            surface.connect_width_notify(glib::clone!(
                                #[weak]
                                window,
                                move |_| apply_empty_input_region(&window)
                            ));
                        }
                    }
                ));
            } else {
                // Interactive fill-output surface (the timers scatter): the
                // Bottom layer (not Background) still sits behind normal
                // application windows, but wlroots routes pointer and keyboard
                // input to Bottom-layer surfaces, whereas Background-layer
                // surfaces are treated as non-interactive wallpaper and do not
                // receive button clicks. The interactive per-timer controls
                // (edit / dismiss) and the edit form need that input.
                //
                // On-demand keyboard so interactive content on the fill-output
                // surface (the per-timer edit form's text fields) can receive
                // keystrokes when focused. On-demand only grabs the keyboard
                // while a focusable element is focused and releases it
                // otherwise, so it does not lock the user out the way Exclusive
                // would.
                window.set_exclusive_zone(0);
                window.set_layer(Layer::Bottom);
                window.set_keyboard_mode(KeyboardMode::OnDemand);
            }
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
            &web_context,
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
            is_destroyed: false,
            // Widgets share the render process (new_webview(true) below).
            owns_render_process: false,
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
    pub(crate) fn new_toast(ctx: WindowContext, view_name: String, position: ViewPosition) -> Self {
        let WindowContext {
            dispatcher,
            theme_store,
            runtime,
            event_tx,
            monitor,
            web_context,
        } = ctx;
        let window = gtk4::Window::builder().decorated(false).build();

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
            &web_context,
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
            is_destroyed: false,
            // Toasts share the render process (new_webview(true) below).
            owns_render_process: false,
        }
    }
}

/// Create the WebView for a widget-style window, configure it, wire the
/// IPC bridge, embed it in `window`, and start forwarding broadcast events
/// to `window.__quantum_notify`. Shared by [`WidgetWindow::new`] and
/// [`WidgetWindow::new_toast`].
#[allow(clippy::too_many_arguments)]
fn build_webview(
    window: &gtk4::Window,
    view_name: &str,
    web_context: &webkit6::WebContext,
    dispatcher: Arc<dyn IpcDispatcher>,
    theme_store: Arc<dyn ThemeStore>,
    runtime: Handle,
    event_tx: broadcast::Sender<EventEnvelope>,
    monitor: Option<&gdk::Monitor>,
) -> WebView {
    // Widget-style views (bar, clock, timers, toast) are always-warm and never
    // destroyed on dismiss, so they share a single render process via the
    // shared web-process anchor to cut per-view renderer memory.
    let webview = crate::web_process::new_webview(web_context, true);

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
    crate::web_process::apply_widget_settings(&settings);
    webkit6::prelude::WebViewExt::set_settings(&webview, &settings);

    // Replace WebKit's browser right-click menu (back/forward/reload) with
    // nothing; views provide their own DOM contextmenu where useful. Kept only
    // under the inspector flag so "Inspect Element" is available in dev.
    crate::windows::suppress_browser_context_menu(&webview, inspector_enabled);

    // Block external navigations and open them in the default browser.
    crate::windows::install_navigation_policy(&webview);

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
    crate::windows::install_web_process_crash_handler(&webview, view_name);
    window.set_child(Some(&webview));

    // Per-webview subscription set, shared between the bridge (which seeds and
    // updates it from `bridge.subscribe` / `bridge.unsubscribe` messages) and
    // the broadcast forwarder below (which filters on it). `None` until the
    // client seeds its first subscription: forward all.
    let subs: crate::subscriptions::WebviewSubscriptions =
        std::sync::Arc::new(std::sync::Mutex::new(None));

    // Register the bridge to wire JS messages to the dispatcher.
    crate::bridge::register_bridge(&webview, dispatcher, runtime.clone(), subs.clone());

    // Subscribe to broadcast events and forward them to the WebView as
    // `window.__quantum_notify(channel, payload)` calls.
    let (js_tx, mut js_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mut event_rx = event_tx.subscribe();
    let theme_store_for_notify = theme_store.clone();
    let subs_for_notify = subs.clone();
    runtime.spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(env) => {
                    // Filter server-side: skip channels this webview never
                    // subscribed to (once seeded). The lock is held only for
                    // the pure decision, never across an await. A poisoned lock
                    // fails open so a panic elsewhere can never silently freeze
                    // this webview's live updates.
                    let forward = match subs_for_notify.lock() {
                        Ok(guard) => crate::subscriptions::should_forward(&env.channel, &guard),
                        Err(_) => true,
                    };
                    if !forward {
                        continue;
                    }
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

    fn destroy(&mut self) {
        if self.is_destroyed {
            return;
        }
        self.is_destroyed = true;
        // Terminate the render process before destroying the window ONLY when
        // this widget owns its process: destroying the GTK window alone leaves
        // an isolated WebKitWebProcess resident (see the note in panel.rs), but
        // terminating a shared (`related-view`) process would kill it for every
        // warm sibling. Widgets share today (owns_render_process is false), so
        // this never fires; the guard keeps the path safe if a future widget is
        // isolated.
        if self.owns_render_process {
            webkit6::prelude::WebViewExt::terminate_web_process(&self.webview);
        }
        gtk4::prelude::GtkWindowExt::destroy(&self.window);
    }

    fn set_height(&mut self, height: u32) {
        // The bar is now full-height (anchored on all four edges), so
        // `set_default_height` is geometrically inert for it: the surface
        // already spans the monitor and the visible row is CSS-driven. This
        // method is retained for other resizable callers and the toast
        // sizing TODO; it stays guarded to top-anchored widgets so a stray
        // IPC request cannot resize a background widget. Behavior is
        // unchanged.
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
        // Remember the extra rectangle so the realize / surface-width
        // handlers re-apply strip ∪ menu rather than dropping the menu on a
        // width change, then apply it now.
        self.input_region.set(region);
        apply_input_region(&self.window, self.bar_height, region);
    }

    fn inject_view_args(&mut self, args: Option<serde_json::Value>) {
        crate::windows::inject_view_args(&self.webview, args);
    }
}

impl Drop for WidgetWindow {
    /// Safety net: if the handle is dropped without an explicit destroy, tear
    /// the surface out here. Guarded so this never double-calls
    /// gtk_window_destroy after an explicit destroy(), which would abort on an
    /// already-freed layer-shell surface.
    fn drop(&mut self) {
        if !self.is_destroyed {
            // Symmetric with destroy(): terminate only a process this widget
            // owns, never a shared (`related-view`) one warm siblings depend on.
            if self.owns_render_process {
                webkit6::prelude::WebViewExt::terminate_web_process(&self.webview);
            }
            gtk4::prelude::GtkWindowExt::destroy(&self.window);
        }
    }
}

/// Compute the pointer input-region rectangles, in surface-local pixels, for
/// a bar surface of the given width and strip height: always the visible
/// strip `(0, 0, width, bar_height)`, plus the `extra` rectangle (an open
/// dropdown menu) when present. `width` is clamped to at least 1 so a
/// zero-width surface still yields a valid strip. Pure function so the
/// geometry can be unit-tested without GTK; [`apply_input_region`] is a thin
/// shell that turns these tuples into a cairo region.
fn input_region_rects(
    width: i32,
    bar_height: i32,
    extra: Option<WindowInputRegion>,
) -> Vec<(i32, i32, i32, i32)> {
    let clamped_width = width.max(1);
    let mut rectangles = vec![(0, 0, clamped_width, bar_height)];
    if let Some(menu) = extra {
        rectangles.push((menu.x, menu.y, menu.width, menu.height));
    }
    rectangles
}

/// Clip the bar surface's pointer input region to the visible strip,
/// optionally unioned with an `extra` rectangle (an open dropdown menu). A
/// no-op if the `GdkSurface` does not yet exist (the caller re-runs once it
/// is realized).
///
/// Coordinates are surface-local pixels at scale 1. HiDPI surfaces report a
/// `scale_factor()` greater than one; if on-device testing shows the region
/// is mis-scaled, convert here at the single marked point. This is left at
/// scale 1 for now per the plan (HiDPI verification is a later manual step).
fn apply_input_region(window: &gtk4::Window, bar_height: i32, extra: Option<WindowInputRegion>) {
    let Some(surface) = gtk4::prelude::NativeExt::surface(window) else {
        return;
    };
    // Single scale-conversion point (currently scale 1; see doc comment).
    let rectangles: Vec<gtk4::cairo::RectangleInt> =
        input_region_rects(surface.width(), bar_height, extra)
            .into_iter()
            .map(|(x, y, width, height)| gtk4::cairo::RectangleInt::new(x, y, width, height))
            .collect();
    let region = gtk4::cairo::Region::create_rectangles(&rectangles);
    gtk4::prelude::SurfaceExt::set_input_region(&surface, &region);
}

/// Install an EMPTY pointer input region on `window`'s surface so it receives
/// no pointer events and every click passes straight through to whatever is
/// beneath it. Used by the click-through fill-output overlay path. A no-op if
/// the `GdkSurface` does not yet exist (the caller re-runs once it is
/// realized). A cairo region built with zero rectangles is empty, which
/// Wayland interprets as "this surface accepts no pointer input".
fn apply_empty_input_region(window: &gtk4::Window) {
    let Some(surface) = gtk4::prelude::NativeExt::surface(window) else {
        return;
    };
    let region = gtk4::cairo::Region::create();
    gtk4::prelude::SurfaceExt::set_input_region(&surface, &region);
}

#[cfg(test)]
mod input_region_geometry_tests {
    use super::*;

    #[test]
    fn strip_only_when_extra_is_none() {
        let rectangles = input_region_rects(300, 32, None);
        assert_eq!(rectangles, vec![(0, 0, 300, 32)]);
    }

    #[test]
    fn strip_plus_menu_when_extra_present() {
        let menu = WindowInputRegion {
            x: 10,
            y: 32,
            width: 200,
            height: 150,
        };
        let rectangles = input_region_rects(300, 32, Some(menu));
        assert_eq!(rectangles, vec![(0, 0, 300, 32), (10, 32, 200, 150)]);
    }

    #[test]
    fn width_is_clamped_to_at_least_one() {
        assert_eq!(input_region_rects(0, 32, None), vec![(0, 0, 1, 32)]);
        assert_eq!(input_region_rects(-5, 32, None), vec![(0, 0, 1, 32)]);
    }
}
