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

/// Resolving a StatusNotifierItem's icon name, private theme path, or inline
/// pixmaps into a [`quantum_domain::IconRef`] for the frontend.
pub mod icon;

/// Selecting the best StatusNotifierItem icon pixmap and encoding it as a
/// PNG data URI for the frontend.
pub mod pixmap;

/// Pure bookkeeping for tracking which StatusNotifierItems are registered so
/// the host can add, remove, and enumerate them.
pub mod registry;

/// The StatusNotifierWatcher DBus server and per-item property and menu
/// mirroring: the bus-facing core that discovers, mirrors, and removes tray
/// items and broadcasts the resulting state.
pub mod host;

/// The [`ProviderSource`](quantum_domain::ProviderSource) implementation that
/// runs the host under a reconnect backoff loop and forwards user activations
/// back to applications over DBus.
pub mod provider;

pub use provider::SystemTrayProvider;
