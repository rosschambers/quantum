//! `ViewMultiplexer` — spawns and tears down per-monitor windows for a set
//! of views in response to `gdk::Display::monitors()` changes.
//!
//! The multiplexer is GTK-main-thread-only. It holds an
//! `Rc<RefCell<ViewMultiplexer>>` (NOT `Arc<Mutex<_>>`) to match the
//! pattern established in `gtk_loop.rs`. Holding the returned
//! `ViewMultiplexerHandle` keeps the `items-changed` signal connected;
//! dropping the handle disconnects it.
//!
//! Each managed view is identified by its canonical name
//! (`plugin/<plugin>/<view>`). For every connected monitor and every
//! canonical view in the managed set, the multiplexer emits an `Open`
//! request keyed `<canonical-view>@<monitor>`. Emitting the canonical
//! name directly avoids the registry's alias-deprecation warning on the
//! auto-show path.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::gdk::prelude::MonitorExt;
use gtk4::gio;
use gtk4::prelude::{Cast, DisplayExt, ListModelExt, ListModelExtManual, ObjectExt};
use tokio::sync::mpsc::UnboundedSender;

use crate::messages::WindowRequest;
use quantum_domain::WindowMode;

/// Split an iterator of optional monitor names into the distinct set of
/// present names and the count of entries that had no name yet (monitors
/// whose Wayland connector has not arrived). Pure so the readiness logic is
/// unit-tested without a live `gdk::Display`.
fn partition_monitor_names(
    names: impl IntoIterator<Item = Option<String>>,
) -> (HashSet<String>, usize) {
    let mut present = HashSet::new();
    let mut unready = 0usize;
    for maybe_name in names {
        match maybe_name {
            Some(name) => {
                present.insert(name);
            }
            None => unready += 1,
        }
    }
    (present, unready)
}

/// Connect a `notify::connector` handler to `monitor` when it has no
/// connector name yet, so the multiplexer re-syncs (and spawns the deferred
/// per-monitor window) the moment the Wayland connector arrives. Monitors
/// that already have a connector need no handler — `sync` already accounts
/// for them. Idempotent at the `sync` level: even if the handler fires more
/// than once, `diff_emit` only opens windows that are not already tracked.
fn wire_connector_arrival(
    monitor: &gdk::Monitor,
    multiplexer: &Rc<RefCell<ViewMultiplexer>>,
    monitors: &gio::ListModel,
) {
    if crate::windows::widget::monitor_name(monitor).is_some() {
        return;
    }
    let multiplexer_for_notify = Rc::clone(multiplexer);
    let monitors_for_notify = monitors.clone();
    monitor.connect_connector_notify(move |_monitor| {
        multiplexer_for_notify
            .borrow_mut()
            .sync(&monitors_for_notify);
    });
    tracing::info!("monitor has no connector name yet; subscribed to notify::connector to spawn its bar once the name arrives");
}

/// Tracks which `<canonical-view>@<monitor>` windows are currently open and
/// emits `WindowRequest`s to keep that set in sync with the live monitor
/// list. The diff is intentionally pure (`diff_emit`) so the bulk of the
/// logic can be tested without a real `gdk::Display`.
pub struct ViewMultiplexer {
    /// Canonical view names (`plugin/<plugin>/<view>`) this multiplexer
    /// manages. For each connected monitor, one window per view is opened.
    views: Vec<String>,
    /// `<canonical-view>@<monitor>` keys that currently have a window open.
    /// Private so external code cannot mutate it out from under the
    /// `items-changed` signal handler.
    active_views: HashSet<String>,
    /// Channel back to the `GtkWindowHost` receiver in
    /// `src/binaries/quantumd/src/main.rs`.
    window_request_tx: UnboundedSender<WindowRequest>,
}

impl ViewMultiplexer {
    /// Compute the full set of `<canonical-view>@<monitor>` keys that should
    /// be open given the live monitor name set, then diff against the
    /// currently-open set. Emit `Open` for keys that should be open but are
    /// not yet tracked, emit `Close` for tracked keys that should no longer
    /// be open, and update `active_views` accordingly. Send errors are
    /// ignored because the receiver dropping means the daemon is already
    /// shutting down.
    pub fn diff_emit(&mut self, monitors: HashSet<String>) {
        let desired: HashSet<String> = self
            .views
            .iter()
            .flat_map(|view| {
                monitors
                    .iter()
                    .map(move |monitor| format!("{view}@{monitor}"))
            })
            .collect();

        let to_add: Vec<String> = desired.difference(&self.active_views).cloned().collect();
        for key in to_add {
            tracing::info!("opening per-monitor view window: {key}");
            let _ = self.window_request_tx.send(WindowRequest::Open {
                view: key.clone(),
                mode: WindowMode::Show,
                args: None,
            });
            self.active_views.insert(key);
        }

        let to_remove: Vec<String> = self.active_views.difference(&desired).cloned().collect();
        for key in to_remove {
            tracing::info!("closing per-monitor view window: {key}");
            let _ = self
                .window_request_tx
                .send(WindowRequest::Close { view: key.clone() });
            self.active_views.remove(&key);
        }
    }

    /// Extracts the current monitor name set from a `gio::ListModel`
    /// of `gdk::Monitor` and runs the diff. Used both for the initial
    /// sync and on every `items-changed` signal.
    fn sync(&mut self, monitors: &gio::ListModel) {
        let names: Vec<Option<String>> = monitors
            .iter::<gdk::Monitor>()
            .filter_map(Result::ok)
            .enumerate()
            .map(|(index, monitor)| {
                let name = crate::windows::widget::monitor_name(&monitor);
                if name.is_none() {
                    tracing::warn!(
                        "monitor at index {index} present without connector name yet; \
                         deferring bar spawn until notify::connector"
                    );
                }
                name
            })
            .collect();
        let (present, unready) = partition_monitor_names(names);
        if unready > 0 {
            tracing::warn!("{unready} monitor(s) not yet ready; bars for them are deferred");
        }
        self.diff_emit(present);
    }

    /// Installs the multiplexer on a `gdk::Display`. `views` is the set of
    /// canonical view names to manage per monitor. Runs an initial sync
    /// against `display.monitors()` and connects to its `items-changed`
    /// signal. Returns a handle that owns the signal connection — drop the
    /// handle to disconnect.
    ///
    /// Calling this more than once on the same `Display` will produce
    /// duplicate `WindowRequest`s and is not supported. The daemon
    /// installs exactly one multiplexer for its entire lifetime.
    pub fn install(
        display: &gdk::Display,
        window_request_tx: UnboundedSender<WindowRequest>,
        views: Vec<String>,
    ) -> ViewMultiplexerHandle {
        let multiplexer = Rc::new(RefCell::new(ViewMultiplexer {
            views,
            active_views: HashSet::new(),
            window_request_tx,
        }));

        let monitors = display.monitors();

        // Initial sync against whatever monitors are already present.
        multiplexer.borrow_mut().sync(&monitors);

        // Wire connector-arrival for any monitor already present without a
        // connector name, so a startup race does not leave a bar unspawned.
        for monitor in monitors.iter::<gdk::Monitor>().filter_map(Result::ok) {
            wire_connector_arrival(&monitor, &multiplexer, &monitors);
        }

        // Subscribe to live updates. The closure captures an
        // `Rc<RefCell<_>>` clone so the multiplexer stays alive as
        // long as the signal is connected.
        let multiplexer_for_signal = Rc::clone(&multiplexer);
        let signal_id =
            monitors.connect_items_changed(move |list_model, position, _removed, added| {
                multiplexer_for_signal.borrow_mut().sync(list_model);
                // A monitor can enter the list before its Wayland connector
                // name arrives; wire each newly added monitor so its bar
                // still spawns once the name is populated.
                for offset in 0..added {
                    if let Some(object) = list_model.item(position + offset) {
                        if let Ok(monitor) = object.downcast::<gdk::Monitor>() {
                            wire_connector_arrival(&monitor, &multiplexer_for_signal, list_model);
                        }
                    }
                }
            });

        ViewMultiplexerHandle {
            display: display.clone(),
            signal_id: Some(signal_id),
            _multiplexer: multiplexer,
        }
    }
}

/// Owns the `items-changed` signal connection and the
/// `Rc<RefCell<ViewMultiplexer>>`. Drop disconnects the `items-changed`
/// signal so the multiplexer stops reacting to monitor add/remove.
///
/// Note: this does NOT disconnect the per-monitor `notify::connector`
/// handlers wired by `wire_connector_arrival` for monitors that appeared
/// without a connector name. Each of those handlers holds its own
/// `Rc<RefCell<ViewMultiplexer>>` clone, so the multiplexer is kept alive
/// (and a late connector arrival still re-runs `sync`) for as long as those
/// monitors remain connected, even after this handle is dropped. That is
/// acceptable because the daemon installs exactly one multiplexer for its
/// entire lifetime and never drops the handle early; a connector handler is
/// released when GDK drops its monitor (on disconnect). Do not rely on
/// handle drop to fully quiesce the multiplexer.
pub struct ViewMultiplexerHandle {
    display: gdk::Display,
    signal_id: Option<glib::SignalHandlerId>,
    _multiplexer: Rc<RefCell<ViewMultiplexer>>,
}

impl Drop for ViewMultiplexerHandle {
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

    /// Build a multiplexer with the given managed views and pre-seeded
    /// active key set, returning it alongside the request receiver.
    fn make_multiplexer(
        views: Vec<&str>,
        active: &[&str],
    ) -> (
        ViewMultiplexer,
        tokio::sync::mpsc::UnboundedReceiver<WindowRequest>,
    ) {
        let (tx, rx) = unbounded_channel();
        let multiplexer = ViewMultiplexer {
            views: views.into_iter().map(str::to_string).collect(),
            active_views: active.iter().map(|s| s.to_string()).collect(),
            window_request_tx: tx,
        };
        (multiplexer, rx)
    }

    fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<WindowRequest>) -> Vec<WindowRequest> {
        let mut messages = Vec::new();
        while let Ok(message) = rx.try_recv() {
            messages.push(message);
        }
        messages
    }

    #[test]
    fn single_view_two_monitors_emits_two_canonical_opens() {
        let (mut multiplexer, mut rx) = make_multiplexer(vec!["plugin/bar/bar"], &[]);

        let mut monitors = HashSet::new();
        monitors.insert("DP-1".to_string());
        monitors.insert("HDMI-A-1".to_string());

        multiplexer.diff_emit(monitors);

        let mut opens: Vec<String> = Vec::new();
        for message in drain(&mut rx) {
            match message {
                WindowRequest::Open {
                    view,
                    mode,
                    args: _,
                } => {
                    assert!(matches!(mode, WindowMode::Show));
                    opens.push(view);
                }
                other => panic!("expected Open, got {other:?}"),
            }
        }

        assert_eq!(opens.len(), 2);
        assert!(opens.contains(&"plugin/bar/bar@DP-1".to_string()));
        assert!(opens.contains(&"plugin/bar/bar@HDMI-A-1".to_string()));
        assert_eq!(multiplexer.active_views.len(), 2);
        assert!(multiplexer.active_views.contains("plugin/bar/bar@DP-1"));
        assert!(multiplexer.active_views.contains("plugin/bar/bar@HDMI-A-1"));
    }

    #[test]
    fn single_view_added_monitor_emits_one_open() {
        let (mut multiplexer, mut rx) =
            make_multiplexer(vec!["plugin/bar/bar"], &["plugin/bar/bar@DP-1"]);

        let mut monitors = HashSet::new();
        monitors.insert("DP-1".to_string());
        monitors.insert("HDMI-A-1".to_string());

        multiplexer.diff_emit(monitors);

        let messages = drain(&mut rx);
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            WindowRequest::Open {
                view,
                mode,
                args: _,
            } => {
                assert_eq!(view, "plugin/bar/bar@HDMI-A-1");
                assert!(matches!(mode, WindowMode::Show));
            }
            other => panic!("expected Open, got {other:?}"),
        }
        assert!(multiplexer.active_views.contains("plugin/bar/bar@HDMI-A-1"));
        assert!(multiplexer.active_views.contains("plugin/bar/bar@DP-1"));
    }

    #[test]
    fn single_view_removed_monitor_emits_one_close() {
        let (mut multiplexer, mut rx) = make_multiplexer(
            vec!["plugin/bar/bar"],
            &["plugin/bar/bar@DP-1", "plugin/bar/bar@HDMI-A-1"],
        );

        let mut monitors = HashSet::new();
        monitors.insert("DP-1".to_string());

        multiplexer.diff_emit(monitors);

        let messages = drain(&mut rx);
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            WindowRequest::Close { view } => {
                assert_eq!(view, "plugin/bar/bar@HDMI-A-1");
            }
            other => panic!("expected Close, got {other:?}"),
        }
        assert!(!multiplexer.active_views.contains("plugin/bar/bar@HDMI-A-1"));
        assert!(multiplexer.active_views.contains("plugin/bar/bar@DP-1"));
    }

    #[test]
    fn two_views_two_monitors_emits_four_opens() {
        let (mut multiplexer, mut rx) =
            make_multiplexer(vec!["plugin/bar/bar", "plugin/clock/clock"], &[]);

        let mut monitors = HashSet::new();
        monitors.insert("DP-1".to_string());
        monitors.insert("HDMI-A-1".to_string());

        multiplexer.diff_emit(monitors);

        let mut opens: Vec<String> = Vec::new();
        for message in drain(&mut rx) {
            match message {
                WindowRequest::Open {
                    view,
                    mode,
                    args: _,
                } => {
                    assert!(matches!(mode, WindowMode::Show));
                    opens.push(view);
                }
                other => panic!("expected Open, got {other:?}"),
            }
        }

        assert_eq!(opens.len(), 4);
        for expected in [
            "plugin/bar/bar@DP-1",
            "plugin/bar/bar@HDMI-A-1",
            "plugin/clock/clock@DP-1",
            "plugin/clock/clock@HDMI-A-1",
        ] {
            assert!(
                opens.contains(&expected.to_string()),
                "missing open for {expected}"
            );
        }
        assert_eq!(multiplexer.active_views.len(), 4);
    }

    #[test]
    fn two_views_removed_monitor_closes_both_views_for_that_monitor() {
        let (mut multiplexer, mut rx) = make_multiplexer(
            vec!["plugin/bar/bar", "plugin/clock/clock"],
            &[
                "plugin/bar/bar@DP-1",
                "plugin/bar/bar@HDMI-A-1",
                "plugin/clock/clock@DP-1",
                "plugin/clock/clock@HDMI-A-1",
            ],
        );

        let mut monitors = HashSet::new();
        monitors.insert("DP-1".to_string());

        multiplexer.diff_emit(monitors);

        let mut closes: Vec<String> = Vec::new();
        for message in drain(&mut rx) {
            match message {
                WindowRequest::Close { view } => closes.push(view),
                other => panic!("expected Close, got {other:?}"),
            }
        }

        assert_eq!(closes.len(), 2);
        assert!(closes.contains(&"plugin/bar/bar@HDMI-A-1".to_string()));
        assert!(closes.contains(&"plugin/clock/clock@HDMI-A-1".to_string()));
        assert_eq!(multiplexer.active_views.len(), 2);
        assert!(multiplexer.active_views.contains("plugin/bar/bar@DP-1"));
        assert!(multiplexer.active_views.contains("plugin/clock/clock@DP-1"));
    }

    #[test]
    fn partition_reports_unready_count() {
        let (names, unready) = partition_monitor_names(vec![
            Some("DP-1".to_string()),
            None,
            Some("HDMI-A-1".to_string()),
        ]);
        assert_eq!(unready, 1);
        assert_eq!(names.len(), 2);
        assert!(names.contains("DP-1"));
        assert!(names.contains("HDMI-A-1"));
    }

    #[test]
    fn partition_all_ready_has_zero_unready() {
        let (names, unready) =
            partition_monitor_names(vec![Some("DP-1".to_string()), Some("DP-2".to_string())]);
        assert_eq!(unready, 0);
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn partition_collapses_duplicate_names() {
        let (names, unready) =
            partition_monitor_names(vec![Some("DP-1".to_string()), Some("DP-1".to_string())]);
        assert_eq!(unready, 0);
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn empty_view_list_emits_nothing() {
        let (mut multiplexer, mut rx) = make_multiplexer(vec![], &[]);

        let mut monitors = HashSet::new();
        monitors.insert("DP-1".to_string());

        multiplexer.diff_emit(monitors);

        assert!(drain(&mut rx).is_empty());
        assert!(multiplexer.active_views.is_empty());
    }
}
