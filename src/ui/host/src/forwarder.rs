//! Broadcast-event forwarding from the daemon's event bus into one webview.
//!
//! Every quantum window pushes the broadcast events its page subscribed to
//! into the page as `window.__quantum_notify(channel, payload)` calls. A
//! `webkit6::WebView` is not `Send`, so the work is split across two tasks:
//!
//! - a Tokio task receives from the broadcast channel, filters on the
//!   webview's subscription set, and sends pre-serialized JavaScript strings
//!   down an unbounded mpsc channel;
//! - a GLib `spawn_local` task drains that channel, coalesces whatever is
//!   already queued into one batch per main-loop turn, and hands the batch to
//!   a sink (in production, `evaluate_javascript` on a weakly held webview).
//!
//! ## Lifetime (2026-11-14)
//!
//! Before this module, each window built these two tasks inline, and they
//! kept each other alive forever: the Tokio task exited only when sending
//! failed (the receiver dropped) and the GLib task exited only when the
//! channel closed (the sender dropped), while it also held a strong clone of
//! the webview. Destroying a window therefore never released its webview.
//! Every per-monitor bar and timers window rebuilt on a monitor flap, and
//! every toast rebuilt on a monitor change, stayed resident: it kept its
//! renderer, kept evaluating every event, and kept its crash handler. On x1
//! that was 24 orphaned toasts, several bars and timers, and the cgroup
//! pinned at its 5 GB cap.
//!
//! [`EventForwarder`] owns the Tokio task. Stopping or dropping it aborts the
//! task, which drops the sender, which ends the GLib task and releases its
//! sink. The sink returns `false` once its target is gone, which ends the
//! GLib task from the other side as well.

use crate::bridge::json_to_js_expression;
use crate::subscriptions::WebviewSubscriptions;
use gtk4::glib;
use quantum_domain::EventEnvelope;
use tokio::runtime::Handle;
use tokio::sync::broadcast;

/// The channel whose arrival also pushes freshly resolved theme tokens.
const THEME_RELOADED_CHANNEL: &str = "theme.reloaded";

/// Owns one window's event-forwarding tasks. Stop it (or drop it) when the
/// window is destroyed so the webview, its subscription, and its renderer are
/// released.
pub struct EventForwarder {
    task: tokio::task::JoinHandle<()>,
}

impl EventForwarder {
    /// Stop forwarding: abort the broadcast task, which closes the channel and
    /// ends the GLib task, releasing the sink and the webview it refers to.
    pub fn stop(&self) {
        self.task.abort();
    }
}

impl Drop for EventForwarder {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Start forwarding broadcast events into a page.
///
/// `label` names the window kind in the lag warning. `theme_push` builds the
/// JavaScript that pushes the current theme tokens; it runs on the Tokio side
/// whenever `theme.reloaded` arrives. `sink` runs on `context`, receives one
/// coalesced batch of JavaScript statements at a time, and returns `false`
/// once its target is gone, which ends forwarding.
pub fn spawn_event_forwarder<ThemePush, Sink>(
    runtime: &Handle,
    context: &glib::MainContext,
    mut event_receiver: broadcast::Receiver<EventEnvelope>,
    subscriptions: WebviewSubscriptions,
    label: &'static str,
    theme_push: ThemePush,
    mut sink: Sink,
) -> EventForwarder
where
    ThemePush: Fn() -> String + Send + 'static,
    Sink: FnMut(&str) -> bool + 'static,
{
    let (script_sender, mut script_receiver) = tokio::sync::mpsc::unbounded_channel::<String>();

    let task = runtime.spawn(async move {
        loop {
            match event_receiver.recv().await {
                Ok(envelope) => {
                    // Filter server-side: skip channels this webview never
                    // subscribed to. The lock is held only for the pure
                    // decision, never across an await. A poisoned lock fails
                    // open so a panic elsewhere can never silently freeze this
                    // webview's live updates.
                    let forward = match subscriptions.lock() {
                        Ok(guard) => {
                            crate::subscriptions::should_forward(&envelope.channel, &guard)
                        }
                        Err(_) => true,
                    };
                    if !forward {
                        continue;
                    }
                    let channel = serde_json::to_string(&envelope.channel)
                        .unwrap_or_else(|_| "\"\"".into());
                    // `envelope.payload` is raw JSON text; `.get()` returns it
                    // without re-serialization, ready to inline. The guard
                    // covers events that arrive before the page's client has
                    // installed `window.__quantum_notify`.
                    let payload = envelope.payload.get();
                    let raw = format!(
                        "if (typeof window.__quantum_notify === 'function') {{ window.__quantum_notify({channel}, {payload}); }}"
                    );
                    if script_sender.send(json_to_js_expression(&raw)).is_err() {
                        break;
                    }
                    // On a theme reload, also push the freshly resolved tokens
                    // into the live `#quantum-tokens` stylesheet so the page
                    // recolors without a reload.
                    if envelope.channel == THEME_RELOADED_CHANNEL
                        && script_sender.send(theme_push()).is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("{label} notify subscription lagged: {skipped} events dropped");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    context.spawn_local(async move {
        // Coalesce per main-loop turn: when the first item arrives, drain any
        // others already queued and submit a single batch. Each item is a
        // self-contained statement, so joining with `;` is syntactically safe.
        // This cuts the JIT entry rate by the burst size when providers fan out.
        while let Some(first) = script_receiver.recv().await {
            let mut batch = first;
            while let Ok(more) = script_receiver.try_recv() {
                batch.push(';');
                batch.push_str(&more);
            }
            if !sink(&batch) {
                break;
            }
        }
    });

    EventForwarder { task }
}

/// A sink that evaluates each batch in `webview` while it is alive, holding it
/// only weakly so forwarding never keeps a destroyed window's webview resident.
pub fn webview_sink(webview: &webkit6::WebView) -> impl FnMut(&str) -> bool + 'static {
    let weak = glib::object::ObjectExt::downgrade(webview);
    move |batch: &str| {
        let Some(webview) = weak.upgrade() else {
            return false;
        };
        webkit6::prelude::WebViewExt::evaluate_javascript(
            &webview,
            batch,
            None,
            None,
            None::<&gtk4::gio::Cancellable>,
            |_| {},
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::value::RawValue;
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn envelope(channel: &str, payload: &str) -> EventEnvelope {
        EventEnvelope {
            channel: channel.to_string(),
            payload: RawValue::from_string(payload.to_string()).unwrap(),
        }
    }

    fn subscribed(channels: &[&str]) -> WebviewSubscriptions {
        Arc::new(Mutex::new(
            channels
                .iter()
                .map(|channel| channel.to_string())
                .collect::<HashSet<_>>(),
        ))
    }

    /// Iterate `context` until `done` holds, failing after two seconds.
    fn pump_until(context: &glib::MainContext, mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            while context.iteration(false) {}
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn forwards_only_subscribed_channels() {
        let runtime = runtime();
        let context = glib::MainContext::new();
        let _owner = context.acquire().unwrap();
        let (event_sender, _) = broadcast::channel::<EventEnvelope>(16);
        let delivered = Rc::new(RefCell::new(Vec::<String>::new()));
        let target = delivered.clone();
        let _forwarder = spawn_event_forwarder(
            runtime.handle(),
            &context,
            event_sender.subscribe(),
            subscribed(&["clock.event"]),
            "test",
            String::new,
            move |batch| {
                target.borrow_mut().push(batch.to_string());
                true
            },
        );

        event_sender.send(envelope("other.event", "0")).unwrap();
        event_sender.send(envelope("clock.event", "7")).unwrap();

        assert!(pump_until(&context, || !delivered.borrow().is_empty()));
        let joined = delivered.borrow().join(";");
        assert!(joined.contains("\"clock.event\", 7"), "got: {joined}");
        assert!(!joined.contains("other.event"), "got: {joined}");
    }

    #[test]
    fn stopping_releases_the_subscription_and_the_sink() {
        // Regression: a destroyed window's forwarder tasks kept each other
        // alive forever, so its webview (held by the sink) and its broadcast
        // subscription were never released.
        let runtime = runtime();
        let context = glib::MainContext::new();
        let _owner = context.acquire().unwrap();
        let (event_sender, _) = broadcast::channel::<EventEnvelope>(16);
        let delivered = Rc::new(RefCell::new(Vec::<String>::new()));
        let target = delivered.clone();
        let forwarder = spawn_event_forwarder(
            runtime.handle(),
            &context,
            event_sender.subscribe(),
            subscribed(&["clock.event"]),
            "test",
            String::new,
            move |batch| {
                target.borrow_mut().push(batch.to_string());
                true
            },
        );
        event_sender.send(envelope("clock.event", "1")).unwrap();
        assert!(pump_until(&context, || !delivered.borrow().is_empty()));
        assert_eq!(event_sender.receiver_count(), 1);

        forwarder.stop();

        assert!(
            pump_until(&context, || event_sender.receiver_count() == 0),
            "the broadcast subscription must be released after stop"
        );
        assert!(
            pump_until(&context, || Rc::strong_count(&delivered) == 1),
            "the sink (and the webview it refers to) must be released after stop"
        );
    }

    #[test]
    fn dropping_the_forwarder_stops_it() {
        let runtime = runtime();
        let context = glib::MainContext::new();
        let _owner = context.acquire().unwrap();
        let (event_sender, _) = broadcast::channel::<EventEnvelope>(16);
        let forwarder = spawn_event_forwarder(
            runtime.handle(),
            &context,
            event_sender.subscribe(),
            subscribed(&[]),
            "test",
            String::new,
            |_| true,
        );
        assert_eq!(event_sender.receiver_count(), 1);

        drop(forwarder);

        assert!(pump_until(&context, || event_sender.receiver_count() == 0));
    }

    #[test]
    fn a_sink_whose_target_is_gone_ends_forwarding() {
        let runtime = runtime();
        let context = glib::MainContext::new();
        let _owner = context.acquire().unwrap();
        let (event_sender, _) = broadcast::channel::<EventEnvelope>(16);
        let _forwarder = spawn_event_forwarder(
            runtime.handle(),
            &context,
            event_sender.subscribe(),
            subscribed(&["clock.event"]),
            "test",
            String::new,
            |_| false,
        );

        event_sender.send(envelope("clock.event", "1")).unwrap();
        // The GLib side ends after the first batch; the next send then fails on
        // the Tokio side, which ends that task and drops the subscription.
        assert!(pump_until(&context, || {
            let _ = event_sender.send(envelope("clock.event", "2"));
            event_sender.receiver_count() == 0
        }));
    }

    #[test]
    fn theme_reload_also_pushes_tokens() {
        let runtime = runtime();
        let context = glib::MainContext::new();
        let _owner = context.acquire().unwrap();
        let (event_sender, _) = broadcast::channel::<EventEnvelope>(16);
        let delivered = Rc::new(RefCell::new(Vec::<String>::new()));
        let target = delivered.clone();
        let _forwarder = spawn_event_forwarder(
            runtime.handle(),
            &context,
            event_sender.subscribe(),
            subscribed(&[]),
            "test",
            || "pushTokens()".to_string(),
            move |batch| {
                target.borrow_mut().push(batch.to_string());
                true
            },
        );

        event_sender
            .send(envelope(THEME_RELOADED_CHANNEL, "{}"))
            .unwrap();

        assert!(pump_until(&context, || delivered
            .borrow()
            .join(";")
            .contains("pushTokens()")));
    }
}
