//! Focused-monitor resolution for the transient notification toast.
//!
//! The toast is opened by a daemon-side task on every `created`
//! notification event, not by a bar click, so no webview ever supplies a
//! `@<monitor>` suffix and the layer surface used to land on the
//! compositor-default output. At show time the task queries the
//! `hyprland.activewindow` provider (a cheap in-memory read) and appends
//! the focused monitor as the construction-time monitor hint. The window
//! registry's single-instance eviction (see `registry.rs`) then
//! re-anchors the surface whenever the requested monitor changes.

/// Canonical toast view name, without a monitor suffix.
pub const TOAST_VIEW_NAME: &str = "plugin/notification-center/toast";

/// Pull `focused_monitor` out of a `MonitorActiveWindowState` value (the
/// `provider.query` result for `hyprland.activewindow`). Returns `None`
/// for unexpected shapes or when no monitor currently has focus.
pub fn extract_focused_monitor(state: &serde_json::Value) -> Option<String> {
    state.get("focused_monitor")?.as_str().map(str::to_owned)
}

/// The view name to request when showing the toast: suffixed with the
/// focused monitor when known, bare (compositor default) otherwise.
pub fn toast_view_name(focused_monitor: Option<&str>) -> String {
    match focused_monitor {
        Some(monitor) => format!("{TOAST_VIEW_NAME}@{monitor}"),
        None => TOAST_VIEW_NAME.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_focused_monitor_from_state() {
        let state = serde_json::json!({
            "monitors": {"DP-1": {"title": "x", "class": "y", "workspace_id": 3}},
            "focused_monitor": "DP-1"
        });
        assert_eq!(extract_focused_monitor(&state), Some("DP-1".to_owned()));
    }

    #[test]
    fn returns_none_when_no_monitor_has_focus() {
        let state = serde_json::json!({"monitors": {}, "focused_monitor": null});
        assert_eq!(extract_focused_monitor(&state), None);
    }

    #[test]
    fn returns_none_for_unexpected_shape() {
        assert_eq!(extract_focused_monitor(&serde_json::json!({})), None);
        assert_eq!(extract_focused_monitor(&serde_json::json!("text")), None);
    }

    #[test]
    fn toast_view_name_appends_focused_monitor_suffix() {
        assert_eq!(
            toast_view_name(Some("HDMI-A-1")),
            "plugin/notification-center/toast@HDMI-A-1"
        );
    }

    #[test]
    fn toast_view_name_falls_back_to_bare_name() {
        assert_eq!(toast_view_name(None), "plugin/notification-center/toast");
    }
}
