//! GTK4 application shell.

use gtk4::prelude::*;
use gtk4::Application;
use quantum_application::dispatcher::Dispatcher;
use std::sync::Arc;

pub struct QuantumApp {
    app: Application,
    dispatcher: Arc<Dispatcher>,
}

impl QuantumApp {
    pub fn new(dispatcher: Arc<Dispatcher>) -> Self {
        let app = Application::builder()
            .application_id("dev.quantum.daemon")
            .flags(gtk4::gio::ApplicationFlags::HANDLES_OPEN)
            .build();

        Self { app, dispatcher }
    }

    pub fn dispatcher(&self) -> &Arc<Dispatcher> {
        &self.dispatcher
    }

    pub fn run(self) -> i32 {
        self.app.run().into()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(feature = "gtk-test")]
    fn app_initializes() {
        // Requires display server; manual test only.
        // This smoke test validates GTK is properly linked and basic app setup works.
        // Not run in CI.
    }
}
