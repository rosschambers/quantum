//! System tray provider module.
//!
//! Implements the host side of the StatusNotifierItem and
//! com.canonical.dbusmenu protocols: discovering tray items registered on
//! the session bus, mirroring their icons and menu trees into
//! [`quantum_domain::SystemTrayState`], and forwarding user activations
//! back over DBus.
//!
//! The module is split into focused submodules so each protocol concern
//! stays independently testable. This task lands the pure dbusmenu layout
//! parser ([`menu`]); later tasks add pixmap decoding, icon resolution, a
//! registry, and the bus-facing host along with the provider struct
//! wired up here.

pub mod menu;
