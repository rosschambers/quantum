#[cfg(test)]
mod tests {
    use cargo_metadata::{MetadataCommand, Package};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// Onion layer derived from a crate's manifest path.
    ///
    /// The rule is: where the crate lives in the source tree dictates what it
    /// is allowed to depend on. Adding a new crate under `src/<layer>/<name>/`
    /// automatically picks up the right constraints; no edits to this file are
    /// needed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum Layer {
        /// `src/domain/...`: no in-workspace dependencies allowed.
        Domain,
        /// `src/application/...`: depends on Domain only.
        Application,
        /// `src/infrastructure/<name>/...`: depends on Domain and sibling
        /// infrastructure crates only. Sibling-on-sibling is allowed because
        /// after the layout reorg the infrastructure region is genuinely
        /// composed of crates that wire together (for example providers
        /// pulling in dbus and hyprland).
        Infrastructure,
        /// `src/ui/...`: depends on Domain and Application. Must not reach
        /// into Infrastructure directly.
        Ui,
        /// `src/binaries/...`: composition roots, may depend on anything.
        Binary,
        /// `tests/...`: harness crates (architecture-test, e2e), unconstrained.
        Test,
    }

    impl Layer {
        fn label(self) -> &'static str {
            match self {
                Layer::Domain => "Domain (src/domain)",
                Layer::Application => "Application (src/application)",
                Layer::Infrastructure => "Infrastructure (src/infrastructure)",
                Layer::Ui => "Ui (src/ui)",
                Layer::Binary => "Binary (src/binaries)",
                Layer::Test => "Test (tests/)",
            }
        }
    }

    /// A crate together with the metadata derived from its location.
    #[derive(Debug, Clone)]
    struct Crate {
        name: String,
        layer: Layer,
    }

    /// Classify a package by its manifest path relative to the workspace root.
    ///
    /// Returns `None` for packages outside the recognized top-level folders;
    /// the test treats that as a hard failure rather than silently allowing
    /// the crate.
    fn classify(package: &Package, workspace_root: &Path) -> Option<Crate> {
        let manifest = PathBuf::from(package.manifest_path.as_std_path());
        let relative = manifest.strip_prefix(workspace_root).ok()?;
        let mut components = relative.components();
        let top = components.next()?.as_os_str().to_str()?;

        match top {
            "src" => {
                let layer_dir = components.next()?.as_os_str().to_str()?;
                match layer_dir {
                    "domain" => Some(Crate {
                        name: package.name.to_string(),
                        layer: Layer::Domain,
                    }),
                    "application" => Some(Crate {
                        name: package.name.to_string(),
                        layer: Layer::Application,
                    }),
                    "infrastructure" => Some(Crate {
                        name: package.name.to_string(),
                        layer: Layer::Infrastructure,
                    }),
                    "ui" => Some(Crate {
                        name: package.name.to_string(),
                        layer: Layer::Ui,
                    }),
                    "binaries" => Some(Crate {
                        name: package.name.to_string(),
                        layer: Layer::Binary,
                    }),
                    _ => None,
                }
            }
            "tests" => Some(Crate {
                name: package.name.to_string(),
                layer: Layer::Test,
            }),
            _ => None,
        }
    }

    /// Returns Ok(()) if `from` is allowed to depend on `to`, Err with a
    /// human-readable reason otherwise.
    fn check_edge(from: &Crate, to: &Crate) -> Result<(), String> {
        match from.layer {
            Layer::Domain => Err(format!(
                "{} is in {} and must not depend on any workspace crate",
                from.name,
                from.layer.label(),
            )),
            Layer::Application => match to.layer {
                Layer::Domain => Ok(()),
                _ => Err(format!(
                    "{} ({}) may only depend on Domain; {} is {}",
                    from.name,
                    from.layer.label(),
                    to.name,
                    to.layer.label(),
                )),
            },
            Layer::Infrastructure => match to.layer {
                Layer::Domain => Ok(()),
                Layer::Infrastructure => Ok(()),
                _ => Err(format!(
                    "{} ({}) may only depend on Domain or sibling Infrastructure crates; \
                     {} is {}",
                    from.name,
                    from.layer.label(),
                    to.name,
                    to.layer.label(),
                )),
            },
            Layer::Ui => match to.layer {
                Layer::Domain | Layer::Application => Ok(()),
                _ => Err(format!(
                    "{} ({}) may only depend on Domain or Application; {} is {} \
                     (Ui must not reach into Infrastructure directly)",
                    from.name,
                    from.layer.label(),
                    to.name,
                    to.layer.label(),
                )),
            },
            // Composition roots and test harnesses are unconstrained by design.
            Layer::Binary | Layer::Test => Ok(()),
        }
    }

    #[test]
    fn workspace_layer_dependencies_are_legal() {
        let metadata = MetadataCommand::new().exec().expect("cargo metadata");
        let workspace_root = PathBuf::from(metadata.workspace_root.as_std_path());

        let mut crates: HashMap<String, Crate> = HashMap::new();
        let workspace_packages = metadata.workspace_packages();
        for package in &workspace_packages {
            let classified = classify(package, &workspace_root).unwrap_or_else(|| {
                panic!(
                    "ARCHITECTURE TEST: crate '{}' at {} does not live under a recognized \
                     top-level folder (expected src/domain, src/application, \
                     src/infrastructure/<name>, src/ui, src/binaries, or tests). \
                     Either move it into one of those folders or update \
                     tests/architecture-test/src/lib.rs::classify to recognize a new \
                     top-level region.",
                    package.name, package.manifest_path,
                );
            });
            crates.insert(package.name.to_string(), classified);
        }

        let mut violations: Vec<String> = Vec::new();
        for package in &workspace_packages {
            let from = &crates[package.name.as_str()];
            for dep in &package.dependencies {
                let Some(to) = crates.get(dep.name.as_str()) else {
                    // External (non-workspace) dependency. The layering rules
                    // only constrain intra-workspace edges.
                    continue;
                };
                if let Err(reason) = check_edge(from, to) {
                    violations.push(format!("  {} -> {}: {}", from.name, to.name, reason));
                }
            }
        }

        assert!(
            violations.is_empty(),
            "ARCHITECTURE TEST: forbidden intra-workspace dependency edges:\n{}\n\n\
             Layering rules (derived from manifest paths):\n  \
             - src/domain/...         depends on nothing in-workspace\n  \
             - src/application/...    depends on Domain only\n  \
             - src/infrastructure/... depends on Domain or sibling Infrastructure\n  \
             - src/ui/...             depends on Domain or Application\n  \
             - src/binaries/...       composition roots, may depend on anything\n  \
             - tests/...              harness crates, unconstrained",
            violations.join("\n"),
        );
    }
}
