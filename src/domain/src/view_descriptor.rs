//! Per-view window semantics. Describes how a plugin or theme view wants
//! to be hosted: as a widget, a panel, or an overlay, with optional anchor
//! and sizing hints.

use serde::{Deserialize, Serialize};

/// How a view wants its window hosted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    /// A free-floating widget window. The default.
    #[default]
    Widget,
    /// A bar-style panel anchored to a screen edge.
    Panel,
    /// A transient overlay window.
    Overlay,
    /// A small, non-modal toast window anchored per `position` and shown
    /// on demand.
    Toast,
}

/// Which screen edge a view anchors to, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewAnchor {
    /// No anchoring. The default.
    #[default]
    None,
    /// Anchored to the top edge.
    Top,
    /// Anchored to the bottom edge.
    Bottom,
}

/// Where on the screen a view places itself. Used by overlay cards and
/// toasts to pick a corner or edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewPosition {
    /// Centered on the screen. The default.
    #[default]
    Center,
    /// Pinned to the top-right corner.
    TopRight,
    /// Pinned to the top-left corner.
    TopLeft,
    /// Pinned to the top center edge.
    TopCenter,
}

/// Window semantics for a single view: kind, anchoring, sizing hints, and
/// instancing behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewDescriptor {
    pub kind: ViewKind,
    pub per_monitor: bool,
    pub auto_show: bool,
    pub anchor: ViewAnchor,
    pub position: ViewPosition,
    pub height: Option<u32>,
    pub width: Option<u32>,
    pub single_instance: Option<bool>,
    /// Whether a background-layer widget should fill the entire monitor with
    /// a transparent surface (anchoring all four edges) so its view can place
    /// content freely across the whole output. Ignored for top-anchored bars.
    #[serde(default)]
    pub fill_output: bool,
}

impl Default for ViewDescriptor {
    fn default() -> Self {
        Self {
            kind: ViewKind::Widget,
            per_monitor: false,
            auto_show: false,
            anchor: ViewAnchor::None,
            position: ViewPosition::Center,
            height: None,
            width: None,
            single_instance: None,
            fill_output: false,
        }
    }
}

impl ViewDescriptor {
    /// Whether only one window of this view may exist. An explicit
    /// `single_instance` value wins; otherwise panels and overlays are
    /// single-instance and widgets are not.
    pub fn effective_single_instance(&self) -> bool {
        match self.single_instance {
            Some(explicit) => explicit,
            None => matches!(
                self.kind,
                ViewKind::Panel | ViewKind::Overlay | ViewKind::Toast
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_default_is_widget_with_widget_defaults() {
        let descriptor = ViewDescriptor::default();
        assert_eq!(descriptor.kind, ViewKind::Widget);
        assert!(!descriptor.per_monitor);
        assert!(!descriptor.auto_show);
        assert_eq!(descriptor.anchor, ViewAnchor::None);
        assert_eq!(descriptor.position, ViewPosition::Center);
        assert_eq!(descriptor.height, None);
        assert_eq!(descriptor.width, None);
        assert_eq!(descriptor.single_instance, None);
        assert!(!descriptor.fill_output);
    }

    #[test]
    fn widget_derives_single_instance_false() {
        let descriptor = ViewDescriptor::default();
        assert!(!descriptor.effective_single_instance());
    }

    #[test]
    fn panel_derives_single_instance_true() {
        let descriptor = ViewDescriptor {
            kind: ViewKind::Panel,
            ..ViewDescriptor::default()
        };
        assert!(descriptor.effective_single_instance());
    }

    #[test]
    fn overlay_derives_single_instance_true() {
        let descriptor = ViewDescriptor {
            kind: ViewKind::Overlay,
            ..ViewDescriptor::default()
        };
        assert!(descriptor.effective_single_instance());
    }

    #[test]
    fn explicit_single_instance_false_on_overlay_wins() {
        let descriptor = ViewDescriptor {
            kind: ViewKind::Overlay,
            single_instance: Some(false),
            ..ViewDescriptor::default()
        };
        assert!(!descriptor.effective_single_instance());
    }

    #[test]
    fn explicit_single_instance_true_on_widget_wins() {
        let descriptor = ViewDescriptor {
            single_instance: Some(true),
            ..ViewDescriptor::default()
        };
        assert!(descriptor.effective_single_instance());
    }

    #[test]
    fn full_descriptor_serde_round_trip() {
        let descriptor = ViewDescriptor {
            kind: ViewKind::Panel,
            per_monitor: true,
            auto_show: true,
            anchor: ViewAnchor::Bottom,
            position: ViewPosition::TopRight,
            height: Some(32),
            width: Some(1920),
            single_instance: Some(true),
            fill_output: true,
        };
        let json = serde_json::to_string(&descriptor).unwrap();
        let restored: ViewDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, descriptor);
    }

    #[test]
    fn view_kind_serializes_as_snake_case() {
        let json = serde_json::to_string(&ViewKind::Overlay).unwrap();
        assert_eq!(json, "\"overlay\"");
        let json = serde_json::to_string(&ViewAnchor::Bottom).unwrap();
        assert_eq!(json, "\"bottom\"");
    }

    #[test]
    fn view_kind_toast_serializes() {
        assert_eq!(
            serde_json::to_string(&ViewKind::Toast).unwrap(),
            "\"toast\""
        );
    }

    #[test]
    fn toast_derives_single_instance_true() {
        let descriptor = ViewDescriptor {
            kind: ViewKind::Toast,
            ..ViewDescriptor::default()
        };
        assert!(descriptor.effective_single_instance());
    }

    #[test]
    fn position_defaults_to_center_and_round_trips() {
        let descriptor = ViewDescriptor::default();
        assert_eq!(descriptor.position, ViewPosition::Center);
        let with_pos = ViewDescriptor {
            position: ViewPosition::TopRight,
            ..ViewDescriptor::default()
        };
        let json = serde_json::to_string(&with_pos).unwrap();
        let restored: ViewDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, with_pos);
    }

    #[test]
    fn view_position_serializes_snake_case() {
        let json = serde_json::to_string(&ViewPosition::TopRight).unwrap();
        assert_eq!(json, "\"top_right\"");
    }
}
