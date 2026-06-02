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
//!                      glib::MainContext::spawn_local task
//!                                       |
//!                                       v
//!                              WindowRegistry::handle
//! ```
//!
//! Tokio mpsc channels are executor-agnostic — their wakers are supplied by
//! whatever runtime polls the receiver. Polling from a `MainContext::spawn_local`
//! task means the GLib main context drives the receiver natively without any
//! busy-loop, and the receiver yields control back to the loop while waiting
//! for the next message. Only Tokio's IO and timer primitives (sockets, sleep)
//! require Tokio's reactor; channels do not.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gtk4::gdk;
use gtk4::gio;
use gtk4::prelude::*;
use quantum_domain::ports::ThemeStore;
use quantum_domain::EventEnvelope;
use quantum_ui::{IpcDispatcher, ManagedWindowConstructor, WindowRegistry, WindowRequest};
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Run the GTK main loop with window registry.
///
/// `auto_show_bar` enables per-monitor bar windows. When true, after the
/// registry is wired up the activate handler installs a `BarMultiplexer` on
/// `gdk::Display::default()`. The multiplexer performs an initial sync against
/// the currently-connected monitors and stays subscribed to the display's
/// `items-changed` signal so hot-plugged monitors get a bar window and removed
/// monitors have theirs closed. The returned handle is stashed for the
/// lifetime of the application so the signal stays connected.
#[allow(clippy::too_many_arguments)]
pub fn run(
    app: &gtk4::Application,
    rx: UnboundedReceiver<WindowRequest>,
    dispatcher: Arc<dyn IpcDispatcher>,
    theme_store: Arc<dyn ThemeStore>,
    runtime: Handle,
    event_tx: broadcast::Sender<EventEnvelope>,
    window_request_tx: UnboundedSender<WindowRequest>,
    auto_show_bar: bool,
) -> i32 {
    let rx = Rc::new(RefCell::new(Some(rx)));

    let dispatcher_for_activate = dispatcher.clone();
    let theme_store_for_activate = theme_store.clone();
    let theme_store_for_scheme = theme_store.clone();
    let rx_for_activate = rx.clone();
    let event_tx_for_activate = event_tx.clone();
    let window_request_tx_for_activate = window_request_tx.clone();

    // Hold a strong reference to keep the application alive even when there
    // are no open windows. `ApplicationHoldGuard` is RAII — dropping it
    // releases the hold and the GTK loop exits. The daemon is service-style,
    // so we keep the guard alive for the entire run.
    let hold_guard = Rc::new(RefCell::new(None::<gio::ApplicationHoldGuard>));
    let hold_for_activate = hold_guard.clone();

    // Owns the `BarMultiplexer` handle for the lifetime of the application.
    // Dropping the handle disconnects the monitor `items-changed` signal,
    // so we keep it alive until after `app.run_with_args` returns.
    let bar_multiplexer_handle: Rc<RefCell<Option<quantum_ui::BarMultiplexerHandle>>> =
        Rc::new(RefCell::new(None));
    let bar_multiplexer_handle_for_activate = bar_multiplexer_handle.clone();

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
            event_tx_for_activate.clone(),
        );
        let registry = Rc::new(RefCell::new(WindowRegistry::new(ctor)));

        let Some(mut rx) = rx_for_activate.borrow_mut().take() else {
            return;
        };
        let registry_for_loop = registry.clone();
        glib::MainContext::default().spawn_local(async move {
            while let Some(req) = rx.recv().await {
                registry_for_loop.borrow_mut().handle(req);
            }
        });

        if auto_show_bar {
            let Some(display) = gdk::Display::default() else {
                tracing::warn!("no gdk::Display available; widgets/bar not auto-shown");
                return;
            };
            let handle = quantum_ui::BarMultiplexer::install(
                &display,
                window_request_tx_for_activate.clone(),
            );
            *bar_multiplexer_handle_for_activate.borrow_mut() = Some(handle);
        }
    });

    // Pass an empty argv to GTK so it doesn't try to parse the daemon's own
    // CLI flags (`--socket`, `--headless`, ...) and exit with "Unknown option".
    // The daemon owns its CLI; GTK only cares about its own loop.
    let exit_code = app.run_with_args::<&str>(&[]);
    // Release our local strong references. By the time GTK has exited
    // no signals will fire anyway, and the activate closure still
    // holds clones of both `Rc`s, so the actual handle / hold drop
    // happens when the `gtk4::Application` is itself dropped at end
    // of `main`. These calls just shed the local refs eagerly.
    drop(bar_multiplexer_handle);
    drop(hold_guard);
    i32::from(exit_code)
}
