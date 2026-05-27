use serde::{Deserialize, Serialize};

/// Window display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    /// Toggle visibility (show if hidden, hide if shown).
    Toggle,
    /// Show the window.
    Show,
    /// Hide the window.
    Hide,
}
