//! quantum-plugins: discovers user-authored plugins under
//! ~/.config/quantum/plugins/<name>/ and produces description records
//! that the daemon binds into its provider / action / view registries.
//!
//! Phase 2 ships only the public surface; the actual walker lands in
//! Phase 3.

pub mod error;
pub mod manifest;

pub use error::PluginsError;
pub use manifest::{parse_manifest, Manifest, ScriptConfig};

use std::path::Path;

/// A discovered plugin. Phase 3 fills in polled scripts, action
/// scripts, and view bundles.
#[derive(Debug, Clone, Default)]
pub struct PluginDescription {
    pub name: String,
}

/// Walk the plugin directory and return one description per discovered
/// plugin. Phase 3 implements the real walker; this stub always returns
/// an empty list.
pub fn walk(_plugins_dir: &Path) -> Result<Vec<PluginDescription>, PluginsError> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn walk_returns_empty_for_now() {
        let result = walk(&PathBuf::from("/nonexistent")).expect("stub never errors");
        assert!(result.is_empty());
    }
}
