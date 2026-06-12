//! Window registry for managing GTK windows on the main thread.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::broadcast;

use quantum_domain::{ports::ThemeStore, EventEnvelope, WindowMode};

use gtk4::gdk;
use gtk4::prelude::*;

use crate::dispatcher::IpcDispatcher;
use crate::messages::WindowRequest;
use crate::windows::{PanelWindow, WidgetWindow};

use tracing::warn;

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
/// For most views the storage key is the full view string — the
/// `widgets/bar@<monitor>` pattern stores one window per monitor and
/// must keep its suffix. But for single-instance panels (the
/// power-menu, the launcher), the `@<monitor>` suffix is only used at
/// construction time to pick which monitor the surface anchors to;
/// subsequent show/hide/toggle requests against the same logical view
/// should hit the same stored window regardless of which monitor's
/// bar (or no bar) triggered them.
///
/// Without this stripping, opening the power-menu from a per-monitor
/// bar registered the window under e.g. `widgets/power-menu@DP-1`,
/// but the page's `view.hide` call uses the bare `widgets/power-menu`
/// — the registry then fails to find the window to hide, and the
/// menu becomes undismissable.
pub(crate) fn canonical_view_key(view: &str) -> String {
    let (prefix, _suffix) = split_view_key(view);
    match prefix {
        // Per-monitor: one window per monitor; keep the suffix.
        "widgets/bar" => view.to_string(),
        // Single-instance views (panels): drop the suffix.
        "launcher" | "widgets/power-menu" | "widgets/power-profile-menu" => prefix.to_string(),
        // Plugin views: one instance per plugin/view combination; keep
        // whatever the caller asked for (no suffix expected today).
        other if other.starts_with("plugin/") => view.to_string(),
        // Other widget views: one instance per name; keep the suffix
        // (no semantic difference today, but doesn't break anything).
        _ => view.to_string(),
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
}

impl ManagedWindowConstructor {
    /// Create a new window constructor.
    pub fn new(
        app: gtk4::Application,
        dispatcher: Arc<dyn IpcDispatcher>,
        theme_store: Arc<dyn ThemeStore>,
        runtime: Handle,
        event_tx: broadcast::Sender<EventEnvelope>,
    ) -> Self {
        Self {
            app,
            dispatcher,
            theme_store,
            runtime,
            event_tx,
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
}

impl WindowConstructor for ManagedWindowConstructor {
    type Window = ManagedWindow;

    fn construct(&mut self, view: &str) -> Option<Self::Window> {
        let (view_name, monitor_name_opt) = split_view_key(view);
        let monitor = monitor_name_opt.and_then(|name| {
            let resolved = self.find_monitor(name);
            if resolved.is_none() {
                tracing::warn!(
                    "widget {view}: requested monitor {name} not found; using compositor default"
                );
            }
            resolved
        });

        match view_name {
            "launcher" => Some(ManagedWindow::Panel(PanelWindow::new(
                &self.app,
                "launcher",
                self.dispatcher.clone(),
                self.theme_store.clone(),
                self.runtime.clone(),
                self.event_tx.clone(),
                None,
            ))),
            "widgets/power-menu" => Some(ManagedWindow::Panel(PanelWindow::new(
                &self.app,
                "widgets/power-menu",
                self.dispatcher.clone(),
                self.theme_store.clone(),
                self.runtime.clone(),
                self.event_tx.clone(),
                monitor,
            ))),
            "widgets/power-profile-menu" => Some(ManagedWindow::Panel(PanelWindow::new(
                &self.app,
                "widgets/power-profile-menu",
                self.dispatcher.clone(),
                self.theme_store.clone(),
                self.runtime.clone(),
                self.event_tx.clone(),
                monitor,
            ))),
            other if other.starts_with("widgets/") => {
                Some(ManagedWindow::Widget(WidgetWindow::new(
                    &self.app,
                    view_name.to_string(),
                    self.dispatcher.clone(),
                    self.theme_store.clone(),
                    self.runtime.clone(),
                    self.event_tx.clone(),
                    monitor,
                )))
            }
            other if other.starts_with("plugin/") => {
                // Plugin views look like `plugin/<plugin-name>/<view-name>`.
                // The window itself is a regular WidgetWindow; only the
                // URL it loads is different (see `WidgetWindow::new`,
                // which detects the `plugin/` prefix and emits a
                // `quantum://plugin/...` URL instead of a theme URL).
                Some(ManagedWindow::Widget(WidgetWindow::new(
                    &self.app,
                    view_name.to_string(),
                    self.dispatcher.clone(),
                    self.theme_store.clone(),
                    self.runtime.clone(),
                    self.event_tx.clone(),
                    monitor,
                )))
            }
            _ => None,
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
    /// view names to their declared window descriptors; dispatch through
    /// the catalog lands in a follow-up change.
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
                let key = canonical_view_key(&view);
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
                let key = canonical_view_key(&view);
                if let Some(window) = self.windows.get_mut(&key) {
                    window.set_height(height);
                } else {
                    warn!("set_height: view {} not open", view);
                }
            }
            WindowRequest::Close { view } => {
                match self.windows.remove(&canonical_view_key(&view)) {
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

    struct FakeCtor {
        construct_count: Rc<Cell<usize>>,
        shown: Rc<Cell<bool>>,
    }

    impl WindowConstructor for FakeCtor {
        type Window = FakeWindow;

        fn construct(&mut self, view: &str) -> Option<FakeWindow> {
            // Mirror ManagedWindowConstructor: strip the @<monitor>
            // suffix before matching the view name so callers can pass
            // either "launcher" or "launcher@DP-1" and get the same
            // window.
            let (view_name, _monitor) = split_view_key(view);
            if view_name == "launcher" {
                self.construct_count.set(self.construct_count.get() + 1);
                Some(FakeWindow {
                    shown: self.shown.clone(),
                })
            } else {
                None
            }
        }
    }

    #[test]
    fn registry_exposes_its_view_catalog() {
        use quantum_domain::{ViewDescriptor, ViewKind};
        let catalog = crate::ViewCatalog::from_plugins(vec![(
            "plugin/bar/bar".to_string(),
            ViewDescriptor {
                kind: ViewKind::Panel,
                ..ViewDescriptor::default()
            },
        )]);
        let reg = WindowRegistry::new(
            FakeCtor {
                construct_count: Rc::new(Cell::new(0)),
                shown: Rc::new(Cell::new(false)),
            },
            catalog,
        );
        let descriptor = reg.catalog().get("plugin/bar/bar");
        assert_eq!(descriptor.map(|d| d.kind), Some(ViewKind::Panel));
        assert!(reg.catalog().get("plugin/unknown/view").is_none());
    }

    #[test]
    fn first_request_constructs_window() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(
            FakeCtor {
                construct_count: count.clone(),
                shown: shown.clone(),
            },
            crate::ViewCatalog::from_plugins(vec![]),
        );
        reg.handle(WindowRequest::Open {
            view: "launcher".into(),
            mode: WindowMode::Show,
        });
        assert_eq!(count.get(), 1);
        assert!(shown.get());
    }

    #[test]
    fn second_request_reuses_window() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(
            FakeCtor {
                construct_count: count.clone(),
                shown: shown.clone(),
            },
            crate::ViewCatalog::from_plugins(vec![]),
        );
        reg.handle(WindowRequest::Open {
            view: "launcher".into(),
            mode: WindowMode::Show,
        });
        reg.handle(WindowRequest::Open {
            view: "launcher".into(),
            mode: WindowMode::Toggle,
        });
        assert_eq!(count.get(), 1);
        assert!(!shown.get()); // toggled off
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
        let mut reg = WindowRegistry::new(
            FakeCtor {
                construct_count: count,
                shown,
            },
            crate::ViewCatalog::from_plugins(vec![]),
        );
        reg.handle(WindowRequest::Open {
            view: "nope".into(),
            mode: WindowMode::Show,
        });
        // If we reach here without panic, the test passes.
    }

    #[test]
    fn close_removes_window_from_registry() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(
            FakeCtor {
                construct_count: count.clone(),
                shown: shown.clone(),
            },
            crate::ViewCatalog::from_plugins(vec![]),
        );
        reg.handle(WindowRequest::Open {
            view: "launcher".into(),
            mode: WindowMode::Show,
        });
        assert_eq!(count.get(), 1, "window was constructed");
        reg.handle(WindowRequest::Close {
            view: "launcher".into(),
        });
        // Re-opening should reconstruct (counter increments again),
        // which is the observable signal that Close actually removed
        // the entry from the windows map.
        reg.handle(WindowRequest::Open {
            view: "launcher".into(),
            mode: WindowMode::Show,
        });
        assert_eq!(count.get(), 2, "window was reconstructed after close");
        assert!(shown.get(), "reopened window is visible");
    }

    #[test]
    fn close_for_unknown_view_does_not_panic() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(
            FakeCtor {
                construct_count: count,
                shown,
            },
            crate::ViewCatalog::from_plugins(vec![]),
        );
        // No prior Open; the registry is empty.
        reg.handle(WindowRequest::Close {
            view: "launcher".into(),
        });
        // If we reach here without panic, the test passes.
    }

    #[test]
    fn canonical_view_key_strips_at_suffix_for_single_instance_panels() {
        assert_eq!(
            canonical_view_key("widgets/power-menu"),
            "widgets/power-menu"
        );
        assert_eq!(
            canonical_view_key("widgets/power-menu@eDP-1"),
            "widgets/power-menu"
        );
        assert_eq!(
            canonical_view_key("widgets/power-menu@DP-1"),
            "widgets/power-menu"
        );
        assert_eq!(canonical_view_key("launcher"), "launcher");
        assert_eq!(canonical_view_key("launcher@eDP-1"), "launcher");
    }

    #[test]
    fn canonical_view_key_preserves_at_suffix_for_per_monitor_bar() {
        // The bar is per-monitor: each monitor's bar is its own window.
        assert_eq!(canonical_view_key("widgets/bar"), "widgets/bar");
        assert_eq!(canonical_view_key("widgets/bar@eDP-1"), "widgets/bar@eDP-1");
        assert_eq!(canonical_view_key("widgets/bar@DP-1"), "widgets/bar@DP-1");
    }

    #[test]
    fn single_instance_panel_open_with_at_suffix_can_be_closed_by_bare_name() {
        // Regression: opening a single-instance panel with an @suffix
        // (e.g. launcher@DP-1 or widgets/power-menu@DP-1) used to
        // register the window under the full key, but the page's
        // view.hide call uses the bare name. Without canonicalization
        // the registry couldn't find the window to hide, making the
        // panel undismissable.
        //
        // Uses launcher because FakeCtor only constructs for that view
        // name; both launcher and widgets/power-menu hit the same
        // canonical_view_key code path.
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(
            FakeCtor {
                construct_count: count.clone(),
                shown: shown.clone(),
            },
            crate::ViewCatalog::from_plugins(vec![]),
        );
        reg.handle(WindowRequest::Open {
            view: "launcher@DP-1".into(),
            mode: WindowMode::Show,
        });
        // Should have constructed exactly one window.
        assert_eq!(count.get(), 1);
        assert!(shown.get());

        // Now close it using the BARE name (what the page calls).
        reg.handle(WindowRequest::Close {
            view: "launcher".into(),
        });

        // Reopen it: should construct a fresh window since the prior
        // one was closed.
        reg.handle(WindowRequest::Open {
            view: "launcher@eDP-1".into(),
            mode: WindowMode::Show,
        });
        assert_eq!(count.get(), 2);
    }
}
