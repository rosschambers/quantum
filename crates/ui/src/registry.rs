//! Window registry for managing GTK windows on the main thread.

use std::collections::HashMap;
use std::sync::Arc;

use quantum_domain::{ports::ThemeStore, WindowMode};

use crate::dispatcher::IpcDispatcher;
use crate::messages::WindowRequest;
use crate::windows::{LauncherWindow, WidgetWindow};

use tracing::warn;

/// Operations that all managed windows must support.
pub trait WindowOps {
    fn show(&mut self);
    fn hide(&mut self);
    fn toggle(&mut self);
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
    Launcher(LauncherWindow),
    Widget(WidgetWindow),
}

/// Real window constructor that builds GTK windows.
pub struct ManagedWindowConstructor {
    app: gtk4::Application,
    dispatcher: Arc<dyn IpcDispatcher>,
    theme_store: Arc<dyn ThemeStore>,
}

impl ManagedWindowConstructor {
    /// Create a new window constructor.
    pub fn new(
        app: gtk4::Application,
        dispatcher: Arc<dyn IpcDispatcher>,
        theme_store: Arc<dyn ThemeStore>,
    ) -> Self {
        Self {
            app,
            dispatcher,
            theme_store,
        }
    }
}

impl WindowConstructor for ManagedWindowConstructor {
    type Window = ManagedWindow;

    fn construct(&mut self, view: &str) -> Option<Self::Window> {
        match view {
            "launcher" => Some(ManagedWindow::Launcher(LauncherWindow::new(
                &self.app,
                self.dispatcher.clone(),
                self.theme_store.clone(),
            ))),
            other if other.starts_with("widgets/") => Some(ManagedWindow::Widget(
                WidgetWindow::new(&self.app, other.to_string(), self.theme_store.clone()),
            )),
            _ => None,
        }
    }
}

impl WindowOps for ManagedWindow {
    fn show(&mut self) {
        match self {
            ManagedWindow::Launcher(w) => w.show(),
            ManagedWindow::Widget(w) => w.show(),
        }
    }

    fn hide(&mut self) {
        match self {
            ManagedWindow::Launcher(w) => w.hide(),
            ManagedWindow::Widget(w) => w.hide(),
        }
    }

    fn toggle(&mut self) {
        match self {
            ManagedWindow::Launcher(w) => w.toggle(),
            ManagedWindow::Widget(w) => w.toggle(),
        }
    }
}

/// Registry for managing all windows on the GTK main thread.
pub struct WindowRegistry<C: WindowConstructor> {
    constructor: C,
    windows: HashMap<String, C::Window>,
}

impl<C: WindowConstructor> WindowRegistry<C> {
    /// Create a new window registry.
    pub fn new(constructor: C) -> Self {
        Self {
            constructor,
            windows: HashMap::new(),
        }
    }

    /// Handle a window request (construct or reuse window, then apply the operation).
    pub fn handle(&mut self, req: WindowRequest)
    where
        C::Window: WindowOps,
    {
        let WindowRequest::Open { view, mode } = req;
        let window = match self.windows.entry(view.clone()) {
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
            if view == "launcher" {
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
    fn first_request_constructs_window() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(FakeCtor {
            construct_count: count.clone(),
            shown: shown.clone(),
        });
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
        let mut reg = WindowRegistry::new(FakeCtor {
            construct_count: count.clone(),
            shown: shown.clone(),
        });
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
    fn unknown_view_does_not_panic() {
        let count = Rc::new(Cell::new(0));
        let shown = Rc::new(Cell::new(false));
        let mut reg = WindowRegistry::new(FakeCtor {
            construct_count: count,
            shown,
        });
        reg.handle(WindowRequest::Open {
            view: "nope".into(),
            mode: WindowMode::Show,
        });
        // If we reach here without panic, the test passes.
    }
}
