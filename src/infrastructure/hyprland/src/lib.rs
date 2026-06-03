//! quantum-hyprland: hyprctl + hyprland event socket client.

pub mod client;
pub mod error;

pub use client::{HyprlandEvent, HyprlandSocketClient};
pub use error::HyprlandError;
