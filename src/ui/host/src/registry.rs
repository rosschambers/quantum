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
/// overlay path, which spans the whole output).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PanelParams {
    pub overlay: bool,
    pub width: i32,
    pub height: i32,
}

/// Resolved widget construction parameters derived from a [`ViewDescriptor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WidgetParams {
    pub anchor: ViewAnchor,
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
    }
}

/// Map a widget descriptor to its concrete window parameters (anchor and the
/// optional exclusive-zone height, both passed straight through to
/// [`WidgetWindow::new`]).
pub(crate) fn widget_params(descriptor: &ViewDescriptor) -> WidgetParams {
    WidgetParams {
        anchor: descriptor.anchor,
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
    /// Resize the window to the given pixel height. The bar uses this
    /// when a popover opens so the popover has room to render below the
    /// visible row. Default no-op for windows that don't care about
    /// runtime resizing.
    fn set_height(&mut self, _height: u32) {}
}

/// Abstraction for constructing windows, allowing test injection.
/// Note: Not Send/Sync because GTK types are not thread-safe. This registry
/// lives entirely on the GTK main thread and is never shared.
pub trait WindowConstructor {
    type Window;
    fn construct(&mut self, view: &str) -> Option<Self::Window>;
}

/// Enum of all managed window types.
pub enum ManagedWindow {
    Panel(PanelWindow),
    Widget(WidgetWindow),
}

/// Real window constructor that builds GTK windows.
pub struct ManagedWindowConstructor {
    app: gtk4::Application,
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
        app: gtk4::Application,
        dispatcher: Arc<dyn IpcDispatcher>,
        theme_store: Arc<dyn ThemeStore>,
        runtime: Handle,
        event_tx: broadcast::Sender<EventEnvelope>,
        catalog: crate::ViewCatalog,
    ) -> Self {
        Self {
            app,
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
    fn window_context(&self, monitor: Option<gdk::Monitor>) -> WindowContext<'_> {
        WindowContext {
            app: &self.app,
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

    fn set_height(&mut self, height: u32) {
        match self {
            ManagedWindow::Panel(w) => w.set_height(height),
            ManagedWindow::Widget(w) => w.set_height(height),
        }
    }
}

/// Registry for managing all windows on the GTK main thread.
pub struct WindowRegistry<C: WindowConstructor> {
    constructor: C,
    windows: HashMap<String, C::Window>,
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
            catalog,
        }
    }

    /// The descriptor catalog this registry was constructed with.
    pub fn catalog(&self) -> &crate::ViewCatalog {
        &self.catalog
    }

    /// Handle a window request (construct or reuse window, then apply the operation).
    pub fn handle(&mut self, req: WindowRequest)
    where
        C::Window: WindowOps,
    {
        match req {
            WindowRequest::Open { view, mode } => {
                tracing::debug!("WindowRegistry::handle view={} mode={:?}", view, mode);
                let key = canonical_view_key(&view, &self.catalog);
                let window = match self.windows.entry(key) {
                    std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                    std::collections::hash_map::Entry::Vacant(v) => {
                        let Some(w) = self.constructor.construct(&view) else {
                            warn!("Unknown view: {}", view);
                            return;
                        };
                        v.insert(w)
                    }
                };
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
            WindowRequest::Close { view } => {
                match self
                    .windows
                    .remove(&canonical_view_key(&view, &self.catalog))
                {
                    Some(mut window) => {
                        // Hide before drop so the layer-shell surface is
                        // released cleanly. The window's Drop releases the
                        // underlying GTK resources.
                        window.hide();
                        tracing::info!("closed window: {view}");
                    }
                    None => {
                        tracing::debug!("close request for unknown view: {view}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
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
            }
        );
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
                height: None,
            }
        );
    }

    struct FakeWindow {
        shown: Rc<Cell<bool>>,
    }

    impl WindowOps for FakeWindow {
        fn show(&mut self) {
            self.shown.set(true);
        }
        fn hide(&mut self) {
            self.shown.set(false);
        }
        fn toggle(&mut self) {
            self.shown.set(!self.shown.get());
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
                })
            } else {
                None
            }
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
        }
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
        });
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Toggle,
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
        });
        assert_eq!(count.get(), 1);
        reg.handle(WindowRequest::Close {
            view: "plugin/launcher/launcher".into(),
        });
        // Reopen via the canonical name reconstructs, proving the close
        // removed the entry the alias-keyed open inserted.
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
        });
        assert_eq!(count.get(), 2);
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
        });
        // If we reach here without panic, the test passes.
        assert_eq!(count.get(), 0, "no window built for unknown view");
    }

    #[test]
    fn close_removes_window_from_registry() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(fake_ctor(&count, &shown), first_party_catalog());
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
        });
        assert_eq!(count.get(), 1, "window was constructed");
        reg.handle(WindowRequest::Close {
            view: "plugin/launcher/launcher".into(),
        });
        // Re-opening should reconstruct (counter increments again),
        // which is the observable signal that Close actually removed
        // the entry from the windows map.
        reg.handle(WindowRequest::Open {
            view: "plugin/launcher/launcher".into(),
            mode: WindowMode::Show,
        });
        assert_eq!(count.get(), 2, "window was reconstructed after close");
        assert!(shown.get(), "reopened window is visible");
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
        });
        assert_eq!(count.get(), 2);
    }
}
