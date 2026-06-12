//! Lookup table from canonical view names to their window descriptors.
//!
//! The canonical name format is `plugin/<plugin>/<view>`. quantum-ui must
//! not depend on the plugin discovery crate (Ui may only see Domain and
//! Application), so the daemon flattens discovered plugins into
//! `(canonical name, descriptor)` tuples and hands them to
//! [`ViewCatalog::from_plugins`].

use std::collections::HashMap;

use quantum_domain::ViewDescriptor;

/// Maps canonical view names (`plugin/<plugin>/<view>`) to the
/// [`ViewDescriptor`] declared by the owning plugin.
#[derive(Debug, Clone, Default)]
pub struct ViewCatalog {
    descriptors: HashMap<String, ViewDescriptor>,
}

impl ViewCatalog {
    /// Build a catalog from pre-flattened `(canonical name, descriptor)`
    /// tuples. The daemon produces these from its merged plugin list; the
    /// canonical name is expected to already be in `plugin/<plugin>/<view>`
    /// form. Later tuples with a duplicate name overwrite earlier ones.
    pub fn from_plugins(entries: Vec<(String, ViewDescriptor)>) -> Self {
        Self {
            descriptors: entries.into_iter().collect(),
        }
    }

    /// Look up the descriptor for a canonical view name.
    pub fn get(&self, canonical_name: &str) -> Option<&ViewDescriptor> {
        self.descriptors.get(canonical_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantum_domain::{ViewAnchor, ViewKind};

    fn panel_descriptor() -> ViewDescriptor {
        ViewDescriptor {
            kind: ViewKind::Panel,
            anchor: ViewAnchor::Top,
            height: Some(32),
            ..ViewDescriptor::default()
        }
    }

    #[test]
    fn from_plugins_keys_entries_by_canonical_name() {
        let catalog = ViewCatalog::from_plugins(vec![
            ("plugin/bar/bar".to_string(), panel_descriptor()),
            (
                "plugin/power-menu/power-menu".to_string(),
                ViewDescriptor::default(),
            ),
        ]);
        assert_eq!(catalog.get("plugin/bar/bar"), Some(&panel_descriptor()));
        assert_eq!(
            catalog.get("plugin/power-menu/power-menu"),
            Some(&ViewDescriptor::default())
        );
    }

    #[test]
    fn get_returns_none_for_unknown_name() {
        let catalog =
            ViewCatalog::from_plugins(vec![("plugin/bar/bar".to_string(), panel_descriptor())]);
        assert_eq!(catalog.get("plugin/bar/nope"), None);
        assert_eq!(catalog.get("widgets/bar"), None);
    }

    #[test]
    fn empty_catalog_returns_none() {
        let catalog = ViewCatalog::from_plugins(vec![]);
        assert_eq!(catalog.get("plugin/bar/bar"), None);
    }

    #[test]
    fn duplicate_canonical_name_keeps_last_entry() {
        let catalog = ViewCatalog::from_plugins(vec![
            ("plugin/bar/bar".to_string(), ViewDescriptor::default()),
            ("plugin/bar/bar".to_string(), panel_descriptor()),
        ]);
        assert_eq!(catalog.get("plugin/bar/bar"), Some(&panel_descriptor()));
    }
}
