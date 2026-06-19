//! Walk an `include_dir`-embedded plugin catalog and produce
//! `PluginDescription` records, plus the merge that lets user plugins
//! shadow embedded ones by name.
//!
//! Embedded plugins carry views only: `polled_scripts`, `idle_scripts`,
//! and `actions` are always empty. First-party behavior lives in Svelte
//! views, not shell scripts, so there is nothing executable to embed.
//!
//! `ViewBundle::dir` for embedded views stores the relative path inside
//! the embedded directory (for example `alpha/views/main`), because no
//! filesystem path exists for compiled-in assets.

use crate::description::{PluginDescription, ViewBundle};
use crate::error::PluginsError;
use quantum_domain::ViewDescriptor;
use std::path::PathBuf;

/// Walk an embedded plugin catalog. Each top-level directory is a plugin
/// named after the directory; each `views/<name>/` containing
/// `index.html` or `dist/index.html` is a view. A `view.toml` next to
/// the chosen `index.html` is parsed when present; malformed metadata
/// falls back to the default descriptor with a warning, mirroring the
/// filesystem walker.
pub fn walk_embedded(
    dir: &include_dir::Dir<'static>,
) -> Result<Vec<PluginDescription>, PluginsError> {
    let mut plugins = Vec::new();
    for plugin_dir in dir.dirs() {
        let Some(name) = plugin_dir
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let views = list_embedded_views(&name, plugin_dir);
        plugins.push(PluginDescription {
            name,
            dir: plugin_dir.path().to_path_buf(),
            polled_scripts: Vec::new(),
            idle_scripts: Vec::new(),
            actions: Vec::new(),
            views,
        });
    }
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(plugins)
}

/// Walk a FILESYSTEM plugins root with the same acceptance rules as
/// [`walk_embedded`], for developer mode (`QUANTUM_PLUGIN_DIR`). Each
/// top-level directory is a plugin named after the directory; each
/// `views/<name>/` containing `index.html` or `dist/index.html` is a
/// view. A `view.toml` next to the chosen `index.html` is parsed when
/// present; malformed or missing metadata falls back to the default
/// descriptor with a warning, mirroring [`walk_embedded`]. The only
/// difference is the source: this reads from `std::fs` instead of a
/// compiled-in `include_dir::Dir`, so a developer can serve the working
/// tree directly without recompiling the daemon. A missing or unreadable
/// root yields an empty catalog rather than an error.
pub fn walk_dev(root: &std::path::Path) -> Result<Vec<PluginDescription>, PluginsError> {
    let mut plugins = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        tracing::warn!(
            "dev plugins root {} is missing or unreadable; serving no dev plugins",
            root.display()
        );
        return Ok(plugins);
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let views = list_dev_views(&name, &path);
        plugins.push(PluginDescription {
            name,
            dir: path,
            polled_scripts: Vec::new(),
            idle_scripts: Vec::new(),
            actions: Vec::new(),
            views,
        });
    }
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(plugins)
}

/// Merge user plugins over embedded plugins. A user plugin with the same
/// name as an embedded one replaces it wholesale. The result is sorted
/// by name.
pub fn merge_plugins(
    user: Vec<PluginDescription>,
    embedded: Vec<PluginDescription>,
) -> Vec<PluginDescription> {
    let mut merged: Vec<PluginDescription> = embedded
        .into_iter()
        .filter(|e| !user.iter().any(|u| u.name == e.name))
        .collect();
    merged.extend(user);
    merged.sort_by(|a, b| a.name.cmp(&b.name));
    merged
}

fn list_embedded_views(plugin_name: &str, plugin_dir: &include_dir::Dir<'_>) -> Vec<ViewBundle> {
    let views_path = plugin_dir.path().join("views");
    let Some(views_dir) = plugin_dir.get_dir(&views_path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for view_dir in views_dir.dirs() {
        let Some(view_name) = view_dir
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let direct_index = view_dir.path().join("index.html");
        let dist_index = view_dir.path().join("dist/index.html");
        let bundle_dir: PathBuf = if view_dir.get_file(&direct_index).is_some() {
            view_dir.path().to_path_buf()
        } else if view_dir.get_file(&dist_index).is_some() {
            view_dir.path().join("dist")
        } else {
            continue;
        };
        let descriptor = read_embedded_view_descriptor(plugin_name, &view_name, view_dir);
        out.push(ViewBundle {
            name: view_name,
            dir: bundle_dir,
            descriptor,
        });
    }
    out
}

fn list_dev_views(plugin_name: &str, plugin_dir: &std::path::Path) -> Vec<ViewBundle> {
    let views_dir = plugin_dir.join("views");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&views_dir) else {
        return out;
    };
    for entry in entries.filter_map(Result::ok) {
        let view_path = entry.path();
        if !view_path.is_dir() {
            continue;
        }
        let Some(view_name) = view_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if view_name.starts_with('.') {
            continue;
        }
        let direct_index = view_path.join("index.html");
        let dist_index = view_path.join("dist/index.html");
        let bundle_dir: PathBuf = if direct_index.exists() {
            view_path.clone()
        } else if dist_index.exists() {
            view_path.join("dist")
        } else {
            continue;
        };
        let descriptor = read_dev_view_descriptor(plugin_name, &view_name, &view_path);
        out.push(ViewBundle {
            name: view_name,
            dir: bundle_dir,
            descriptor,
        });
    }
    out
}

fn read_dev_view_descriptor(
    plugin_name: &str,
    view_name: &str,
    view_dir: &std::path::Path,
) -> ViewDescriptor {
    let descriptor_path = view_dir.join("view.toml");
    let text = match std::fs::read_to_string(&descriptor_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return ViewDescriptor::default();
        }
        Err(e) => {
            tracing::warn!(
                "dev plugin '{plugin_name}' view '{view_name}': failed to read {}: {e}; using default descriptor",
                descriptor_path.display()
            );
            return ViewDescriptor::default();
        }
    };
    match crate::view_metadata::parse_view_toml(&text) {
        Ok(descriptor) => descriptor,
        Err(e) => {
            tracing::warn!(
                "dev plugin '{plugin_name}' view '{view_name}': invalid view.toml: {e}; using default descriptor"
            );
            ViewDescriptor::default()
        }
    }
}

fn read_embedded_view_descriptor(
    plugin_name: &str,
    view_name: &str,
    view_dir: &include_dir::Dir<'_>,
) -> ViewDescriptor {
    let descriptor_path = view_dir.path().join("view.toml");
    let Some(file) = view_dir.get_file(&descriptor_path) else {
        return ViewDescriptor::default();
    };
    let Some(text) = file.contents_utf8() else {
        tracing::warn!(
            "embedded plugin '{plugin_name}' view '{view_name}': view.toml is not valid UTF-8; using default descriptor"
        );
        return ViewDescriptor::default();
    };
    match crate::view_metadata::parse_view_toml(text) {
        Ok(descriptor) => descriptor,
        Err(e) => {
            tracing::warn!(
                "embedded plugin '{plugin_name}' view '{view_name}': invalid view.toml: {e}; using default descriptor"
            );
            ViewDescriptor::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    static FIXTURE: include_dir::Dir<'static> =
        include_dir::include_dir!("$CARGO_MANIFEST_DIR/test-fixtures/embedded-plugins");

    static MALFORMED_FIXTURE: include_dir::Dir<'static> =
        include_dir::include_dir!("$CARGO_MANIFEST_DIR/test-fixtures/embedded-plugins-malformed");

    fn user_plugin(name: &str) -> PluginDescription {
        PluginDescription {
            name: name.to_string(),
            dir: PathBuf::from(format!("/home/user/.config/quantum/plugins/{name}")),
            polled_scripts: Vec::new(),
            idle_scripts: Vec::new(),
            actions: Vec::new(),
            views: Vec::new(),
        }
    }

    #[test]
    fn discovers_embedded_plugins_sorted_by_name() {
        let plugins = walk_embedded(&FIXTURE).expect("walk ok");
        let names: Vec<&str> = plugins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn embedded_plugins_have_no_scripts_or_actions() {
        let plugins = walk_embedded(&FIXTURE).expect("walk ok");
        for p in &plugins {
            assert!(p.polled_scripts.is_empty(), "{}: no polled scripts", p.name);
            assert!(p.idle_scripts.is_empty(), "{}: no idle scripts", p.name);
            assert!(p.actions.is_empty(), "{}: no actions", p.name);
        }
    }

    #[test]
    fn direct_index_view_is_discovered_with_parsed_descriptor() {
        let plugins = walk_embedded(&FIXTURE).expect("walk ok");
        let alpha = plugins.iter().find(|p| p.name == "alpha").expect("alpha");
        assert_eq!(alpha.views.len(), 1);
        let view = &alpha.views[0];
        assert_eq!(view.name, "main");
        assert_eq!(view.dir, Path::new("alpha/views/main"));
        assert_eq!(view.descriptor.kind, quantum_domain::ViewKind::Panel);
        assert_eq!(view.descriptor.anchor, quantum_domain::ViewAnchor::Top);
        assert_eq!(view.descriptor.height, Some(32));
        assert!(view.descriptor.per_monitor);
    }

    #[test]
    fn dist_index_view_is_discovered_with_default_descriptor() {
        let plugins = walk_embedded(&FIXTURE).expect("walk ok");
        let beta = plugins.iter().find(|p| p.name == "beta").expect("beta");
        assert_eq!(beta.views.len(), 1);
        let view = &beta.views[0];
        assert_eq!(view.name, "main");
        assert_eq!(view.dir, Path::new("beta/views/main/dist"));
        assert_eq!(view.descriptor, ViewDescriptor::default());
    }

    #[test]
    fn malformed_view_toml_falls_back_to_default_and_view_is_still_discovered() {
        let plugins = walk_embedded(&MALFORMED_FIXTURE).expect("walk ok");
        assert_eq!(plugins.len(), 1);
        let gamma = &plugins[0];
        assert_eq!(gamma.name, "gamma");
        assert_eq!(gamma.views.len(), 1, "view must still be discovered");
        assert_eq!(gamma.views[0].name, "main");
        assert_eq!(gamma.views[0].descriptor, ViewDescriptor::default());
    }

    #[test]
    fn merge_user_shadows_embedded_with_same_name() {
        let embedded = walk_embedded(&FIXTURE).expect("walk ok");
        let user = vec![user_plugin("alpha")];
        let merged = merge_plugins(user, embedded);
        assert_eq!(merged.len(), 2);
        let alpha = merged.iter().find(|p| p.name == "alpha").expect("alpha");
        assert_eq!(
            alpha.dir,
            Path::new("/home/user/.config/quantum/plugins/alpha"),
            "user plugin must replace the embedded one wholesale"
        );
        assert!(merged.iter().any(|p| p.name == "beta"));
    }

    #[test]
    fn merge_result_is_sorted_by_name() {
        let embedded = walk_embedded(&FIXTURE).expect("walk ok");
        let user = vec![user_plugin("zeta"), user_plugin("aardvark")];
        let merged = merge_plugins(user, embedded);
        let names: Vec<&str> = merged.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["aardvark", "alpha", "beta", "zeta"]);
    }

    #[test]
    fn merge_with_no_user_plugins_returns_embedded() {
        let embedded = walk_embedded(&FIXTURE).expect("walk ok");
        let merged = merge_plugins(Vec::new(), embedded);
        let names: Vec<&str> = merged.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn walk_dev_discovers_filesystem_view_with_parsed_descriptor() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let view_dir = tmp.path().join("bar/views/bar");
        std::fs::create_dir_all(view_dir.join("dist")).expect("mkdir");
        std::fs::write(view_dir.join("dist/index.html"), b"<html>bar</html>").expect("write index");
        std::fs::write(
            view_dir.join("view.toml"),
            "kind = \"panel\"\nanchor = \"top\"\nheight = 32\nper_monitor = true\n",
        )
        .expect("write view.toml");

        let plugins = walk_dev(tmp.path()).expect("walk ok");
        assert_eq!(plugins.len(), 1);
        let bar = &plugins[0];
        assert_eq!(bar.name, "bar");
        assert_eq!(bar.views.len(), 1);
        let view = &bar.views[0];
        assert_eq!(view.name, "bar");
        assert_eq!(
            format!("plugin/{}/{}", bar.name, view.name),
            "plugin/bar/bar"
        );
        assert_eq!(view.dir, view_dir.join("dist"));
        assert_eq!(view.descriptor.kind, quantum_domain::ViewKind::Panel);
        assert_eq!(view.descriptor.anchor, quantum_domain::ViewAnchor::Top);
        assert_eq!(view.descriptor.height, Some(32));
        assert!(view.descriptor.per_monitor);
    }

    #[test]
    fn walk_dev_skips_views_without_index_html() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // A view directory with a view.toml but no index.html anywhere must
        // not be discovered, matching walk_embedded's acceptance rule.
        let view_dir = tmp.path().join("bar/views/empty");
        std::fs::create_dir_all(&view_dir).expect("mkdir");
        std::fs::write(view_dir.join("view.toml"), "kind = \"panel\"\n").expect("write view.toml");

        let plugins = walk_dev(tmp.path()).expect("walk ok");
        let bar = plugins.iter().find(|p| p.name == "bar").expect("bar");
        assert!(bar.views.is_empty(), "view without index.html is skipped");
    }

    #[test]
    fn walk_dev_missing_root_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing = tmp.path().join("does-not-exist");
        let plugins = walk_dev(&missing).expect("missing root is not an error");
        assert!(plugins.is_empty());
    }
}
