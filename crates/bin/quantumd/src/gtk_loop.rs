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
use quantum_domain::{EventEnvelope, WindowMode};
use quantum_ui::registry::WindowConstructor;
use quantum_ui::{
    IpcDispatcher, ManagedWindow, ManagedWindowConstructor, WindowRegistry, WindowRequest,
};
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedReceiver;

/// Run the GTK main loop with window registry.
///
/// `auto_show_bar` enables the per-monitor bar spawn. When true, after the
/// registry is wired up the activate handler schedules a glib task that, after
/// a 500 ms delay (matching the Tokio auto-show pattern in `main.rs`), enumerates
/// all connected monitors via `gdk::Display::default()` and dispatches one
/// `widgets/bar@<connector>` Open request per monitor. Monitor enumeration
/// must happen here because `gdk::Display::default()` is GTK-thread-only.
pub fn run(
    app: &gtk4::Application,
    rx: UnboundedReceiver<WindowRequest>,
    dispatcher: Arc<dyn IpcDispatcher>,
    theme_store: Arc<dyn ThemeStore>,
    runtime: Handle,
    event_tx: broadcast::Sender<EventEnvelope>,
    auto_show_bar: bool,
) -> i32 {
    let rx = Rc::new(RefCell::new(Some(rx)));

    let dispatcher_for_activate = dispatcher.clone();
    let theme_store_for_activate = theme_store.clone();
    let theme_store_for_scheme = theme_store.clone();
    let rx_for_activate = rx.clone();
    let event_tx_for_activate = event_tx.clone();

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
            let registry_for_seed = registry.clone();
            glib::MainContext::default().spawn_local(async move {
                // Brief delay so monitors are fully enumerated; matches the
                // 500 ms wait in main.rs's Tokio auto-show loop.
                glib::timeout_future(std::time::Duration::from_millis(500)).await;
                spawn_bar_per_monitor(&mut registry_for_seed.borrow_mut());
            });
        }
    });

    // Pass an empty argv to GTK so it doesn't try to parse the daemon's own
    // CLI flags (`--socket`, `--headless`, ...) and exit with "Unknown option".
    // The daemon owns its CLI; GTK only cares about its own loop.
    let exit_code = app.run_with_args::<&str>(&[]);
    // hold_guard drops on function return.
    drop(hold_guard);
    i32::from(exit_code)
}

/// Enumerate all currently-connected monitors and dispatch an Open
/// request for `widgets/bar@<connector>` per monitor. Skips monitors
/// without a connector name (virtual outputs, headless) with a warn
/// log so the operator can see them in the journal.
///
/// Must be called on the GTK main thread because `gdk::Display::default()`
/// is GTK-thread-only.
fn spawn_bar_per_monitor<C>(registry: &mut WindowRegistry<C>)
where
    C: WindowConstructor<Window = ManagedWindow>,
{
    let Some(display) = gdk::Display::default() else {
        tracing::warn!("no gdk::Display available; widgets/bar not auto-shown");
        return;
    };
    for monitor in display
        .monitors()
        .iter::<gdk::Monitor>()
        .filter_map(Result::ok)
    {
        match quantum_ui::windows::widget::monitor_name(&monitor) {
            Some(name) => {
                registry.handle(WindowRequest::Open {
                    view: format!("widgets/bar@{name}"),
                    mode: WindowMode::Show,
                });
            }
            None => tracing::warn!("skipping monitor without connector name"),
        }
    }
}
