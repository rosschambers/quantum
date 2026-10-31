//! Per-webview event subscription filtering.
//!
//! Each WebView's broadcast forwarder consults a shared set of channels the
//! webview's JavaScript client has subscribed to. A webview that has subscribed
//! to nothing receives nothing except `theme.reloaded`, so a large payload (for
//! example the task manager's `processes.event`) is only marshaled into the
//! webviews that actually asked for it, not into every window.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Channels a single webview has subscribed to. The set starts empty and grows
/// only through `bridge.subscribe`; `bridge.unsubscribe` removes a channel.
pub type WebviewSubscriptions = Arc<Mutex<HashSet<String>>>;

/// The forwarding decision — pure and unit-tested.
///
/// Only the channels in `subs` are forwarded, plus `theme.reloaded`, which the
/// host pushes into the live stylesheet on a theme reload and which every view
/// needs whether or not it subscribed.
///
/// Rule: a webview receives a broadcast event only when it asked for that
/// channel. Forwarding unrequested events is not free. WebKitGTK's
/// `webkit_web_view_evaluate_javascript` always removes the page's transient
/// user activation (`RemoveTransientActivation::Yes`), so every push that
/// reached a page that never asked for it cleared the activation that
/// `navigator.clipboard.writeText` needs. The quick view's Copy button failed
/// about one click in ten because the once-a-second `system.stats.event` push
/// landed between mousedown and the click. Idle views also wasted work
/// marshaling payloads nobody reads.
pub fn should_forward(channel: &str, subs: &HashSet<String>) -> bool {
    channel == "theme.reloaded" || subs.contains(channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subscribed(channels: &[&str]) -> HashSet<String> {
        channels.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn empty_set_forwards_nothing_except_theme_reloaded() {
        let subs = subscribed(&[]);
        assert!(!should_forward("processes.event", &subs));
        assert!(!should_forward("mpris.event", &subs));
        assert!(!should_forward("system.stats.event", &subs));
        assert!(should_forward("theme.reloaded", &subs));
    }

    #[test]
    fn set_forwards_exactly_its_listed_channels() {
        let subs = subscribed(&["system.stats.event", "mpris.event"]);
        assert!(should_forward("system.stats.event", &subs));
        assert!(should_forward("mpris.event", &subs));
        assert!(!should_forward("processes.event", &subs));
        assert!(should_forward("theme.reloaded", &subs));
    }
}
