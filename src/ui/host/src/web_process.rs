//! Shared WebKit render-process anchor and memory-tuned `WebContext`.
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
//!
//! ## Memory tuning (2026-09-02)
//!
//! The default `WebContext` allocates browser-grade caches that quantum never
//! benefits from (its views are tiny local Svelte bundles, not the open web).
//! [`build_web_context`] constructs an explicit context with:
//!
//! - **`MemoryPressureSettings`** — caps per-web-process memory at 512 MB,
//!   starts conservative GC at 50 %, aggressive release at 80 %, and kills the
//!   process at 2 × the limit (1 GB). Killed processes trigger
//!   `web-process-terminated` on every attached `WebView`, which callers must
//!   handle by reloading.
//! - **`CacheModel::DocumentViewer`** — minimal page/back-forward cache, sized
//!   for a document viewer rather than a general web browser.
//!
//! See `docs/plans/2026-09-02-memory-b-webkit-memory-pressure.md`.

use std::cell::RefCell;

/// Per-web-process memory limit in megabytes. WebKit's conservative and strict
/// thresholds are fractions of this, and the kill threshold a multiple. Tunable
/// via the `QUANTUM_WEBKIT_MEMORY_LIMIT_MB` environment variable; defaults to
/// 512 MB. The shared render process hosts all warm view DOM/JS heaps, so this
/// must accommodate the combined working set of bars, timers, launcher, clock,
/// and toast — but well below the multi-gigabyte growth seen on 2026-09-02.
const DEFAULT_MEMORY_LIMIT_MB: u32 = 512;

/// Build the memory-tuned [`webkit6::WebContext`] used for all quantum views.
///
/// Must be called exactly once, at activate time, before any `WebView` is
/// created. The returned context owns the `MemoryPressureSettings` and cache
/// model; register the `quantum://` scheme on it before loading any view.
pub fn build_web_context() -> webkit6::WebContext {
    let limit_mb = std::env::var("QUANTUM_WEBKIT_MEMORY_LIMIT_MB")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(DEFAULT_MEMORY_LIMIT_MB);

    let mut pressure = webkit6::MemoryPressureSettings::new();
    pressure.set_memory_limit(limit_mb);
    // Begin releasing caches (page cache, JIT code caches, malloc trim) when
    // the web process reaches 50 % of the limit.
    pressure.set_conservative_threshold(0.50);
    // Aggressive release (drop JIT caches, force full GC) at 80 %.
    pressure.set_strict_threshold(0.80);
    // Kill the web process at 2x the limit (last resort). If the shared process
    // is killed, every warm view receives `web-process-terminated` and must
    // reload. This is still better than the process growing unbounded.
    pressure.set_kill_threshold(2.0);
    // Check every 30 seconds.
    pressure.set_poll_interval(30.0);

    let context = webkit6::WebContext::builder()
        .memory_pressure_settings(&pressure)
        .build();

    // Minimal cache model — quantum serves tiny local Svelte bundles over
    // quantum://, not the open web. The default web-browser model keeps
    // page/back-forward caches sized for general browsing that waste memory.
    context.set_cache_model(webkit6::CacheModel::DocumentViewer);

    tracing::info!(
        limit_mb,
        "WebContext built with MemoryPressureSettings and DocumentViewer cache model"
    );

    context
}

thread_local! {
    /// The lazily created, never-shown anchor whose render process the warm
    /// views share. `None` until the first `new_webview(true)` call.
    static ANCHOR: RefCell<Option<webkit6::WebView>> = const { RefCell::new(None) };
}

/// Create a [`webkit6::WebView`] on the given [`webkit6::WebContext`].
///
/// When `share_process` is `true` the returned view shares the process-lifetime
/// anchor's render process (creating the anchor on first use); when `false` the
/// view gets its own isolated render process. All views use the explicit
/// context so they inherit its `MemoryPressureSettings` and `CacheModel`.
pub fn new_webview(context: &webkit6::WebContext, share_process: bool) -> webkit6::WebView {
    if !share_process {
        return webkit6::WebView::builder().web_context(context).build();
    }
    ANCHOR.with(|cell| {
        let mut slot = cell.borrow_mut();
        let anchor = slot
            .get_or_insert_with(|| webkit6::WebView::builder().web_context(context).build())
            .clone();
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
    // Disable WebKit's GPU-accelerated compositing. WebKitGTK loads
    // libvulkan_intel.so for its own internal compositor independently of
    // GSK_RENDERER, allocating six 1 GB sparse "state table" memfds via Mesa's
    // ANV driver that grow monotonically (840 MB resident at startup, 1.66 GB
    // after 13 h on 2026-09-02). Quantum's views are tiny Svelte widgets with
    // no WebGL, canvas, or video — software compositing is sufficient and
    // eliminates the Vulkan state pool overhead entirely. Easily reverted if
    // animation smoothness regresses.
    settings.set_hardware_acceleration_policy(webkit6::HardwareAccelerationPolicy::Never);
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
