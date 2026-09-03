//! Panel window with gtk4-layer-shell anchoring.

use crate::bridge::json_to_js_expression;
use crate::windows::WindowContext;
use gtk4::gdk;
use gtk4::gio;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use webkit6::{prelude::*, WebView};

/// The panel window - a top-layer panel window anchored on-demand.
///
/// The `dispatcher` and `theme_store` constructor arguments are consumed
/// during construction — `dispatcher` is handed to `register_bridge` and
/// `theme_store` is owned by the quantum:// scheme handler registered on
/// the GTK default context — so neither is stored on the struct.
pub struct PanelWindow {
    window: gtk4::Window,
    webview: WebView,
    visible: bool,
    layer_shell: bool,
    /// True for the power-menu (and any future overlay) — `show`/`hide`
    /// skip the dynamic OnDemand <-> Exclusive keyboard toggle in that
    /// case, because the overlay's keyboard mode was set Exclusive at
    /// construction and must stay Exclusive so Escape and click-outside
    /// reach the page.
    fullscreen_overlay: bool,
    /// Set once destroy() (or Drop) has torn the GTK window down, so the
    /// second teardown is skipped. Calling gtk_window_destroy after the
    /// surface is gone aborts with the gdk_surface_get_display assertion.
    is_destroyed: bool,
    /// True when this window's WebView has its OWN render process (the
    /// inverse of `share_process`). Only then may teardown call
    /// `terminate_web_process`: terminating a view that shares the process via
    /// `related-view` kills the SHARED process for every warm view (bar, clock,
    /// timers, toast), blanking them until reload. Transient
    /// `destroy_on_dismiss` views own their process (since B1) and are the only
    /// ones torn down, so gating on this keeps termination safe on every path
    /// (dismiss, eviction, Drop).
    owns_render_process: bool,
}

/// Decide whether this panel surface uses layer-shell: an overlay always does
/// (it dims the whole screen and dismisses on Escape, so it cannot lock the
/// user out), and a plain panel never does (it is a normal xdg-toplevel the
/// user can close or switch away from). Pulled out as a pure function so the
/// decision can be unit-tested without constructing a GTK window.
fn should_use_layer_shell(is_overlay: bool) -> bool {
    is_overlay
}

/// Derive a human-readable window title from a canonical view name. The last
/// path segment is taken, hyphens become spaces, and each word is capitalized:
/// `plugin/files/files` becomes `Files`, `plugin/power-menu/power-menu` becomes
/// `Power Menu`. An empty name falls back to `Quantum`.
fn panel_title(canonical_name: &str) -> String {
    let segment = canonical_name.rsplit('/').next().unwrap_or("");
    if segment.is_empty() {
        return "Quantum".to_string();
    }
    segment
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl PanelWindow {
    /// Create a new panel window.
    ///
    /// `ctx` carries the shared host context (application, dispatcher, theme
    /// store, runtime, event sender, target monitor). `canonical_name` is the
    /// `plugin/<plugin>/<view>` (or theme) name used to build the load URL and
    /// the layer-shell namespace. `overlay` selects fullscreen-overlay
    /// treatment (all four edges anchored, exclusive keyboard, transparent
    /// surface) versus a fixed-size centered panel; `width`/`height` size the
    /// centered panel and are ignored for overlays, which span the whole
    /// output.
    pub(crate) fn new(
        ctx: WindowContext,
        canonical_name: impl Into<String>,
        overlay: bool,
        width: i32,
        height: i32,
        share_process: bool,
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
        let view_name: String = canonical_name.into();

        // An overlay anchors all four edges of the active output so the
        // page's own backdrop covers the whole screen, click-outside
        // dismissal reaches the modal card, and the surrounding "bars above
        // and below the card" (artefacts of a fixed-size centered surface)
        // disappear. Plain panels stay as fixed-size centered surfaces.
        let is_fullscreen_overlay = overlay;

        // Panels are normal xdg-toplevels; only overlays use layer-shell.
        // The struct's `layer_shell` field below is set from this value so
        // `show`/`hide` keyboard handling stays consistent with whether
        // `init_layer_shell` actually ran.
        let layer_shell = should_use_layer_shell(is_fullscreen_overlay);

        let mut builder = gtk4::Window::builder();

        if is_fullscreen_overlay {
            // Do NOT force a default size here. When all four edges are
            // anchored (below), the compositor sends the output's exact
            // dimensions in its configure event and gtk4-layer-shell uses
            // them — exactly like the fill-output widget path
            // (`widget.rs`). Forcing a giant `default_width`/`default_height`
            // instead pins the surface to that fixed size, which Hyprland
            // clamps to the WHOLE desktop span (the sum of every output's
            // width) rather than the single target output, leaving the
            // page's centered card off-center on multi-monitor layouts.
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
            builder = builder.decorated(true).title(panel_title(&view_name));
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
                // Pin to the monitor supplied by the caller (the bar
                // widget's `@<connector>` suffix on the view key) so the
                // overlay opens on the same display as the bar that
                // triggered it. When no monitor is supplied (the launcher
                // keybind, a suffix-less `view.show`, or an unknown/stale
                // connector) we deliberately do NOT call `set_monitor`:
                // gtk4-layer-shell then leaves the output unset and the
                // compositor places the surface on the focused monitor,
                // which is what wofi/fuzzel/rofi do and what the user
                // expects. Forcing `monitors.item(0)` here made the
                // launcher and wifi menu open on the wrong display on
                // multi-monitor setups.
                if let Some(m) = monitor.as_ref() {
                    window.set_monitor(m);
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

        // Create and embed WebView. Every panel and overlay shares the render
        // process via the anchor (share_process is always true): overlays are
        // hidden and reused on dismiss, never destroyed, so an isolated renderer
        // would stay resident regardless.
        let webview = crate::web_process::new_webview(&web_context, share_process);

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
        let inspector_enabled = std::env::var("QUANTUM_INSPECTOR").as_deref() == Ok("1");
        let settings: webkit6::Settings =
            webkit6::prelude::WebViewExt::settings(&webview).unwrap_or_default();
        settings.set_enable_write_console_messages_to_stdout(inspector_enabled);
        settings.set_enable_developer_extras(inspector_enabled);
        settings.set_javascript_can_open_windows_automatically(false);
        crate::web_process::apply_widget_settings(&settings);
        webkit6::prelude::WebViewExt::set_settings(&webview, &settings);

        // Replace WebKit's browser right-click menu (back/forward/reload) with
        // nothing; the launcher/overlays drive their own interactions. Kept
        // only under the inspector flag so "Inspect Element" stays available.
        crate::windows::suppress_browser_context_menu(&webview, inspector_enabled);

        // Block external navigations (http/https) and open them in the user's
        // default browser via xdg-open. Internal quantum:// navigations are
        // allowed. This prevents the WebView from navigating away from its
        // plugin page when the user clicks an external link (for example in
        // rendered markdown).
        crate::windows::install_navigation_policy(&webview);

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

        let url = crate::windows::resolve_view_uri(&view_name);
        webview.load_uri(&url);
        crate::windows::install_web_process_crash_handler(&webview, &view_name);
        window.set_child(Some(&webview));

        // Per-webview subscription set, shared between the bridge (which
        // seeds and updates it from `bridge.subscribe` / `bridge.unsubscribe`
        // messages) and the broadcast forwarder below (which filters on it).
        // `None` until the client seeds its first subscription: forward all.
        let subs: crate::subscriptions::WebviewSubscriptions =
            std::sync::Arc::new(std::sync::Mutex::new(None));

        // Register the bridge to wire JS messages to the dispatcher. The
        // dispatcher Arc is consumed here; the bridge keeps its own clone
        // inside the script message handler closure.
        crate::bridge::register_bridge(&webview, dispatcher, runtime.clone(), subs.clone());

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
        let theme_store_for_notify = theme_store.clone();
        let subs_for_notify = subs.clone();
        runtime.spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(env) => {
                        // Filter server-side: skip channels this webview never
                        // subscribed to (once seeded). The lock is held only
                        // for the pure decision, never across an await. A
                        // poisoned lock fails open so a panic elsewhere can
                        // never silently freeze this webview's live updates.
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
                        // client has installed `window.__quantum_notify`.
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
                            let push =
                                crate::windows::theme_reload_push_js(&theme_store_for_notify);
                            if js_tx.send(push).is_err() {
                                break;
                            }
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
            is_destroyed: false,
            owns_render_process: !share_process,
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

    /// Tear the panel down: dispose the widget tree and destroy the
    /// underlying surface so the embedded `WebView` is finalized and its
    /// `WebKitWebProcess` terminates.
    fn destroy(&mut self) {
        if self.is_destroyed {
            return;
        }
        self.is_destroyed = true;
        // Destroying the GTK window unparents the WebView but does NOT
        // terminate its render process: for a view on its own render process
        // (every destroy_on_dismiss view since B1) WebKit keeps the
        // WebKitWebProcess alive, so the renderer memory the destroy was meant
        // to reclaim leaks — and a fresh process is spawned on the next open,
        // stacking up hundreds of resident processes over a session. Terminate
        // the render process explicitly so it exits and its memory returns to
        // the OS. This host has no unsaved page state, so a forceful terminate
        // (versus the graceful, possibly-async try_close) is correct. Only
        // terminate when this view OWNS its process: terminating a shared
        // (`related-view`) process would kill it for every warm sibling.
        if self.owns_render_process {
            webkit6::prelude::WebViewExt::terminate_web_process(&self.webview);
        }
        gtk4::prelude::GtkWindowExt::destroy(&self.window);
    }

    fn inject_view_args(&mut self, args: Option<serde_json::Value>) {
        crate::windows::inject_view_args(&self.webview, args);
    }
}

impl Drop for PanelWindow {
    /// Safety net: if the handle is dropped without an explicit destroy, tear
    /// the surface out here. Guarded so this never double-calls
    /// gtk_window_destroy after an explicit destroy(), which would abort on an
    /// already-freed layer-shell surface.
    fn drop(&mut self) {
        if !self.is_destroyed {
            // Only terminate a process this view owns — never a shared
            // (`related-view`) process, which warm siblings depend on.
            if self.owns_render_process {
                webkit6::prelude::WebViewExt::terminate_web_process(&self.webview);
            }
            gtk4::prelude::GtkWindowExt::destroy(&self.window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::should_use_layer_shell;

    #[test]
    fn overlay_always_uses_layer_shell() {
        // An overlay dims the whole screen and dismisses on Escape, so it
        // cannot lock the user out: it always uses layer-shell.
        assert!(should_use_layer_shell(true));
    }

    #[test]
    fn plain_panel_never_uses_layer_shell() {
        // A plain (non-overlay) panel is always a normal xdg-toplevel the
        // user can close or switch away from — it never uses layer-shell.
        assert!(!should_use_layer_shell(false));
    }

    #[test]
    fn panel_title_derives_from_last_segment() {
        use super::panel_title;
        assert_eq!(panel_title("plugin/files/files"), "Files");
        assert_eq!(panel_title("plugin/power-menu/power-menu"), "Power Menu");
        assert_eq!(panel_title("files"), "Files");
        assert_eq!(panel_title(""), "Quantum");
    }
}
