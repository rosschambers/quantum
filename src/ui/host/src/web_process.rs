//! Shared WebKit render-process anchor.
//!
//! WebKitGTK spawns one web process per [`webkit6::WebView`]. The `related-view`
//! construct property (via [`webkit6::WebView::builder`]) makes a new view share
//! the render process and network session of an existing "anchor" view. Every
//! Quantum view is same-origin (the `quantum://` scheme) on the same
//! `WebContext`, so warm, always-resident views (bar, clock, timers, toast, and
//! the launcher) can all ride a single shared render process instead of one
//! process each.
//!
//! Transient overlays that are destroyed on dismiss deliberately do NOT share
//! the anchor: giving each its own render process means tearing it down on
//! dismiss frees a whole process, which is the point of `destroy_on_dismiss`.
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
