//! Window registry for managing GTK windows on the main thread.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::broadcast;

use quantum_domain::{
    ports::ThemeStore, EventEnvelope, ViewAnchor, ViewDescriptor, ViewKind, ViewPosition,
    WindowMode,
};

use gtk4::gdk;
use gtk4::prelude::*;

use crate::dispatcher::IpcDispatcher;
use crate::messages::WindowRequest;
use crate::windows::{PanelWindow, WidgetWindow, WindowContext};

use tracing::warn;

/// Default panel width in pixels when a panel descriptor omits `width`.
const DEFAULT_PANEL_WIDTH: i32 = 480;
/// Default panel height in pixels when a panel descriptor omits `height`.
const DEFAULT_PANEL_HEIGHT: i32 = 320;

/// Resolved panel construction parameters derived from a [`ViewDescriptor`].
/// `overlay` selects fullscreen-overlay treatment versus a fixed-size centered
/// panel; `width`/`height` size the centered panel (and are ignored by the
/// overlay path, which spans the whole output). `share_process` splits by
/// lifetime: `destroy_on_dismiss` views get their OWN render process (it is
/// `false`), so tearing the window down on dismiss returns its renderer memory
/// to the OS instead of pinning it in the one shared process forever; warm,
/// always-resident views (the launcher and other views kept alive across
/// dismissals) stay shared (it is `true`), folding their resident renderers
/// into the single shared process rather than paying one process per type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PanelParams {
    pub overlay: bool,
    pub width: i32,
    pub height: i32,
    pub share_process: bool,
}

/// Resolved widget construction parameters derived from a [`ViewDescriptor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WidgetParams {
    pub anchor: ViewAnchor,
    pub fill_output: bool,
    pub click_through: bool,
    pub height: Option<u32>,
}

/// Map a panel/overlay descriptor to its concrete window parameters.
///
/// `ViewKind::Overlay` sets `overlay = true`; any other kind (the caller only
/// passes panels and overlays here) sets `overlay = false`. Missing
/// `width`/`height` fall back to [`DEFAULT_PANEL_WIDTH`]/[`DEFAULT_PANEL_HEIGHT`].
pub(crate) fn panel_params(descriptor: &ViewDescriptor) -> PanelParams {
    PanelParams {
        overlay: matches!(descriptor.kind, ViewKind::Overlay),
        width: descriptor
            .width
            .map(|w| w as i32)
            .unwrap_or(DEFAULT_PANEL_WIDTH),
        height: descriptor
            .height
            .map(|h| h as i32)
            .unwrap_or(DEFAULT_PANEL_HEIGHT),
        // Transient (destroy_on_dismiss) views get their own render process so
        // destroying them on dismiss returns renderer memory to the OS; warm,
        // always-resident views stay in the one shared process.
        share_process: !descriptor.destroy_on_dismiss,
    }
}

/// Map a widget descriptor to its concrete window parameters (anchor and the
/// optional exclusive-zone height, both passed straight through to
/// [`WidgetWindow::new`]).
pub(crate) fn widget_params(descriptor: &ViewDescriptor) -> WidgetParams {
    WidgetParams {
        anchor: descriptor.anchor,
        fill_output: descriptor.fill_output,
        click_through: descriptor.click_through,
        height: descriptor.height,
    }
}

/// Static deprecation alias map: legacy bare view names to their canonical
/// `plugin/<plugin>/<view>` names. Returns `Some(canonical)` on a hit so the
/// caller can decide whether to emit a deprecation warning. Bare names only
/// (the `@<monitor>` suffix is split off before this is consulted).
fn resolve_alias(bare: &str) -> Option<&'static str> {
    match bare {
        "widgets/bar" => Some("plugin/bar/bar"),
        "launcher" => Some("plugin/launcher/launcher"),
        "widgets/power-menu" => Some("plugin/power-menu/power-menu"),
        "widgets/power-profile-menu" => Some("plugin/power-profile-menu/power-profile-menu"),
        _ => None,
    }
}

/// Resolve a view name to its canonical `plugin/<plugin>/<view>` form,
/// silently following the alias map. A canonical name (or any name not in
/// the alias map) passes through unchanged. The `@<monitor>` suffix is not
/// handled here; callers that may pass a suffixed name should split it off
/// first. Used by the daemon to match config overrides (which may name a
/// view by its legacy alias or canonical name) against the canonical view
/// list computed from descriptors.
pub fn canonicalize_view_name(name: &str) -> String {
    match resolve_alias(name) {
        Some(canonical) => canonical.to_string(),
        None => name.to_string(),
    }
}

/// Resolve a bare legacy name to its canonical form, emitting a deprecation
/// warning on every alias hit. Names that are not aliases pass through
/// unchanged. Used at construction time; [`canonical_view_key`] resolves the
/// same alias silently to avoid log spam on high-frequency requests like the
/// bar's `set_height` ticks.
fn resolve_alias_warning(bare: &str) -> String {
    match resolve_alias(bare) {
        Some(canonical) => {
            warn!("deprecated view name '{bare}'; use canonical name '{canonical}' instead");
            canonical.to_string()
        }
        None => bare.to_string(),
    }
}

/// Split a view-name key on the first `@`. Returns `(prefix, suffix)`
/// where `suffix` is the optional monitor name. Pure function, no GTK
/// dependency — pulled out so tests can exercise the parsing without
/// constructing a `gdk::Display`.
pub(crate) fn split_view_key(view: &str) -> (&str, Option<&str>) {
    match view.split_once('@') {
        Some((prefix, suffix)) => (prefix, Some(suffix)),
        None => (view, None),
    }
}

/// Canonicalize a view key for window-map storage.
///
/// Two transformations make the storage key stable regardless of how a
/// caller addressed the view:
///
/// 1. **Alias resolution.** Legacy bare names (`launcher`,
///    `widgets/power-menu`, ...) are rewritten to their canonical
///    `plugin/<plugin>/<view>` form. This means an Open via the legacy
///    name and a Close via the canonical name (or vice versa) land on the
///    same stored window.
/// 2. **Single-instance suffix stripping.** Single-instance views (panels
///    and overlays by default) drop their `@<monitor>` suffix: the suffix
///    is only used at construction time to pick which monitor the surface
///    anchors to. Per-monitor widgets (the bar) keep the suffix so each
///    monitor's surface is its own window.
///
/// Whether a view is single-instance is read from its [`ViewDescriptor`]
/// via [`ViewDescriptor::effective_single_instance`]. A name with no
/// catalog entry keeps its suffix (today's default), so theme-hosted
/// widgets like the clock are unaffected.
///
/// Without alias resolution + suffix stripping, opening the power-menu from
/// a per-monitor bar registered the window under e.g.
/// `widgets/power-menu@DP-1`, but the page's `view.hide` call uses the
/// bare canonical name — the registry then failed to find the window to
/// hide, and the menu became undismissable.
pub(crate) fn canonical_view_key(view: &str, catalog: &crate::ViewCatalog) -> String {
    let (prefix, suffix) = split_view_key(view);
    let canonical = match resolve_alias(prefix) {
        Some(canonical) => canonical.to_string(),
        None => prefix.to_string(),
    };
    let single_instance = catalog
        .get(&canonical)
        .map(ViewDescriptor::effective_single_instance)
        .unwrap_or(false);
    match (single_instance, suffix) {
        // Single-instance: drop the suffix so every request for this
        // logical view hits the same stored window.
        (true, _) => canonical,
        // Per-monitor / catalog-miss with a suffix: keep the suffix so
        // each monitor's surface is its own window.
        (false, Some(s)) => format!("{canonical}@{s}"),
        (false, None) => canonical,
    }
}

/// Operations that all managed windows must support.
pub trait WindowOps {
    fn show(&mut self);
    fn hide(&mut self);
    fn toggle(&mut self);
    /// Tear the window down for good: dispose the widget tree and destroy
    /// the underlying surface so the embedded `WebView` is finalized and its
    /// `WebKitWebProcess` terminates. Merely hiding a window keeps the whole
    /// widget tree (and its live renderer) resident, so the renderer process
    /// leaks. Every removal path must call this before dropping the handle.
    fn destroy(&mut self);
    /// Resize the window to the given pixel height. The bar uses this
    /// when a popover opens so the popover has room to render below the
    /// visible row. Default no-op for windows that don't care about
    /// runtime resizing.
    fn set_height(&mut self, _height: u32) {}
    /// Set the pointer input region of the window. The bar uses this so its
    /// full-height surface only captures clicks over the visible strip and
    /// any open menu. `None` resets to the strip-only default. Default no-op
    /// for windows that do not manage an input region.
    fn set_input_region(&mut self, _region: Option<quantum_domain::WindowInputRegion>) {}
    /// Inject view arguments as `window.__quantum_args` into the WebView.
    /// Default no-op for windows that do not need view args.
    fn inject_view_args(&mut self, _args: Option<serde_json::Value>) {}
}

/// Stable identity of a connected monitor. Two values compare equal only
/// while GDK keeps the same underlying `gdk::Monitor` object; a monitor
/// flap (disconnect/reconnect) yields a new object and a new `MonitorId`,
/// which is how the registry detects that a per-monitor window's binding
/// went stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorId(usize);

/// Abstraction for constructing windows, allowing test injection.
/// Note: Not Send/Sync because GTK types are not thread-safe. This registry
/// lives entirely on the GTK main thread and is never shared.
pub trait WindowConstructor {
    type Window;
    fn construct(&mut self, view: &str) -> Option<Self::Window>;
    /// Current identity of the monitor named `connector`, or `None` if no
    /// connected monitor matches. Default `None` for constructors that do not
    /// manage real monitors (test doubles override).
    fn monitor_identity(&self, _connector: &str) -> Option<MonitorId> {
        None
    }
}

/// Enum of all managed window types.
pub enum ManagedWindow {
    Panel(PanelWindow),
    Widget(WidgetWindow),
}

/// Real window constructor that builds GTK windows.
pub struct ManagedWindowConstructor {
    dispatcher: Arc<dyn IpcDispatcher>,
    theme_store: Arc<dyn ThemeStore>,
    runtime: Handle,
    event_tx: broadcast::Sender<EventEnvelope>,
    catalog: crate::ViewCatalog,
}

impl ManagedWindowConstructor {
    /// Create a new window constructor. The `catalog` supplies the
    /// [`ViewDescriptor`] for each canonical view name so `construct`
    /// dispatches on declared window semantics instead of hardcoded names.
    pub fn new(
        dispatcher: Arc<dyn IpcDispatcher>,
        theme_store: Arc<dyn ThemeStore>,
        runtime: Handle,
        event_tx: broadcast::Sender<EventEnvelope>,
        catalog: crate::ViewCatalog,
    ) -> Self {
        Self {
            dispatcher,
            theme_store,
            runtime,
            event_tx,
            catalog,
        }
    }

    /// Look up a `gdk::Monitor` by its Wayland connector name (the
    /// suffix in `widgets/bar@<connector>` view keys). Returns
    /// `None` if no currently-connected monitor matches.
    fn find_monitor(&self, name: &str) -> Option<gdk::Monitor> {
        let display = gdk::Display::default()?;
        display
            .monitors()
            .iter::<gdk::Monitor>()
            .filter_map(Result::ok)
            .find(|m| crate::windows::widget::monitor_name(m).as_deref() == Some(name))
    }

    /// Build the shared [`WindowContext`] for a single construction, cloning
    /// the host-context fields the windows consume and bundling in the
    /// resolved monitor.
    fn window_context(&self, monitor: Option<gdk::Monitor>) -> WindowContext {
        WindowContext {
            dispatcher: self.dispatcher.clone(),
            theme_store: self.theme_store.clone(),
            runtime: self.runtime.clone(),
            event_tx: self.event_tx.clone(),
            monitor,
        }
    }
}

impl WindowConstructor for ManagedWindowConstructor {
    type Window = ManagedWindow;

    fn construct(&mut self, view: &str) -> Option<Self::Window> {
        let (prefix, monitor_name_opt) = split_view_key(view);
        // Resolve the legacy alias (warning on every hit) before any catalog
        // lookup or URL building so the rest of the flow only ever deals in
        // canonical `plugin/<plugin>/<view>` names.
        let canonical = resolve_alias_warning(prefix);
        let monitor = monitor_name_opt.and_then(|name| {
            let resolved = self.find_monitor(name);
            if resolved.is_none() {
                tracing::warn!(
                    "view {canonical}: requested monitor {name} not found; using compositor default"
                );
            }
            resolved
        });

        // A descriptor in the catalog drives dispatch; absent one, fall back
        // to the legacy default WidgetWindow behavior so theme-hosted widgets
        // and manifest-less plugins keep working. Both helpers only read self,
        // so they take `&self` and the borrow of `self.catalog` for the
        // descriptor can coexist with them.
        match self.catalog.get(&canonical) {
            Some(descriptor) => {
                Some(self.construct_from_descriptor(&canonical, descriptor, monitor))
            }
            None => self.construct_fallback(&canonical, monitor),
        }
    }

    fn monitor_identity(&self, connector: &str) -> Option<MonitorId> {
        use glib::translate::ToGlibPtr;
        self.find_monitor(connector).map(|monitor| {
            let pointer: *const gdk::ffi::GdkMonitor = monitor.to_glib_none().0;
            MonitorId(pointer as usize)
        })
    }
}

impl ManagedWindowConstructor {
    /// Build a window from an explicit [`ViewDescriptor`], dispatching on its
    /// declared `kind`. The descriptor-to-parameter mapping lives in the pure
    /// [`panel_params`]/[`widget_params`] functions so it can be unit-tested
    /// without GTK.
    fn construct_from_descriptor(
        &self,
        canonical: &str,
        descriptor: &ViewDescriptor,
        monitor: Option<gdk::Monitor>,
    ) -> ManagedWindow {
        let ctx = self.window_context(monitor);
        match descriptor.kind {
            ViewKind::Widget => {
                let params = widget_params(descriptor);
                ManagedWindow::Widget(WidgetWindow::new(
                    ctx,
                    canonical.to_string(),
                    params.anchor,
                    descriptor.position,
                    params.fill_output,
                    params.click_through,
                    params.height,
                ))
            }
            ViewKind::Panel | ViewKind::Overlay => {
                let params = panel_params(descriptor);
                ManagedWindow::Panel(PanelWindow::new(
                    ctx,
                    canonical.to_string(),
                    params.overlay,
                    params.width,
                    params.height,
                    params.share_process,
                ))
            }
            ViewKind::Toast => ManagedWindow::Widget(WidgetWindow::new_toast(
                ctx,
                canonical.to_string(),
                descriptor.position,
            )),
        }
    }

    /// Construct a window for a canonical name that has no catalog entry.
    ///
    /// - `plugin/...` names get a default WidgetWindow loading the plugin URL
    ///   (the moon-distance / manifest-less plugin path: anchor none, default
    ///   height).
    /// - Theme-hosted `widgets/...` names get a default WidgetWindow loading
    ///   the theme URL (the clock widget path), exactly as before.
    /// - Anything else is genuinely unknown and yields `None`.
    fn construct_fallback(
        &self,
        canonical: &str,
        monitor: Option<gdk::Monitor>,
    ) -> Option<ManagedWindow> {
        let is_plugin = canonical.starts_with("plugin/");
        // Detect a theme-hosted widget (the only remaining one is the clock)
        // by splitting the first path segment, so the alias map stays the
        // single source of the legacy-name string literals.
        let is_theme_widget = matches!(canonical.split_once('/'), Some(("widgets", _)));
        if is_plugin || is_theme_widget {
            Some(ManagedWindow::Widget(WidgetWindow::new(
                self.window_context(monitor),
                canonical.to_string(),
                ViewAnchor::None,
                // Fallback widgets (the clock) keep their historical top-right
                // placement: Center maps to top-right in the background branch.
                ViewPosition::Center,
                false,
                // Fallback widgets are never fill-output, so click-through does
                // not apply.
                false,
                None,
            )))
        } else {
            None
        }
    }
}

impl WindowOps for ManagedWindow {
    fn show(&mut self) {
        match self {
            ManagedWindow::Panel(w) => w.show(),
            ManagedWindow::Widget(w) => w.show(),
        }
    }

    fn hide(&mut self) {
        match self {
            ManagedWindow::Panel(w) => w.hide(),
            ManagedWindow::Widget(w) => w.hide(),
        }
    }

    fn toggle(&mut self) {
        match self {
            ManagedWindow::Panel(w) => w.toggle(),
            ManagedWindow::Widget(w) => w.toggle(),
        }
    }

    fn destroy(&mut self) {
        match self {
            ManagedWindow::Panel(w) => w.destroy(),
            ManagedWindow::Widget(w) => w.destroy(),
        }
    }

    fn set_height(&mut self, height: u32) {
        match self {
            ManagedWindow::Panel(w) => w.set_height(height),
            ManagedWindow::Widget(w) => w.set_height(height),
        }
    }

    fn set_input_region(&mut self, region: Option<quantum_domain::WindowInputRegion>) {
        match self {
            ManagedWindow::Panel(w) => w.set_input_region(region),
            ManagedWindow::Widget(w) => w.set_input_region(region),
        }
    }

    fn inject_view_args(&mut self, args: Option<serde_json::Value>) {
        match self {
            ManagedWindow::Panel(w) => w.inject_view_args(args),
            ManagedWindow::Widget(w) => w.inject_view_args(args),
        }
    }
}

/// Registry for managing all windows on the GTK main thread.
pub struct WindowRegistry<C: WindowConstructor> {
    constructor: C,
    windows: HashMap<String, C::Window>,
    /// The `@<monitor>` suffix each stored window was constructed for, keyed
    /// by the same storage key as `windows`. Single-instance views strip the
    /// suffix from their key, so this is the only record of which monitor a
    /// reused window is pinned to; an Open that asks for a different monitor
    /// uses it to decide the window must be rebuilt there.
    window_monitor: HashMap<String, Option<String>>,
    /// The monitor identity each stored window was built against, keyed by the
    /// same storage key as `windows`. A per-monitor (suffixed) key embeds its
    /// connector in the key, so `window_monitor` above always matches for it;
    /// this instead records the underlying `gdk::Monitor` object identity at
    /// construction. When a monitor flaps (disconnect/reconnect) GDK recreates
    /// its object and its `MonitorId` changes, leaving the hidden window bound
    /// to a stale monitor. Comparing the current identity against this stored
    /// one detects that flap so the window is rebuilt against the fresh object.
    window_monitor_id: HashMap<String, Option<MonitorId>>,
    catalog: crate::ViewCatalog,
}

impl<C: WindowConstructor> WindowRegistry<C> {
    /// Create a new window registry. The `catalog` maps canonical plugin
    /// view names to their declared window descriptors and is consulted by
    /// [`canonical_view_key`] to decide single-instance suffix stripping.
    pub fn new(constructor: C, catalog: crate::ViewCatalog) -> Self {
        Self {
            constructor,
            windows: HashMap::new(),
            window_monitor: HashMap::new(),
            window_monitor_id: HashMap::new(),
            catalog,
        }
    }

    /// The descriptor catalog this registry was constructed with.
    pub fn catalog(&self) -> &crate::ViewCatalog {
        &self.catalog
    }

    /// Whether the view addressed by `view` is flagged `destroy_on_dismiss`.
    ///
    /// The window map is keyed by the possibly-suffixed
    /// [`canonical_view_key`], but the catalog is keyed by the bare canonical
    /// name, so this resolves the descriptor the same way that key does its
    /// alias step: split the `@<monitor>` suffix, resolve the legacy alias to
    /// the canonical name, then read the descriptor. An absent descriptor
    /// (theme-hosted widgets, unknown names) is treated as warm (`false`), so
    /// only views that explicitly opt in are torn down on dismiss.
    fn is_destroy_on_dismiss(&self, view: &str) -> bool {
        let (prefix, _suffix) = split_view_key(view);
        let canonical = match resolve_alias(prefix) {
            Some(canonical) => canonical.to_string(),
            None => prefix.to_string(),
        };
        self.catalog
            .get(&canonical)
            .map(|descriptor| descriptor.destroy_on_dismiss)
            .unwrap_or(false)
    }

    /// Tear down and remove the stored window under `key`, destroying it
    /// WITHOUT hiding first.
    ///
    /// Only `destroy_on_dismiss` views take this path (see
    /// [`Self::is_destroy_on_dismiss`]). Since B1 those views own their render
    /// process, so destroying the window terminates that process and returns
    /// its renderer memory to the OS. The destroy must not be preceded by a
    /// `hide()`: hiding a layer-shell overlay releases its Wayland surface, and
    /// a subsequent `destroy()` aborts with the `gdk_surface_get_display`
    /// assertion. The entry is removed from both `windows` and `window_monitor`
    /// so the next open reconstructs a fresh window.
    fn destroy_window(&mut self, key: &str)
    where
        C::Window: WindowOps,
    {
        if let Some(mut window) = self.windows.remove(key) {
            window.destroy();
        }
        self.window_monitor.remove(key);
        self.window_monitor_id.remove(key);
    }

    /// Hide a stored window and KEEP it for reuse on the next open.
    ///
    /// This is the warm path: `destroy_on_dismiss == false` views (the
    /// launcher, bar, timers) stay resident across dismissals, folding their
    /// renderers into the one shared render process, so their windows are
    /// hidden rather than destroyed. Destroying a warm window would gain
    /// nothing (its renderer is shared and cannot be freed alone) and, for a
    /// layer-shell surface, historically drove
    /// `gdk_wayland_toplevel_remove_from_session` to segfault. Flagged views
    /// take [`Self::destroy_window`] instead. Shared by the `Close` handler.
    fn hide_window(&mut self, key: &str)
    where
        C::Window: WindowOps,
    {
        if let Some(window) = self.windows.get_mut(key) {
            window.hide();
        }
    }

    /// Handle a window request (construct or reuse window, then apply the operation).
    pub fn handle(&mut self, req: WindowRequest)
    where
        C::Window: WindowOps,
    {
        match req {
            WindowRequest::Open { view, mode, args } => {
                tracing::debug!("WindowRegistry::handle view={} mode={:?}", view, mode);
                let key = canonical_view_key(&view, &self.catalog);
                // Dismiss handling splits by the descriptor's
                // destroy_on_dismiss flag. Flagged views (power-menu and the
                // other transient overlays) own their render process since B1,
                // so dismissing one tears the window down: the isolated process
                // terminates and its renderer memory returns to the OS. A live
                // experiment on this host (GTK 4.22.4, WebKit 2.52.4) proved 75
                // destroys across every overlay type crash-free, and the
                // windows are plain gtk4::Window (not GtkApplicationWindow), so
                // the old gdk_wayland_toplevel_remove_from_session segfault no
                // longer applies. Warm views (destroy_on_dismiss == false: the
                // launcher, bar, timers) instead stay resident across
                // dismissals, folding their renderers into the one shared
                // process, so they are hidden and kept for reuse.
                //
                // A dismiss on the flagged path destroys and removes the window
                // here, before the reuse/construct block: a Hide is always a
                // dismiss, and a Toggle is a dismiss only when the window is
                // currently present (flagged windows are always removed on
                // dismiss, so map presence tracks visibility). Destroying must
                // NOT hide first (see destroy_window). Show, and Toggle when the
                // window is absent, fall through to construct-and-show below.
                let flagged = self.is_destroy_on_dismiss(&view);
                let dismiss = matches!(mode, WindowMode::Hide)
                    || (matches!(mode, WindowMode::Toggle) && self.windows.contains_key(&key));
                if flagged && dismiss {
                    self.destroy_window(&key);
                    tracing::info!("destroyed window (dismiss): {view}");
                    return;
                }
                let (_, requested_suffix) = split_view_key(&view);
                let requested = requested_suffix.map(str::to_string);
                // A single-instance view (overlay/panel) keeps one window under
                // a suffix-stripped key, fixing its monitor at construction. If
                // it was last built for a different monitor, evict it so the
                // Open below reconstructs and re-anchors it on the monitor that
                // asked. (Per-monitor views embed the suffix in their key, so
                // the stored suffix always matches and this never fires.)
                if self
                    .window_monitor
                    .get(&key)
                    .is_some_and(|built| *built != requested)
                {
                    if let Some(mut old) = self.windows.remove(&key) {
                        // The evicted window is removed from the map, so it can
                        // never be reused; destroy it to reclaim its renderer
                        // rather than orphaning a hidden surface. Destroy is safe
                        // on a plain gtk4::Window, and a fresh window is
                        // reconstructed below on the requested monitor. Destroy
                        // must NOT be preceded by a hide: hiding a layer-shell
                        // overlay releases its Wayland surface and a later destroy
                        // aborts, so we go straight to destroy.
                        old.destroy();
                    }
                }
                // A per-monitor view (bar) embeds its connector in the key, so
                // the single-instance eviction above never fires for it, yet its
                // stored window can still go stale: a monitor flap
                // (disconnect/reconnect) makes GDK recreate the `gdk::Monitor`
                // object, changing its identity while the hidden window keeps its
                // binding to the old object. On reshow the compositor mis-places
                // the surface onto the wrong output. Detect that here for a
                // per-monitor (suffixed) key by comparing the connector's current
                // monitor identity against the one the stored window was built
                // against; if they differ, evict the stale window so the entry
                // block below reconstructs it against the fresh monitor object.
                // `requested.as_deref()` reuses the owned suffix string to avoid
                // borrowing `view` while `key` is also borrowed here.
                let current_id = requested
                    .as_deref()
                    .and_then(|connector| self.constructor.monitor_identity(connector));
                if requested.is_some()
                    && self.windows.contains_key(&key)
                    && current_id != self.window_monitor_id.get(&key).copied().flatten()
                {
                    tracing::info!(
                        "per-monitor view {view}: monitor object changed, rebuilding window"
                    );
                    if let Some(mut old) = self.windows.remove(&key) {
                        // Destroy the stale-bound window rather than orphaning a
                        // hidden surface; the entry block below reconstructs a
                        // fresh one against the current monitor object. Destroy
                        // must NOT be preceded by a hide (see destroy_window).
                        old.destroy();
                    }
                }
                let window = match self.windows.entry(key.clone()) {
                    std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                    std::collections::hash_map::Entry::Vacant(v) => {
                        let Some(w) = self.constructor.construct(&view) else {
                            warn!("Unknown view: {}", view);
                            return;
                        };
                        v.insert(w)
                    }
                };
                window.inject_view_args(args);
                self.window_monitor_id.insert(key.clone(), current_id);
                self.window_monitor.insert(key, requested);
                match mode {
                    WindowMode::Toggle => window.toggle(),
                    WindowMode::Show => window.show(),
                    WindowMode::Hide => window.hide(),
                }
            }
            WindowRequest::SetHeight { view, height } => {
                tracing::debug!(
                    "WindowRegistry::handle set_height view={} h={}",
                    view,
                    height
                );
                let key = canonical_view_key(&view, &self.catalog);
                if let Some(window) = self.windows.get_mut(&key) {
                    window.set_height(height);
                } else {
                    warn!("set_height: view {} not open", view);
                }
            }
            WindowRequest::SetInputRegion { view, region } => {
                tracing::debug!(
                    "WindowRegistry::handle set_input_region view={} region={:?}",
                    view,
                    region
                );
                let key = canonical_view_key(&view, &self.catalog);
                if let Some(window) = self.windows.get_mut(&key) {
                    window.set_input_region(region);
                } else {
                    warn!("set_input_region: view {} not open", view);
                }
            }
            WindowRequest::Close { view } => {
                let key = canonical_view_key(&view, &self.catalog);
                if self.windows.contains_key(&key) {
                    // Close splits by the destroy_on_dismiss flag, exactly like
                    // the Open dismiss path. A flagged view (power-menu and the
                    // other transient overlays) owns its render process, so
                    // Close destroys and removes the window WITHOUT hiding first
                    // (see destroy_window), terminating that process to reclaim
                    // its memory. A warm view (the launcher, bar, timers) is
                    // hidden and kept for reuse (see hide_window).
                    if self.is_destroy_on_dismiss(&view) {
                        self.destroy_window(&key);
                        tracing::info!("destroyed window (close request): {view}");
                    } else {
                        self.hide_window(&key);
                        tracing::info!("hid window (close request): {view}");
                    }
                } else {
                    self.window_monitor.remove(&key);
                    self.window_monitor_id.remove(&key);
                    tracing::debug!("close request for unknown view: {view}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn panel_params_panel_kind_is_not_overlay_with_declared_size() {
        let descriptor = ViewDescriptor {
            kind: ViewKind::Panel,
            width: Some(600),
            height: Some(420),
            ..ViewDescriptor::default()
        };
        assert_eq!(
            panel_params(&descriptor),
            PanelParams {
                overlay: false,
                width: 600,
                height: 420,
                share_process: true,
            }
        );
    }

    #[test]
    fn panel_params_panel_kind_falls_back_to_defaults_when_size_missing() {
        let descriptor = ViewDescriptor {
            kind: ViewKind::Panel,
            ..ViewDescriptor::default()
        };
        assert_eq!(
            panel_params(&descriptor),
            PanelParams {
                overlay: false,
                width: DEFAULT_PANEL_WIDTH,
                height: DEFAULT_PANEL_HEIGHT,
                share_process: true,
            }
        );
    }

    #[test]
    fn panel_params_overlay_kind_sets_overlay_flag() {
        // The power-menu overlay declares no width/height, so it should pick
        // up the defaults while flipping the overlay flag on.
        let descriptor = ViewDescriptor {
            kind: ViewKind::Overlay,
            ..ViewDescriptor::default()
        };
        assert_eq!(
            panel_params(&descriptor),
            PanelParams {
                overlay: true,
                width: DEFAULT_PANEL_WIDTH,
                height: DEFAULT_PANEL_HEIGHT,
                share_process: true,
            }
        );
    }

    #[test]
    fn panel_params_overlay_kind_keeps_declared_size() {
        let descriptor = ViewDescriptor {
            kind: ViewKind::Overlay,
            width: Some(440),
            height: Some(320),
            ..ViewDescriptor::default()
        };
        assert_eq!(
            panel_params(&descriptor),
            PanelParams {
                overlay: true,
                width: 440,
                height: 320,
                share_process: true,
            }
        );
    }

    #[test]
    fn panel_params_shares_process_for_warm_panel() {
        // A panel that is NOT destroyed on dismiss (the launcher) is warm and
        // joins the shared render process.
        let descriptor = ViewDescriptor {
            kind: ViewKind::Panel,
            destroy_on_dismiss: false,
            ..ViewDescriptor::default()
        };
        assert!(panel_params(&descriptor).share_process);
    }

    #[test]
    fn panel_params_isolates_process_for_transient_panel() {
        // A destroy_on_dismiss panel or overlay gets its OWN render process so
        // that destroying it on dismiss returns its renderer memory to the OS.
        // Sharing would pin that memory in the one shared process forever.
        let descriptor = ViewDescriptor {
            kind: ViewKind::Panel,
            destroy_on_dismiss: true,
            ..ViewDescriptor::default()
        };
        assert!(!panel_params(&descriptor).share_process);
    }

    #[test]
    fn widget_params_passes_anchor_and_height_through() {
        let descriptor = ViewDescriptor {
            kind: ViewKind::Widget,
            anchor: ViewAnchor::Top,
            height: Some(32),
            ..ViewDescriptor::default()
        };
        assert_eq!(
            widget_params(&descriptor),
            WidgetParams {
                anchor: ViewAnchor::Top,
                fill_output: false,
                click_through: false,
                height: Some(32),
            }
        );
    }

    #[test]
    fn widget_params_defaults_to_no_anchor_and_no_height() {
        // A manifest-less widget (default descriptor) anchors nowhere and
        // leaves the height unset, matching the background-layer fallback.
        let descriptor = ViewDescriptor::default();
        assert_eq!(
            widget_params(&descriptor),
            WidgetParams {
                anchor: ViewAnchor::None,
                fill_output: false,
                click_through: false,
                height: None,
            }
        );
    }

    #[test]
    fn widget_params_passes_fill_output_through() {
        let descriptor = ViewDescriptor {
            kind: ViewKind::Widget,
            fill_output: true,
            ..ViewDescriptor::default()
        };
        assert_eq!(
            widget_params(&descriptor),
            WidgetParams {
                anchor: ViewAnchor::None,
                fill_output: true,
                click_through: false,
                height: None,
            }
        );
    }

    #[test]
    fn widget_params_passes_click_through_through() {
        let descriptor = ViewDescriptor {
            kind: ViewKind::Widget,
            fill_output: true,
            click_through: true,
            ..ViewDescriptor::default()
        };
        assert_eq!(
            widget_params(&descriptor),
            WidgetParams {
                anchor: ViewAnchor::None,
                fill_output: true,
                click_through: true,
                height: None,
            }
        );
    }

    #[test]
    fn window_constructor_monitor_identity_defaults_to_none() {
        struct Stub;
        impl WindowConstructor for Stub {
            type Window = FakeWindow;
            fn construct(&mut self, _view: &str) -> Option<FakeWindow> {
                None
            }
        }
        let stub = Stub;
        assert_eq!(stub.monitor_identity("DP-1"), None);
    }

    struct FakeWindow {
        shown: Rc<Cell<bool>>,
        /// Records the most recent `set_input_region` call. `None` means the
        /// method was never called; `Some(inner)` captures the argument
        /// (itself `Option`, distinguishing a strip-only reset from a menu
        /// rectangle).
        input_region: Rc<Cell<Option<Option<quantum_domain::WindowInputRegion>>>>,
        /// Counts how many times `destroy` was called, so tests can assert
        /// teardown happened on the removal paths.
        destroyed: Rc<Cell<usize>>,
        /// Counts hide() calls. The teardown path must destroy WITHOUT hiding
        /// first: hiding a layer-shell overlay releases its Wayland surface,
        /// and a subsequent destroy() aborts with the gdk_surface_get_display
        /// assertion (observed in a live quantumd coredump).
        hidden: Rc<Cell<usize>>,
    }

    impl WindowOps for FakeWindow {
        fn show(&mut self) {
            self.shown.set(true);
        }
        fn hide(&mut self) {
            self.hidden.set(self.hidden.get() + 1);
            self.shown.set(false);
        }
        fn toggle(&mut self) {
            self.shown.set(!self.shown.get());
        }
        fn destroy(&mut self) {
            self.destroyed.set(self.destroyed.get() + 1);
            // A destroyed window is no longer shown. The teardown path
            // destroys WITHOUT hiding first (see
            // dismiss_destroys_once_without_hiding_first), so this is where a
            // dismissed flagged overlay becomes not-shown in the mock.
            self.shown.set(false);
        }
        fn set_input_region(&mut self, region: Option<quantum_domain::WindowInputRegion>) {
            self.input_region.set(Some(region));
        }
    }

    /// A `FakeCtor` that mirrors `ManagedWindowConstructor`'s dispatch
    /// decision (alias-resolve, then construct for any canonical name that
    /// would yield a real window; `None` for genuinely unknown names)
    /// without touching GTK. It records construct calls so tests can assert
    /// reuse / reconstruction.
    struct FakeCtor {
        construct_count: Rc<Cell<usize>>,
        shown: Rc<Cell<bool>>,
        input_region: Rc<Cell<Option<Option<quantum_domain::WindowInputRegion>>>>,
        destroyed: Rc<Cell<usize>>,
        hidden: Rc<Cell<usize>>,
        /// Connector -> current monitor identity; tests mutate this to
        /// simulate a monitor flap.
        monitors: Rc<RefCell<HashMap<String, MonitorId>>>,
    }

    impl WindowConstructor for FakeCtor {
        type Window = FakeWindow;

        fn construct(&mut self, view: &str) -> Option<FakeWindow> {
            // Mirror ManagedWindowConstructor: split the @<monitor> suffix,
            // resolve the legacy alias, then decide whether a window would be
            // built. Catalog-driven dispatch (Widget/Panel/Overlay) and the
            // plugin/theme/none fallback all collapse here to "is a window
            // built at all": plugin/* and widgets/* names build one, anything
            // else does not.
            let (prefix, _monitor) = split_view_key(view);
            let canonical = match resolve_alias(prefix) {
                Some(c) => c.to_string(),
                None => prefix.to_string(),
            };
            let builds_window = canonical.starts_with("plugin/")
                || matches!(canonical.split_once('/'), Some(("widgets", _)));
            if builds_window {
                self.construct_count.set(self.construct_count.get() + 1);
                Some(FakeWindow {
                    shown: self.shown.clone(),
                    input_region: self.input_region.clone(),
                    destroyed: self.destroyed.clone(),
                    hidden: self.hidden.clone(),
                })
            } else {
                None
            }
        }

        fn monitor_identity(&self, connector: &str) -> Option<MonitorId> {
            self.monitors.borrow().get(connector).copied()
        }
    }

    /// A catalog carrying the four first-party descriptors keyed by their
    /// canonical names, so `canonical_view_key` can consult
    /// `effective_single_instance`.
    fn first_party_catalog() -> crate::ViewCatalog {
        crate::ViewCatalog::from_plugins(vec![
            (
                "plugin/bar/bar".to_string(),
                ViewDescriptor {
                    kind: ViewKind::Widget,
                    per_monitor: true,
                    auto_show: true,
                    anchor: ViewAnchor::Top,
                    height: Some(32),
                    ..ViewDescriptor::default()
                },
            ),
            (
                "plugin/launcher/launcher".to_string(),
                ViewDescriptor {
                    kind: ViewKind::Panel,
                    width: Some(600),
                    height: Some(420),
                    ..ViewDescriptor::default()
                },
            ),
            (
                "plugin/power-menu/power-menu".to_string(),
                ViewDescriptor {
                    kind: ViewKind::Overlay,
                    destroy_on_dismiss: true,
                    ..ViewDescriptor::default()
                },
            ),
            (
                "plugin/power-profile-menu/power-profile-menu".to_string(),
                ViewDescriptor {
                    kind: ViewKind::Overlay,
                    ..ViewDescriptor::default()
                },
            ),
        ])
    }

    fn fake_ctor(count: &Rc<Cell<usize>>, shown: &Rc<Cell<bool>>) -> FakeCtor {
        FakeCtor {
            construct_count: count.clone(),
            shown: shown.clone(),
            input_region: Rc::new(Cell::new(None)),
            destroyed: Rc::new(Cell::new(0)),
            hidden: Rc::new(Cell::new(0)),
            monitors: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    #[allow(clippy::type_complexity)]
    fn fake_ctor_with_region(
        count: &Rc<Cell<usize>>,
        shown: &Rc<Cell<bool>>,
        input_region: &Rc<Cell<Option<Option<quantum_domain::WindowInputRegion>>>>,
    ) -> FakeCtor {
        FakeCtor {
            construct_count: count.clone(),
            shown: shown.clone(),
            input_region: input_region.clone(),
            destroyed: Rc::new(Cell::new(0)),
            hidden: Rc::new(Cell::new(0)),
            monitors: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// A `FakeCtor` that threads an explicit `destroyed` counter into every
    /// window it builds, so tests can assert that a removal path (close or
    /// monitor eviction) tore the window down instead of merely hiding it.
    fn fake_ctor_with_destroy(
        count: &Rc<Cell<usize>>,
        shown: &Rc<Cell<bool>>,
        destroyed: &Rc<Cell<usize>>,
    ) -> FakeCtor {
        FakeCtor {
            construct_count: count.clone(),
            shown: shown.clone(),
            input_region: Rc::new(Cell::new(None)),
            destroyed: destroyed.clone(),
            hidden: Rc::new(Cell::new(0)),
            monitors: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// A `FakeCtor` that threads both an explicit `destroyed` and `hidden`
    /// counter into every window it builds, so tests can assert that a
    /// teardown path destroyed the window WITHOUT hiding it first.
    fn fake_ctor_with_teardown(
        count: &Rc<Cell<usize>>,
        shown: &Rc<Cell<bool>>,
        destroyed: &Rc<Cell<usize>>,
        hidden: &Rc<Cell<usize>>,
    ) -> FakeCtor {
        FakeCtor {
            construct_count: count.clone(),
            shown: shown.clone(),
            input_region: Rc::new(Cell::new(None)),
            destroyed: destroyed.clone(),
            hidden: hidden.clone(),
            monitors: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// A `FakeCtor` seeded with connector -> identity pairs, returning the shared
    /// `monitors` map so a test can mutate it mid-scenario to simulate a flap.
    fn fake_ctor_with_monitors(
        count: &Rc<Cell<usize>>,
        shown: &Rc<Cell<bool>>,
        pairs: &[(&str, usize)],
    ) -> (FakeCtor, Rc<RefCell<HashMap<String, MonitorId>>>) {
        let monitors = Rc::new(RefCell::new(
            pairs
                .iter()
                .map(|(connector, id)| ((*connector).to_string(), MonitorId(*id)))
                .collect::<HashMap<String, MonitorId>>(),
        ));
        let ctor = FakeCtor {
            construct_count: count.clone(),
            shown: shown.clone(),
            input_region: Rc::new(Cell::new(None)),
            destroyed: Rc::new(Cell::new(0)),
            hidden: Rc::new(Cell::new(0)),
            monitors: monitors.clone(),
        };
        (ctor, monitors)
    }

    #[test]
    fn fake_ctor_reports_and_updates_monitor_identity() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let (ctor, monitors) = fake_ctor_with_monitors(&count, &shown, &[("DP-1", 1)]);
        assert_eq!(ctor.monitor_identity("DP-1"), Some(MonitorId(1)));
        assert_eq!(ctor.monitor_identity("eDP-1"), None);

        // Simulate a flap: DP-1's identity changes.
        monitors.borrow_mut().insert("DP-1".into(), MonitorId(2));
        assert_eq!(ctor.monitor_identity("DP-1"), Some(MonitorId(2)));
    }

    #[test]
    fn per_monitor_window_rebuilds_when_monitor_identity_changes() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let destroyed = Rc::new(Cell::new(0));
        let monitors = Rc::new(RefCell::new(std::collections::HashMap::from([(
            "DP-1".to_string(),
            MonitorId(1),
        )])));
        let ctor = FakeCtor {
            construct_count: count.clone(),
            shown: shown.clone(),
            input_region: Rc::new(Cell::new(None)),
            destroyed: destroyed.clone(),
            hidden: Rc::new(Cell::new(0)),
            monitors: monitors.clone(),
        };
        let mut reg = WindowRegistry::new(ctor, first_party_catalog());

        reg.handle(WindowRequest::Open {
            view: "plugin/bar/bar@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        reg.handle(WindowRequest::Close {
            view: "plugin/bar/bar@DP-1".into(),
        });
        assert_eq!(count.get(), 1, "constructed once");
        assert_eq!(
            destroyed.get(),
            0,
            "warm view hidden, not destroyed, on close"
        );

        // Simulate a flap: DP-1's monitor object identity changes.
        monitors.borrow_mut().insert("DP-1".into(), MonitorId(2));

        reg.handle(WindowRequest::Open {
            view: "plugin/bar/bar@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(
            destroyed.get(),
            1,
            "stale-bound window destroyed on flap reopen"
        );
        assert_eq!(count.get(), 2, "reconstructed against the fresh monitor");
        assert!(shown.get(), "rebuilt bar is shown");
    }

    #[test]
    fn per_monitor_window_reused_when_monitor_identity_unchanged() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let destroyed = Rc::new(Cell::new(0));
        let monitors = Rc::new(RefCell::new(std::collections::HashMap::from([(
            "DP-1".to_string(),
            MonitorId(1),
        )])));
        let ctor = FakeCtor {
            construct_count: count.clone(),
            shown: shown.clone(),
            input_region: Rc::new(Cell::new(None)),
            destroyed: destroyed.clone(),
            hidden: Rc::new(Cell::new(0)),
            monitors: monitors.clone(),
        };
        let mut reg = WindowRegistry::new(ctor, first_party_catalog());
        reg.handle(WindowRequest::Open {
            view: "plugin/bar/bar@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        reg.handle(WindowRequest::Close {
            view: "plugin/bar/bar@DP-1".into(),
        });
        reg.handle(WindowRequest::Open {
            view: "plugin/bar/bar@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        }); // same identity
        assert_eq!(count.get(), 1, "warm window reused, not rebuilt");
        assert_eq!(destroyed.get(), 0, "warm window not destroyed");
        assert!(shown.get());
    }

    #[test]
    fn resolve_alias_maps_legacy_names_to_canonical() {
        assert_eq!(resolve_alias("widgets/bar"), Some("plugin/bar/bar"));
        assert_eq!(resolve_alias("launcher"), Some("plugin/launcher/launcher"));
        assert_eq!(
            resolve_alias("widgets/power-menu"),
            Some("plugin/power-menu/power-menu")
        );
        assert_eq!(
            resolve_alias("widgets/power-profile-menu"),
            Some("plugin/power-profile-menu/power-profile-menu")
        );
        // Non-alias names pass through as misses.
        assert_eq!(resolve_alias("widgets/clock"), None);
        assert_eq!(resolve_alias("plugin/bar/bar"), None);
        assert_eq!(resolve_alias("nope"), None);
    }

    #[test]
    fn registry_exposes_its_view_catalog() {
        let reg = WindowRegistry::new(
            fake_ctor(&Rc::new(Cell::new(0)), &Rc::new(Cell::new(false))),
            first_party_catalog(),
        );
        let descriptor = reg.catalog().get("plugin/launcher/launcher");
        assert_eq!(descriptor.map(|d| d.kind), Some(ViewKind::Panel));
        assert!(reg.catalog().get("plugin/unknown/view").is_none());
    }

    #[test]
    fn first_request_constructs_window() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(fake_ctor(&count, &shown), first_party_catalog());
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 1);
        assert!(shown.get());
    }

    #[test]
    fn second_request_reuses_window() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(fake_ctor(&count, &shown), first_party_catalog());
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
            args: None,
        });
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Toggle,
            args: None,
        });
        assert_eq!(count.get(), 1);
        assert!(!shown.get()); // toggled off
    }

    #[test]
    fn legacy_alias_open_and_canonical_close_hit_same_window() {
        // Open via the deprecated `launcher` alias, then close via the
        // canonical `plugin/launcher/launcher` name. Both must resolve to the
        // same storage key or the close would silently miss.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(fake_ctor(&count, &shown), first_party_catalog());
        reg.handle(WindowRequest::Open {
            view: "launcher".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 1);
        assert!(shown.get(), "window is shown after open");
        reg.handle(WindowRequest::Close {
            view: "plugin/launcher/launcher".into(),
        });
        // The canonical-name close hid the alias-keyed window: if it had missed
        // the key, the window would still be shown. Close hides (never destroys)
        // and keeps the window, so reopening via the canonical name reuses it
        // (the counter does not grow).
        assert!(!shown.get(), "canonical close hid the alias-opened window");
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 1, "reopen reuses the same-keyed window");
        assert!(shown.get(), "reopened window is shown");
    }

    #[test]
    fn single_instance_overlay_reuses_on_same_monitor() {
        // Re-opening a single-instance overlay from the same monitor reuses
        // the one stored window (here toggling it back off), never rebuilding.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(fake_ctor(&count, &shown), first_party_catalog());
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Toggle,
            args: None,
        });
        assert_eq!(count.get(), 1);
        assert!(!shown.get());
    }

    #[test]
    fn single_instance_overlay_reconstructs_on_different_monitor() {
        // A single-instance overlay (notification center, power menu, ...)
        // drops its `@<monitor>` suffix from the storage key, so the monitor
        // is fixed at construction. Opening it from a bar on another monitor
        // must move it there: evict the surface pinned to the old monitor and
        // construct a fresh one on the monitor that asked.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(fake_ctor(&count, &shown), first_party_catalog());
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-2".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 2);
        assert!(shown.get());
    }

    #[test]
    fn splits_view_key_with_monitor() {
        assert_eq!(
            split_view_key("widgets/bar@DP-1"),
            ("widgets/bar", Some("DP-1"))
        );
    }

    #[test]
    fn splits_view_key_without_monitor() {
        assert_eq!(split_view_key("widgets/bar"), ("widgets/bar", None));
    }

    #[test]
    fn splits_view_key_with_complex_suffix() {
        // Hyprland connector names can include slashes in theory; we
        // only split on the FIRST `@` so a suffix like "DP-1@2" (not a
        // real Hyprland name, but defensive) parses correctly.
        assert_eq!(
            split_view_key("widgets/bar@DP-1@2"),
            ("widgets/bar", Some("DP-1@2"))
        );
    }

    #[test]
    fn unknown_view_does_not_panic() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(fake_ctor(&count, &shown), first_party_catalog());
        reg.handle(WindowRequest::Open {
            view: "nope".into(),
            mode: WindowMode::Show,
            args: None,
        });
        // If we reach here without panic, the test passes.
        assert_eq!(count.get(), 0, "no window built for unknown view");
    }

    #[test]
    fn close_hides_window_and_keeps_it_for_reuse() {
        // Close must NOT destroy: destroying a gtk4-layer-shell window
        // segfaults in gdk_wayland_toplevel_remove_from_session. The window is
        // hidden and kept, so reopening reuses it (the counter does not grow).
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(fake_ctor(&count, &shown), first_party_catalog());
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 1, "window was constructed");
        reg.handle(WindowRequest::Close {
            view: "plugin/launcher/launcher".into(),
        });
        assert!(!shown.get(), "close hides the window");
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(
            count.get(),
            1,
            "reopen reuses the kept window, no reconstruct"
        );
        assert!(shown.get(), "reopened window is visible");
    }

    #[test]
    fn close_hides_without_destroying() {
        // Regression for a live SIGSEGV: Close must hide, never destroy, because
        // gtk_window_destroy on a layer-shell surface aborts in
        // gdk_wayland_toplevel_remove_from_session.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let destroyed = Rc::new(Cell::new(0));
        let hidden = Rc::new(Cell::new(0));
        let mut reg = WindowRegistry::new(
            fake_ctor_with_teardown(&count, &shown, &destroyed, &hidden),
            first_party_catalog(),
        );
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
            args: None,
        });
        reg.handle(WindowRequest::Close {
            view: "plugin/launcher/launcher".into(),
        });
        assert_eq!(destroyed.get(), 0, "close must never destroy the window");
        assert_eq!(hidden.get(), 1, "close hides the window");
    }

    #[test]
    fn close_destroys_flagged_view() {
        // Closing a flagged (destroy_on_dismiss = true) view tears the window
        // down instead of hiding it, so its isolated render process terminates
        // and its memory returns to the OS. The teardown destroys WITHOUT hiding
        // first (hiding a layer-shell overlay releases its Wayland surface and a
        // later destroy() would abort). Reopening reconstructs a fresh window.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let destroyed = Rc::new(Cell::new(0));
        let hidden = Rc::new(Cell::new(0));
        let mut reg = WindowRegistry::new(
            fake_ctor_with_teardown(&count, &shown, &destroyed, &hidden),
            first_party_catalog(),
        );
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 1, "window was constructed");
        reg.handle(WindowRequest::Close {
            view: "plugin/power-menu/power-menu@DP-1".into(),
        });
        assert_eq!(destroyed.get(), 1, "close destroys the flagged view");
        assert_eq!(hidden.get(), 0, "close destroys WITHOUT hiding first");
        assert!(!shown.get(), "flagged view is not shown after close");
        // The window was removed, so reopening RECONSTRUCTS it.
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 2, "reopen reconstructs the destroyed window");
        assert!(shown.get(), "reopened flagged view is shown");
    }

    #[test]
    fn monitor_eviction_destroys_old_and_reconstructs() {
        // Opening a single-instance overlay on one monitor and then another
        // evicts the surface pinned to the old monitor and reconstructs it on
        // the new one. The evicted window has already been removed from the
        // map, so it can never be reused; destroying it reclaims its renderer.
        // Destroy is safe on a plain gtk4::Window and must NOT be preceded by a
        // hide (hiding a layer-shell overlay releases its Wayland surface and a
        // later destroy would abort), so the evicted window is destroyed
        // without hiding first.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let destroyed = Rc::new(Cell::new(0));
        let hidden = Rc::new(Cell::new(0));
        let mut reg = WindowRegistry::new(
            fake_ctor_with_teardown(&count, &shown, &destroyed, &hidden),
            first_party_catalog(),
        );
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-2".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(
            count.get(),
            2,
            "eviction must reconstruct on the new monitor"
        );
        assert_eq!(
            destroyed.get(),
            1,
            "eviction destroys the old window to reclaim its renderer"
        );
        assert_eq!(hidden.get(), 0, "eviction destroys WITHOUT hiding first");
    }

    #[test]
    fn dismiss_destroys_flagged_overlay() {
        // The power-menu overlay is flagged destroy_on_dismiss = true, so it
        // owns its render process (B1). Dismissing it must DESTROY the window
        // WITHOUT hiding first, so the isolated render process terminates and
        // returns its memory to the OS. Hiding a layer-shell overlay releases
        // its Wayland surface and a later destroy() would abort, so the
        // teardown must not hide before destroying.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let destroyed = Rc::new(Cell::new(0));
        let hidden = Rc::new(Cell::new(0));
        let mut reg = WindowRegistry::new(
            fake_ctor_with_teardown(&count, &shown, &destroyed, &hidden),
            first_party_catalog(),
        );
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 1, "first open constructs the window");
        // Dismiss destroys the flagged window without hiding it first.
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Hide,
            args: None,
        });
        assert_eq!(destroyed.get(), 1, "dismiss destroys the flagged overlay");
        assert_eq!(hidden.get(), 0, "dismiss destroys WITHOUT hiding first");
        assert!(!shown.get(), "overlay is not shown after dismiss");
        // The window was removed, so reopening RECONSTRUCTS it.
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 2, "reopen reconstructs the destroyed window");
        assert!(shown.get(), "overlay is shown again after reopen");
    }

    #[test]
    fn flagged_overlay_toggle_off_destroys_toggle_on_reconstructs() {
        // The power-menu overlay is flagged destroy_on_dismiss = true. Toggling
        // it off is a dismiss, so it destroys and removes the window (freeing
        // its isolated render process). Toggling it back on RECONSTRUCTS a fresh
        // window because the prior one was torn down.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let destroyed = Rc::new(Cell::new(0));
        let hidden = Rc::new(Cell::new(0));
        let mut reg = WindowRegistry::new(
            fake_ctor_with_teardown(&count, &shown, &destroyed, &hidden),
            first_party_catalog(),
        );
        // Toggle when absent -> construct + show.
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Toggle,
            args: None,
        });
        assert_eq!(count.get(), 1);
        assert!(shown.get());
        // Toggle when present -> destroy without hiding first.
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Toggle,
            args: None,
        });
        assert_eq!(
            destroyed.get(),
            1,
            "toggle-off destroys the flagged overlay"
        );
        assert_eq!(hidden.get(), 0, "toggle-off destroys WITHOUT hiding first");
        assert!(!shown.get(), "toggle-off dismisses the overlay");
        // Toggle again -> reconstruct (the prior window was destroyed) and show.
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Toggle,
            args: None,
        });
        assert_eq!(count.get(), 2, "toggle-on reconstructs after destroy");
        assert!(shown.get());
    }

    #[test]
    fn unflagged_view_hide_does_not_destroy() {
        // launcher is NOT flagged: Hide parks it, reused on next Show.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let destroyed = Rc::new(Cell::new(0));
        let mut reg = WindowRegistry::new(
            fake_ctor_with_destroy(&count, &shown, &destroyed),
            first_party_catalog(),
        );
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
            args: None,
        });
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Hide,
            args: None,
        });
        assert_eq!(destroyed.get(), 0, "unflagged Hide must not destroy");
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(
            count.get(),
            1,
            "unflagged view is reused, not reconstructed"
        );
    }

    #[test]
    fn hide_destroys_flagged_view() {
        // Hide on a flagged (destroy_on_dismiss = true) view destroys and
        // removes the window WITHOUT hiding first, freeing its isolated render
        // process. Reopening reconstructs a fresh window.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let destroyed = Rc::new(Cell::new(0));
        let hidden = Rc::new(Cell::new(0));
        let mut reg = WindowRegistry::new(
            fake_ctor_with_teardown(&count, &shown, &destroyed, &hidden),
            first_party_catalog(),
        );
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Hide,
            args: None,
        });
        assert_eq!(destroyed.get(), 1, "hide destroys the flagged view");
        assert_eq!(hidden.get(), 0, "hide destroys WITHOUT hiding first");
        reg.handle(WindowRequest::Open {
            view: "plugin/power-menu/power-menu@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 2, "flagged view is reconstructed after hide");
    }

    #[test]
    fn hide_parks_warm_view() {
        // Hide on a warm (destroy_on_dismiss = false) view parks it: it is
        // hidden and kept, never destroyed, and reopening reuses the same
        // window without reconstructing.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let destroyed = Rc::new(Cell::new(0));
        let hidden = Rc::new(Cell::new(0));
        let mut reg = WindowRegistry::new(
            fake_ctor_with_teardown(&count, &shown, &destroyed, &hidden),
            first_party_catalog(),
        );
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
            args: None,
        });
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Hide,
            args: None,
        });
        assert_eq!(destroyed.get(), 0, "warm Hide must not destroy");
        assert_eq!(hidden.get(), 1, "warm Hide parks the window");
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 1, "warm view is reused, not reconstructed");
    }

    #[test]
    fn set_input_region_request_routes_to_open_window() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let region_cell = Rc::new(Cell::new(None));
        let mut reg = WindowRegistry::new(
            fake_ctor_with_region(&count, &shown, &region_cell),
            first_party_catalog(),
        );
        reg.handle(WindowRequest::Open {
            view: "plugin/bar/bar@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        let region = quantum_domain::WindowInputRegion {
            x: 0,
            y: 0,
            width: 300,
            height: 32,
        };
        reg.handle(WindowRequest::SetInputRegion {
            view: "plugin/bar/bar@DP-1".into(),
            region: Some(region),
        });
        assert_eq!(region_cell.get(), Some(Some(region)));
    }

    #[test]
    fn set_input_region_for_unopened_view_does_not_panic() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let region_cell = Rc::new(Cell::new(None));
        let mut reg = WindowRegistry::new(
            fake_ctor_with_region(&count, &shown, &region_cell),
            first_party_catalog(),
        );
        // No prior Open: the registry is empty, so the request must be a
        // no-op warning rather than a panic.
        reg.handle(WindowRequest::SetInputRegion {
            view: "plugin/bar/bar@DP-1".into(),
            region: None,
        });
        assert_eq!(region_cell.get(), None, "no window received the region");
    }

    #[test]
    fn close_for_unknown_view_does_not_panic() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(fake_ctor(&count, &shown), first_party_catalog());
        // No prior Open; the registry is empty.
        reg.handle(WindowRequest::Close {
            view: "plugin/launcher/launcher".into(),
        });
        // If we reach here without panic, the test passes.
    }

    #[test]
    fn canonical_view_key_strips_at_suffix_for_single_instance_panels() {
        let catalog = first_party_catalog();
        // Overlay (power-menu): single-instance -> suffix dropped, and the
        // legacy alias resolves to the canonical name.
        assert_eq!(
            canonical_view_key("plugin/power-menu/power-menu", &catalog),
            "plugin/power-menu/power-menu"
        );
        assert_eq!(
            canonical_view_key("plugin/power-menu/power-menu@eDP-1", &catalog),
            "plugin/power-menu/power-menu"
        );
        assert_eq!(
            canonical_view_key("widgets/power-menu@DP-1", &catalog),
            "plugin/power-menu/power-menu"
        );
        // Panel (launcher): single-instance -> suffix dropped.
        assert_eq!(
            canonical_view_key("plugin/launcher/launcher", &catalog),
            "plugin/launcher/launcher"
        );
        assert_eq!(
            canonical_view_key("launcher@eDP-1", &catalog),
            "plugin/launcher/launcher"
        );
    }

    #[test]
    fn canonical_view_key_preserves_at_suffix_for_per_monitor_bar() {
        let catalog = first_party_catalog();
        // The bar is a per-monitor widget: each monitor's bar is its own
        // window, so the suffix is kept. The legacy alias resolves first.
        assert_eq!(
            canonical_view_key("plugin/bar/bar", &catalog),
            "plugin/bar/bar"
        );
        assert_eq!(
            canonical_view_key("plugin/bar/bar@eDP-1", &catalog),
            "plugin/bar/bar@eDP-1"
        );
        assert_eq!(
            canonical_view_key("widgets/bar@DP-1", &catalog),
            "plugin/bar/bar@DP-1"
        );
    }

    #[test]
    fn canonical_view_key_keeps_suffix_for_catalog_miss() {
        // Theme-hosted widgets (the clock) have no catalog entry, so the
        // suffix is kept (today's default) and the name passes through
        // unchanged.
        let catalog = first_party_catalog();
        assert_eq!(
            canonical_view_key("widgets/clock", &catalog),
            "widgets/clock"
        );
        assert_eq!(
            canonical_view_key("widgets/clock@eDP-1", &catalog),
            "widgets/clock@eDP-1"
        );
    }

    #[test]
    fn single_instance_panel_open_with_at_suffix_can_be_closed_by_bare_name() {
        // Regression: opening a single-instance panel with an @suffix used to
        // register the window under the full key, but the page's view.hide
        // call uses the bare name. Without descriptor-driven canonicalization
        // the registry couldn't find the window to hide, making the panel
        // undismissable. Reworked against descriptors: the catalog marks the
        // launcher single-instance, so the suffix is dropped at both open and
        // close.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(fake_ctor(&count, &shown), first_party_catalog());
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher@DP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        // Should have constructed exactly one window.
        assert_eq!(count.get(), 1);
        assert!(shown.get());

        // Now close it using the BARE name (what the page calls).
        reg.handle(WindowRequest::Close {
            view: "plugin/launcher/launcher".into(),
        });

        // Reopen it: should construct a fresh window since the prior
        // one was closed.
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher@eDP-1".into(),
            mode: WindowMode::Show,
            args: None,
        });
        assert_eq!(count.get(), 2);
    }
}
