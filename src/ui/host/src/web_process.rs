//! Shared WebKit render-process anchor.
//!
//! WebKitGTK spawns one web process per [`webkit6::WebView`]. The `related-view`
//! construct property (via [`webkit6::WebView::builder`]) makes a new view share
//! the render process and network session of an existing "anchor" view. Every
//! Quantum view is same-origin (the `quantum://` scheme) on the same
//! `WebContext`, so every view — the always-resident widgets (bar, clock,
//! timers, toast) and every panel and overlay (launcher, wifi, power, sound,
//! bluetooth, notification center, files, task manager) — can ride a single
//! shared render process instead of one process each.
//!
//! Overlays are hidden and reused on dismiss, never destroyed (destroying a
//! layer-shell window segfaults; see `WindowRegistry::handle`), so their
//! renderer stays resident for the session either way. Sharing therefore keeps
//! every resident overlay renderer in the one shared process rather than paying
//! a whole separate process per overlay type. The `share_process` parameter is
//! retained so a future session-safe teardown could isolate genuinely
//! destroyed views again.
//!
//! The anchor is a single hidden `WebView` created lazily on the first
//! shared-process request and cached in a `thread_local!` for the lifetime of
//! the process. All `WebView` construction happens on the GTK main thread, so a
//! thread-local holding the `!Send` `WebView` is safe and needs no locking.

use std::cell::RefCell;

thread_local! {
    /// The lazily created, never-shown anchor whose render process the warm
    /// views share. `None` until the first `new_webview(true)` call.
    static ANCHOR: RefCell<Option<webkit6::WebView>> = const { RefCell::new(None) };
}

/// Create a [`webkit6::WebView`].
///
/// When `share_process` is `true` the returned view shares the process-lifetime
/// anchor's render process (creating the anchor on first use); when `false` the
/// view gets its own isolated render process.
pub fn new_webview(share_process: bool) -> webkit6::WebView {
    if !share_process {
        return webkit6::WebView::new();
    }
    ANCHOR.with(|cell| {
        let mut slot = cell.borrow_mut();
        let anchor = slot.get_or_insert_with(webkit6::WebView::new).clone();
        webkit6::WebView::builder().related_view(&anchor).build()
    })
}

/// Disable WebKit subsystems no Quantum view uses.
///
/// Every view is a static Svelte user interface driven over the `quantum://`
/// IPC bridge: none use a 3D canvas, media capture (camera/microphone),
/// WebRTC, Web Audio, encrypted media (DRM), DNS prefetching (the scheme is
/// local), hyperlink auditing, back/forward navigation, or the legacy HTML5
/// database / offline application cache. Turning these off keeps each renderer
/// from initializing machinery it will never exercise. Base media playback
/// (`enable_media`), JavaScript, and local storage are left on deliberately.
///
/// Call this on the [`webkit6::Settings`] of every view before applying them.
pub fn apply_widget_settings(settings: &webkit6::Settings) {
    settings.set_enable_webgl(false);
    settings.set_enable_webrtc(false);
    settings.set_enable_media_stream(false);
    settings.set_enable_mock_capture_devices(false);
    settings.set_enable_webaudio(false);
    settings.set_enable_encrypted_media(false);
    settings.set_enable_media_capabilities(false);
    settings.set_enable_mediasource(false);
    settings.set_enable_back_forward_navigation_gestures(false);
    // `enable_hyperlink_auditing` is deprecated in WebKitGTK 2.52 and does
    // nothing (it logs a warning per view), so it is intentionally omitted.
    settings.set_enable_dns_prefetching(false);
    settings.set_enable_html5_database(false);
    settings.set_enable_offline_web_application_cache(false);
}
