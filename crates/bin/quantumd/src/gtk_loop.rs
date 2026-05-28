//! GTK main loop with integrated window registry.
//!
//! ## Threading model
//!
//! GTK4 is single-threaded: all `gtk4::*` and `webkit6::*` calls must happen
//! on the thread that called `app.run_*`. We make that the main thread.
//!
//! The Tokio multi-threaded runtime lives on a worker thread (see
//! `runtime::spawn_worker`). Window requests flow as follows:
//!
//! ```text
//! Tokio task (dispatcher) --send--> GtkWindowHost
//!                                       |
//!                              tokio::mpsc::Unbounded
//!                                       v
//!                            Tokio forwarder task
//!                                       |
//!                            std::sync::mpsc::Sender
//!                                       v
//!                          GTK idle_add_local on main
//!                                       |
//!                                       v
//!                              WindowRegistry::handle
//! ```
//!
//! The tokio mpsc receiver must be polled inside a tokio task because that's
//! where its wakers register. We can't poll it from `glib::MainContext::spawn_local`
//! — that won't drive Tokio's reactor. The std::sync::mpsc bridge into the GTK
//! main loop is `!Send` on the receiver but lives entirely on the GTK thread,
//! so that's fine.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gtk4::gio;
use gtk4::prelude::*;
use quantum_domain::ports::ThemeStore;
use quantum_ui::{IpcDispatcher, ManagedWindowConstructor, WindowRegistry, WindowRequest};
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedReceiver;

/// Run the GTK main loop with window registry.
pub fn run(
    app: &gtk4::Application,
    rx: UnboundedReceiver<WindowRequest>,
    dispatcher: Arc<dyn IpcDispatcher>,
    theme_store: Arc<dyn ThemeStore>,
    runtime: Handle,
) -> i32 {
    let rx = Rc::new(RefCell::new(Some(rx)));

    let dispatcher_for_activate = dispatcher.clone();
    let theme_store_for_activate = theme_store.clone();
    let theme_store_for_scheme = theme_store.clone();
    let rx_for_activate = rx.clone();

    // Hold a strong reference to keep the application alive even when there
    // are no open windows. `ApplicationHoldGuard` is RAII — dropping it
    // releases the hold and the GTK loop exits. The daemon is service-style,
    // so we keep the guard alive for the entire run.
    let hold_guard = Rc::new(RefCell::new(None::<gio::ApplicationHoldGuard>));
    let hold_for_activate = hold_guard.clone();

    app.connect_activate(move |app| {
        *hold_for_activate.borrow_mut() = Some(app.hold());

        // Register the quantum:// URI scheme on the default WebContext so
        // every WebView built from `WebContext::default()` can resolve theme
        // bundles. Must happen before any WebView load_uri call.
        quantum_ui::register_quantum_scheme_on_default(theme_store_for_scheme.clone());

        let ctor = ManagedWindowConstructor::new(
            app.clone(),
            dispatcher_for_activate.clone(),
            theme_store_for_activate.clone(),
            runtime.clone(),
        );
        let registry = Rc::new(RefCell::new(WindowRegistry::new(ctor)));

        let Some(rx) = rx_for_activate.borrow_mut().take() else {
            return;
        };
        let registry_for_loop = registry.clone();
        let (forward_tx, forward_rx) = std::sync::mpsc::channel::<quantum_ui::WindowRequest>();
        runtime.spawn(async move {
            let mut rx = rx;
            while let Some(req) = rx.recv().await {
                if forward_tx.send(req).is_err() {
                    break;
                }
            }
        });
        glib::idle_add_local(move || {
            while let Ok(req) = forward_rx.try_recv() {
                registry_for_loop.borrow_mut().handle(req);
            }
            glib::ControlFlow::Continue
        });
    });

    // Pass an empty argv to GTK so it doesn't try to parse the daemon's own
    // CLI flags (`--socket`, `--headless`, ...) and exit with "Unknown option".
    // The daemon owns its CLI; GTK only cares about its own loop.
    let exit_code = app.run_with_args::<&str>(&[]);
    // hold_guard drops on function return.
    drop(hold_guard);
    i32::from(exit_code)
}
