//! `BarMultiplexer` — spawns and tears down per-monitor bar windows in
//! response to `gdk::Display::monitors()` changes.
//!
//! The multiplexer is GTK-main-thread-only. It holds an
//! `Rc<RefCell<BarMultiplexer>>` (NOT `Arc<Mutex<_>>`) to match the
//! pattern established in `gtk_loop.rs`. Holding the returned
//! `BarMultiplexerHandle` keeps the `items-changed` signal connected;
//! dropping the handle disconnects it.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::gio;
use gtk4::prelude::{Cast, DisplayExt, ListModelExt, ListModelExtManual, ObjectExt};
use tokio::sync::mpsc::UnboundedSender;

use crate::messages::WindowRequest;
use quantum_domain::WindowMode;

/// Tracks which monitors currently have a bar window open and emits
/// `WindowRequest`s to keep that set in sync with the live monitor
/// list. The diff is intentionally pure (`diff_emit`) so the bulk of
/// the logic can be tested without a real `gdk::Display`.
pub struct BarMultiplexer {
    /// Monitor connector names (e.g. `"DP-1"`) that currently have a
    /// `widgets/bar@<name>` window open.
    pub active_bars: HashSet<String>,
    /// Channel back to the `GtkWindowHost` receiver in
    /// `crates/bin/quantumd/src/main.rs`.
    pub window_request_tx: UnboundedSender<WindowRequest>,
}

impl BarMultiplexer {
    /// Pure diff: emit `Open` for names in `current` that are not yet
    /// tracked, emit `Close` for names that are tracked but no longer
    /// present, and update `active_bars` accordingly. Send errors are
    /// ignored because the receiver dropping means the daemon is
    /// already shutting down.
    pub fn diff_emit(&mut self, current: HashSet<String>) {
        let to_add: Vec<String> = current.difference(&self.active_bars).cloned().collect();
        for name in to_add {
            tracing::info!("monitor connected: {name}, spawning bar");
            let _ = self.window_request_tx.send(WindowRequest::Open {
                view: format!("widgets/bar@{name}"),
                mode: WindowMode::Show,
            });
            self.active_bars.insert(name);
        }

        let to_remove: Vec<String> = self.active_bars.difference(&current).cloned().collect();
        for name in to_remove {
            tracing::info!("monitor disconnected: {name}, tearing down bar");
            let _ = self.window_request_tx.send(WindowRequest::Close {
                view: format!("widgets/bar@{name}"),
            });
            self.active_bars.remove(&name);
        }
    }

    /// Extracts the current monitor name set from a `gio::ListModel`
    /// of `gdk::Monitor` and runs the diff. Used both for the initial
    /// sync and on every `items-changed` signal.
    fn sync(&mut self, monitors: &gio::ListModel) {
        let current: HashSet<String> = monitors
            .iter::<gdk::Monitor>()
            .filter_map(Result::ok)
            .filter_map(|monitor| crate::windows::widget::monitor_name(&monitor))
            .collect();
        self.diff_emit(current);
    }

    /// Installs the multiplexer on a `gdk::Display`. Runs an initial
    /// sync against `display.monitors()` and connects to its
    /// `items-changed` signal. Returns a handle that owns the signal
    /// connection — drop the handle to disconnect.
    pub fn install(
        display: &gdk::Display,
        tx: UnboundedSender<WindowRequest>,
    ) -> BarMultiplexerHandle {
        let multiplexer = Rc::new(RefCell::new(BarMultiplexer {
            active_bars: HashSet::new(),
            window_request_tx: tx,
        }));

        let monitors = display.monitors();

        // Initial sync against whatever monitors are already present.
        multiplexer.borrow_mut().sync(&monitors);

        // Subscribe to live updates. The closure captures an
        // `Rc<RefCell<_>>` clone so the multiplexer stays alive as
        // long as the signal is connected.
        let multiplexer_for_signal = Rc::clone(&multiplexer);
        let signal_id =
            monitors.connect_items_changed(move |list_model, _position, _removed, _added| {
                multiplexer_for_signal.borrow_mut().sync(list_model);
            });

        BarMultiplexerHandle {
            display: display.clone(),
            signal_id: Some(signal_id),
            _multiplexer: multiplexer,
        }
    }
}

/// Owns the `items-changed` signal connection and the
/// `Rc<RefCell<BarMultiplexer>>`. Drop disconnects the signal so the
/// multiplexer stops reacting to monitor changes.
pub struct BarMultiplexerHandle {
    display: gdk::Display,
    signal_id: Option<glib::SignalHandlerId>,
    _multiplexer: Rc<RefCell<BarMultiplexer>>,
}

impl Drop for BarMultiplexerHandle {
    fn drop(&mut self) {
        if let Some(signal_id) = self.signal_id.take() {
            let monitors = self.display.monitors();
            // `gio::ListModel` is an interface; the concrete type
            // implements `ObjectExt::disconnect`. Cast for the
            // disconnect call.
            let object: glib::Object = monitors.upcast();
            object.disconnect(signal_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn diff_emit_initial_empty_with_two_monitors_emits_two_opens() {
        let (tx, mut rx) = unbounded_channel();
        let mut multiplexer = BarMultiplexer {
            active_bars: HashSet::new(),
            window_request_tx: tx,
        };

        let mut current = HashSet::new();
        current.insert("DP-1".to_string());
        current.insert("HDMI-A-1".to_string());

        multiplexer.diff_emit(current);

        let mut opens: Vec<String> = Vec::new();
        while let Ok(message) = rx.try_recv() {
            match message {
                WindowRequest::Open { view, mode } => {
                    assert!(matches!(mode, WindowMode::Show));
                    opens.push(view);
                }
                other => panic!("expected Open, got {other:?}"),
            }
        }

        assert_eq!(opens.len(), 2);
        assert!(opens.contains(&"widgets/bar@DP-1".to_string()));
        assert!(opens.contains(&"widgets/bar@HDMI-A-1".to_string()));
        assert_eq!(multiplexer.active_bars.len(), 2);
        assert!(multiplexer.active_bars.contains("DP-1"));
        assert!(multiplexer.active_bars.contains("HDMI-A-1"));
    }

    #[test]
    fn diff_emit_added_monitor_emits_one_open() {
        let (tx, mut rx) = unbounded_channel();
        let mut active_bars = HashSet::new();
        active_bars.insert("DP-1".to_string());
        let mut multiplexer = BarMultiplexer {
            active_bars,
            window_request_tx: tx,
        };

        let mut current = HashSet::new();
        current.insert("DP-1".to_string());
        current.insert("HDMI-A-1".to_string());

        multiplexer.diff_emit(current);

        let mut messages: Vec<WindowRequest> = Vec::new();
        while let Ok(message) = rx.try_recv() {
            messages.push(message);
        }

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            WindowRequest::Open { view, mode } => {
                assert_eq!(view, "widgets/bar@HDMI-A-1");
                assert!(matches!(mode, WindowMode::Show));
            }
            other => panic!("expected Open, got {other:?}"),
        }
        assert!(multiplexer.active_bars.contains("HDMI-A-1"));
        assert!(multiplexer.active_bars.contains("DP-1"));
    }

    #[test]
    fn diff_emit_removed_monitor_emits_one_close() {
        let (tx, mut rx) = unbounded_channel();
        let mut active_bars = HashSet::new();
        active_bars.insert("DP-1".to_string());
        active_bars.insert("HDMI-A-1".to_string());
        let mut multiplexer = BarMultiplexer {
            active_bars,
            window_request_tx: tx,
        };

        let mut current = HashSet::new();
        current.insert("DP-1".to_string());

        multiplexer.diff_emit(current);

        let mut messages: Vec<WindowRequest> = Vec::new();
        while let Ok(message) = rx.try_recv() {
            messages.push(message);
        }

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            WindowRequest::Close { view } => {
                assert_eq!(view, "widgets/bar@HDMI-A-1");
            }
            other => panic!("expected Close, got {other:?}"),
        }
        assert!(!multiplexer.active_bars.contains("HDMI-A-1"));
        assert!(multiplexer.active_bars.contains("DP-1"));
    }
}
