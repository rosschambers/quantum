//! quantum-plugins: discovers user-authored plugins under
//! ~/.config/quantum/plugins/<name>/ and produces description records
//! that the daemon binds into its provider / action / view registries.
//!
//! Phase 3 lands the manifest parser, description types, and walker.
//! Phase 4 wires them into the daemon.

pub mod description;
pub mod embedded;
pub mod error;
pub mod manifest;
pub mod view_metadata;
pub mod walker;

pub use description::{ActionScript, IdleScript, PluginDescription, PolledScript, ViewBundle};
pub use embedded::{merge_plugins, walk_dev, walk_embedded};
pub use error::PluginsError;
pub use manifest::{parse_manifest, Manifest, ScriptConfig};
pub use quantum_domain::{ViewAnchor, ViewDescriptor, ViewKind};
pub use view_metadata::parse_view_toml;
pub use walker::walk;
