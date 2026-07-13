//! Per-webview event subscription filtering.
//!
//! Each WebView's broadcast forwarder consults a shared set of channels the
//! webview's JavaScript client has subscribed to, so a large payload (for
//! example the task manager's `processes.event`) is only marshaled into the
//! webviews that actually asked for it, not into every window.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Channels a single webview has subscribed to. `None` until the JavaScript
/// client registers its first subscription — during that window the forwarder
/// sends ALL events so early events are never dropped before the client is
/// ready. Once `Some`, only listed channels (plus always `theme.reloaded`) are
/// forwarded.
pub type WebviewSubscriptions = Arc<Mutex<Option<HashSet<String>>>>;

/// The forwarding decision — pure and unit-tested.
///
/// `theme.reloaded` is host-driven (the host pushes freshly resolved tokens
/// into the live stylesheet on a theme reload) and is always forwarded. Before
/// the client has seeded any subscription (`None`) every channel is forwarded
/// so no early event is lost. Once seeded (`Some`) only the listed channels
/// pass.
pub fn should_forward(channel: &str, subs: &Option<HashSet<String>>) -> bool {
    if channel == "theme.reloaded" {
        return true;
    }
    match subs {
        None => true,
        Some(set) => set.contains(channel),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded(channels: &[&str]) -> Option<HashSet<String>> {
        Some(channels.iter().map(|c| c.to_string()).collect())
    }

    #[test]
    fn not_seeded_forwards_anything() {
        let subs: Option<HashSet<String>> = None;
        assert!(should_forward("processes.event", &subs));
        assert!(should_forward("mpris.event", &subs));
        assert!(should_forward("theme.reloaded", &subs));
    }

    #[test]
    fn seeded_forwards_only_listed_channels() {
        let subs = seeded(&["system.stats.event", "mpris.event"]);
        assert!(should_forward("system.stats.event", &subs));
        assert!(should_forward("mpris.event", &subs));
        assert!(!should_forward("processes.event", &subs));
    }

    #[test]
    fn theme_reloaded_always_forwarded_even_when_seeded_and_absent() {
        let subs = seeded(&["system.stats.event"]);
        assert!(!subs.as_ref().unwrap().contains("theme.reloaded"));
        assert!(should_forward("theme.reloaded", &subs));
    }

    #[test]
    fn empty_seeded_set_forwards_nothing_except_theme_reloaded() {
        let subs = seeded(&[]);
        assert!(!should_forward("processes.event", &subs));
        assert!(!should_forward("mpris.event", &subs));
        assert!(should_forward("theme.reloaded", &subs));
    }
}
