//! GTK main loop with integrated window registry.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

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
    let rx_for_activate = rx.clone();

    app.connect_activate(move |app| {
        let ctor = ManagedWindowConstructor::new(
            app.clone(),
            dispatcher_for_activate.clone(),
            theme_store_for_activate.clone(),
            runtime.clone(),
        );
        let registry = Rc::new(RefCell::new(WindowRegistry::new(ctor)));

        // Drain channel on GLib loop.
        let Some(rx) = rx_for_activate.borrow_mut().take() else {
            return;
        };
        let registry_for_loop = registry.clone();
        glib::MainContext::default().spawn_local(async move {
            let mut rx = rx;
            while let Some(req) = rx.recv().await {
                registry_for_loop.borrow_mut().handle(req);
            }
        });
    });

    let exit_code = app.run();
    i32::from(exit_code)
}
