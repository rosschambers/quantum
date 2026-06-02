#[cfg(test)]
mod tests {
    use cargo_metadata::MetadataCommand;
    use std::collections::{HashMap, HashSet};

    fn allowed_deps() -> HashMap<&'static str, HashSet<&'static str>> {
        let mut m: HashMap<&str, HashSet<&str>> = HashMap::new();
        m.insert("quantum-domain", HashSet::new());
        m.insert("quantum-dbus", ["quantum-domain"].into_iter().collect());
        m.insert("quantum-hyprland", ["quantum-domain"].into_iter().collect());
        m.insert("quantum-config", ["quantum-domain"].into_iter().collect());
        m.insert("quantum-theme", ["quantum-domain"].into_iter().collect());
        m.insert("quantum-ipc", ["quantum-domain"].into_iter().collect());
        m.insert(
            "quantum-providers",
            [
                "quantum-domain",
                "quantum-dbus",
                "quantum-hyprland",
                "quantum-config",
            ]
            .into_iter()
            .collect(),
        );
        m.insert(
            "quantum-application",
            ["quantum-domain"].into_iter().collect(),
        );
        m.insert(
            "quantum-infrastructure",
            [
                "quantum-domain",
                "quantum-dbus",
                "quantum-hyprland",
                "quantum-config",
                "quantum-theme",
                "quantum-ipc",
                "quantum-providers",
            ]
            .into_iter()
            .collect(),
        );
        m.insert(
            "quantum-ui",
            ["quantum-application", "quantum-domain"]
                .into_iter()
                .collect(),
        );
        m.insert(
            "quantumd",
            [
                "quantum-ui",
                "quantum-application",
                "quantum-infrastructure",
                "quantum-config",
                "quantum-theme",
                "quantum-domain",
                "quantum-ipc",
                "quantum-providers",
            ]
            .into_iter()
            .collect(),
        );
        m.insert(
            "quantumctl",
            ["quantum-domain", "quantum-infrastructure"]
                .into_iter()
                .collect(),
        );
        m.insert(
            "quantum-dev",
            ["quantum-domain", "quantum-infrastructure"]
                .into_iter()
                .collect(),
        );
        m.insert(
            "quantum-e2e",
            ["quantum-domain", "quantum-infrastructure"]
                .into_iter()
                .collect(),
        );
        m
    }

    #[test]
    fn workspace_layer_dependencies_are_legal() {
        let metadata = MetadataCommand::new().exec().expect("cargo metadata");
        let allowed = allowed_deps();

        for package in &metadata.workspace_packages() {
            let Some(allow) = allowed.get(package.name.as_str()) else {
                // architecture-test crate itself is unconstrained
                continue;
            };
            for dep in &package.dependencies {
                // Only constrain intra-workspace deps
                if metadata
                    .workspace_packages()
                    .iter()
                    .any(|p| p.name == dep.name)
                {
                    assert!(
                        allow.contains(dep.name.as_str()),
                        "FORBIDDEN DEP: {} -> {} (allowed: {:?})",
                        package.name,
                        dep.name,
                        allow
                    );
                }
            }
        }
    }
}
