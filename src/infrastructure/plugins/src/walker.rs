//! Walk a `~/.config/quantum/plugins/` directory and produce a
//! `PluginDescription` per subdirectory by inspecting `config.toml`,
//! `scripts/`, `actions/`, and `views/`.
//!
//! Collision detection between plugins is intentionally deferred to a
//! later task; this module surfaces every well-formed plugin it finds.

use crate::description::{ActionScript, IdleScript, PluginDescription, PolledScript, ViewBundle};
use crate::error::PluginsError;
use crate::manifest::{parse_manifest, Manifest};
use quantum_domain::ViewDescriptor;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub fn walk(plugins_dir: &Path) -> Result<Vec<PluginDescription>, PluginsError> {
    let mut plugins = Vec::new();

    let entries = match fs::read_dir(plugins_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(PluginsError::Io(e.to_string())),
    };

    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        match describe_plugin(&name, &dir) {
            Ok(desc) => plugins.push(desc),
            Err(e) => {
                tracing::warn!("skipping plugin '{name}': {e}");
            }
        }
    }

    plugins.sort_by(|a, b| a.name.cmp(&b.name));

    let mut claimed: HashSet<String> = HashSet::new();
    for plugin in &mut plugins {
        let mut keep: Vec<PolledScript> = Vec::new();
        let mut demoted: Vec<IdleScript> = Vec::new();
        for ps in plugin.polled_scripts.drain(..) {
            if claimed.insert(ps.channel.clone()) {
                keep.push(ps);
            } else {
                tracing::warn!(
                    "plugin '{}': channel '{}' already claimed; script downgraded to idle",
                    plugin.name,
                    ps.channel
                );
                demoted.push(IdleScript {
                    command: ps.command,
                    channel: ps.channel,
                });
            }
        }
        plugin.polled_scripts = keep;
        plugin.idle_scripts.extend(demoted);
    }

    Ok(plugins)
}

fn describe_plugin(name: &str, dir: &Path) -> Result<PluginDescription, PluginsError> {
    let manifest = read_manifest(dir)?;
    let mut polled_scripts = Vec::new();
    let mut idle_scripts = Vec::new();
    classify_scripts(name, dir, &manifest, &mut polled_scripts, &mut idle_scripts);
    let actions = list_actions(dir);
    let views = list_views(dir);
    Ok(PluginDescription {
        name: name.to_string(),
        dir: dir.to_path_buf(),
        polled_scripts,
        idle_scripts,
        actions,
        views,
    })
}

fn read_manifest(dir: &Path) -> Result<Manifest, PluginsError> {
    let config_path = dir.join("config.toml");
    match fs::read_to_string(&config_path) {
        Ok(text) => parse_manifest(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
        Err(e) => Err(PluginsError::Io(e.to_string())),
    }
}

fn is_executable(p: &Path) -> bool {
    fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn file_stem(p: &Path) -> String {
    p.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

fn classify_scripts(
    plugin_name: &str,
    dir: &Path,
    manifest: &Manifest,
    polled: &mut Vec<PolledScript>,
    idle: &mut Vec<IdleScript>,
) {
    let scripts_dir = dir.join("scripts");
    let Ok(entries) = fs::read_dir(&scripts_dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !is_executable(&path) {
            tracing::trace!("skipping non-executable script: {}", path.display());
            continue;
        }
        let basename = file_stem(&path);
        if let Some(cfg) = manifest.scripts.get(&basename) {
            let channel = cfg
                .channel
                .clone()
                .unwrap_or_else(|| format!("{plugin_name}.{basename}"));
            polled.push(PolledScript {
                command: path,
                interval: cfg.interval,
                channel,
            });
        } else {
            let channel = format!("{plugin_name}.{basename}");
            idle.push(IdleScript {
                command: path,
                channel,
            });
        }
    }
}

fn list_actions(dir: &Path) -> Vec<ActionScript> {
    let actions_dir = dir.join("actions");
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&actions_dir) else {
        return out;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !is_executable(&path) {
            tracing::trace!("skipping non-executable action: {}", path.display());
            continue;
        }
        out.push(ActionScript {
            name: file_stem(&path),
            command: path,
        });
    }
    out
}

fn list_views(dir: &Path) -> Vec<ViewBundle> {
    let views_dir = dir.join("views");
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&views_dir) else {
        return out;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !path.join("index.html").exists() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let descriptor = read_view_descriptor(&name, &path);
        out.push(ViewBundle {
            name,
            dir: path,
            descriptor,
        });
    }
    out
}

fn read_view_descriptor(view_name: &str, view_dir: &Path) -> ViewDescriptor {
    let descriptor_path = view_dir.join("view.toml");
    let text = match fs::read_to_string(&descriptor_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return ViewDescriptor::default();
        }
        Err(e) => {
            tracing::warn!(
                "view '{view_name}': failed to read {}: {e}; using default descriptor",
                descriptor_path.display()
            );
            return ViewDescriptor::default();
        }
    };
    match crate::view_metadata::parse_view_toml(&text) {
        Ok(descriptor) => descriptor,
        Err(e) => {
            tracing::warn!(
                "view '{view_name}': invalid {}: {e}; using default descriptor",
                descriptor_path.display()
            );
            ViewDescriptor::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use tempfile::tempdir;

    fn write_executable(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o755)
            .open(path)
            .expect("write exec");
        write!(f, "{body}").unwrap();
    }

    fn write_file(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).expect("write file");
    }

    #[test]
    fn missing_directory_returns_empty() {
        let result = walk(Path::new("/this/path/does/not/exist")).expect("ok");
        assert!(result.is_empty());
    }

    #[test]
    fn empty_directory_returns_empty() {
        let tmp = tempdir().unwrap();
        let result = walk(tmp.path()).expect("ok");
        assert!(result.is_empty());
    }

    #[test]
    fn discovers_full_plugin() {
        let tmp = tempdir().unwrap();
        let plugin = tmp.path().join("moon");
        write_file(
            &plugin.join("config.toml"),
            "[scripts.moon]\ninterval = 60\n",
        );
        write_executable(&plugin.join("scripts/moon"), "#!/bin/sh\necho hi\n");
        write_executable(&plugin.join("actions/open"), "#!/bin/sh\nxdg-open x\n");
        write_file(&plugin.join("views/widget/index.html"), "<html></html>");

        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert_eq!(p.name, "moon");
        assert_eq!(p.polled_scripts.len(), 1);
        assert_eq!(p.polled_scripts[0].channel, "moon.moon");
        assert_eq!(p.idle_scripts.len(), 0);
        assert_eq!(p.actions.len(), 1);
        assert_eq!(p.actions[0].name, "open");
        assert_eq!(p.views.len(), 1);
        assert_eq!(p.views[0].name, "widget");
    }

    #[test]
    fn missing_config_makes_all_scripts_idle() {
        let tmp = tempdir().unwrap();
        let plugin = tmp.path().join("noconf");
        write_executable(&plugin.join("scripts/a"), "#!/bin/sh\n");
        write_executable(&plugin.join("scripts/b"), "#!/bin/sh\n");

        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert_eq!(p.polled_scripts.len(), 0);
        assert_eq!(p.idle_scripts.len(), 2);
    }

    #[test]
    fn malformed_config_skips_plugin_with_warning() {
        let tmp = tempdir().unwrap();
        let plugin = tmp.path().join("broken");
        write_file(&plugin.join("config.toml"), "this is not toml [[[");
        write_executable(&plugin.join("scripts/x"), "#!/bin/sh\n");

        let result = walk(tmp.path()).expect("ok");
        assert!(result.is_empty());
    }

    #[test]
    fn view_without_index_html_is_skipped() {
        let tmp = tempdir().unwrap();
        let plugin = tmp.path().join("v");
        fs::create_dir_all(plugin.join("views/empty")).unwrap();
        write_file(&plugin.join("views/good/index.html"), "<html></html>");

        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert_eq!(p.views.len(), 1);
        assert_eq!(p.views[0].name, "good");
    }

    #[test]
    fn view_with_view_toml_gets_parsed_descriptor() {
        let tmp = tempdir().unwrap();
        let plugin = tmp.path().join("withmeta");
        write_file(&plugin.join("views/bar/index.html"), "<html></html>");
        write_file(
            &plugin.join("views/bar/view.toml"),
            "kind = \"panel\"\nanchor = \"top\"\nheight = 32\nper_monitor = true\n",
        );

        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert_eq!(p.views.len(), 1);
        let view = &p.views[0];
        assert_eq!(view.name, "bar");
        assert_eq!(view.descriptor.kind, quantum_domain::ViewKind::Panel);
        assert_eq!(view.descriptor.anchor, quantum_domain::ViewAnchor::Top);
        assert_eq!(view.descriptor.height, Some(32));
        assert!(view.descriptor.per_monitor);
    }

    #[test]
    fn view_without_view_toml_gets_default_descriptor() {
        let tmp = tempdir().unwrap();
        let plugin = tmp.path().join("nometa");
        write_file(&plugin.join("views/plain/index.html"), "<html></html>");

        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert_eq!(p.views.len(), 1);
        assert_eq!(p.views[0].descriptor, ViewDescriptor::default());
    }

    #[test]
    fn malformed_view_toml_falls_back_to_default_and_view_is_still_discovered() {
        let tmp = tempdir().unwrap();
        let plugin = tmp.path().join("badmeta");
        write_file(&plugin.join("views/broken/index.html"), "<html></html>");
        write_file(&plugin.join("views/broken/view.toml"), "not toml [[[");

        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert_eq!(p.views.len(), 1, "view must still be discovered");
        assert_eq!(p.views[0].name, "broken");
        assert_eq!(p.views[0].descriptor, ViewDescriptor::default());
    }

    #[test]
    fn view_toml_with_invalid_kind_falls_back_to_default_and_view_is_still_discovered() {
        let tmp = tempdir().unwrap();
        let plugin = tmp.path().join("badkind");
        write_file(&plugin.join("views/odd/index.html"), "<html></html>");
        write_file(&plugin.join("views/odd/view.toml"), "kind = \"banana\"\n");

        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert_eq!(p.views.len(), 1, "view must still be discovered");
        assert_eq!(p.views[0].descriptor, ViewDescriptor::default());
    }

    #[test]
    fn hidden_subfolders_are_ignored() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".cache")).unwrap();
        fs::create_dir_all(tmp.path().join("real")).unwrap();
        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "real");
    }

    #[test]
    fn non_executable_script_is_skipped() {
        let tmp = tempdir().unwrap();
        let plugin = tmp.path().join("p");
        write_file(&plugin.join("scripts/notexec"), "echo x");

        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert_eq!(p.polled_scripts.len(), 0);
        assert_eq!(p.idle_scripts.len(), 0);
    }

    #[test]
    fn manifest_channel_override_wins() {
        let tmp = tempdir().unwrap();
        let plugin = tmp.path().join("ovr");
        write_file(
            &plugin.join("config.toml"),
            "[scripts.s]\ninterval = 60\nchannel = \"custom.channel\"\n",
        );
        write_executable(&plugin.join("scripts/s"), "#!/bin/sh\n");

        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert_eq!(p.polled_scripts.len(), 1);
        assert_eq!(p.polled_scripts[0].channel, "custom.channel");
    }

    #[test]
    fn channel_collision_first_wins() {
        let tmp = tempdir().unwrap();

        // Plugin "alpha" claims channel "weather.event".
        let alpha = tmp.path().join("alpha");
        write_file(
            &alpha.join("config.toml"),
            "[scripts.weather]\ninterval = 60\nchannel = \"weather.event\"\n",
        );
        fs::create_dir_all(alpha.join("scripts")).unwrap();
        write_executable(&alpha.join("scripts/weather"), "#!/bin/sh\n");

        // Plugin "beta" ALSO claims channel "weather.event".
        let beta = tmp.path().join("beta");
        write_file(
            &beta.join("config.toml"),
            "[scripts.weather]\ninterval = 60\nchannel = \"weather.event\"\n",
        );
        fs::create_dir_all(beta.join("scripts")).unwrap();
        write_executable(&beta.join("scripts/weather"), "#!/bin/sh\n");

        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 2);

        let alpha_p = result
            .iter()
            .find(|p| p.name == "alpha")
            .expect("alpha present");
        let beta_p = result
            .iter()
            .find(|p| p.name == "beta")
            .expect("beta present");

        assert_eq!(alpha_p.polled_scripts.len(), 1);
        assert_eq!(alpha_p.idle_scripts.len(), 0);
        assert_eq!(beta_p.polled_scripts.len(), 0);
        assert_eq!(beta_p.idle_scripts.len(), 1);
        assert_eq!(beta_p.idle_scripts[0].channel, "weather.event");
    }

    #[test]
    fn malformed_script_does_not_drop_plugin() {
        let tmp = tempdir().unwrap();
        let plugin = tmp.path().join("mixed");
        // 'bad' has sub-minimum interval; 'good' is fine.
        write_file(
            &plugin.join("config.toml"),
            "[scripts.bad]\ninterval = 1\n[scripts.good]\ninterval = 60\n",
        );
        write_executable(&plugin.join("scripts/bad"), "#!/bin/sh\n");
        write_executable(&plugin.join("scripts/good"), "#!/bin/sh\n");
        write_executable(&plugin.join("actions/open"), "#!/bin/sh\nxdg-open x\n");
        write_file(&plugin.join("views/widget/index.html"), "<html></html>");

        let result = walk(tmp.path()).expect("plugin must still be discovered");
        assert_eq!(result.len(), 1, "plugin must not be skipped");
        let p = &result[0];
        assert_eq!(p.name, "mixed");

        // The 'good' script is polled, the 'bad' script is downgraded to idle
        // (because the manifest now has no entry for it).
        assert_eq!(p.polled_scripts.len(), 1, "good script is polled");
        assert_eq!(p.polled_scripts[0].channel, "mixed.good");
        assert_eq!(p.idle_scripts.len(), 1, "bad script falls through to idle");
        assert_eq!(p.idle_scripts[0].channel, "mixed.bad");

        // Action and view must still be present.
        assert_eq!(p.actions.len(), 1, "action must be discovered");
        assert_eq!(p.actions[0].name, "open");
        assert_eq!(p.views.len(), 1, "view must be discovered");
        assert_eq!(p.views[0].name, "widget");
    }

    #[test]
    fn distinct_channels_do_not_collide() {
        let tmp = tempdir().unwrap();
        let a = tmp.path().join("a");
        write_file(&a.join("config.toml"), "[scripts.foo]\ninterval = 60\n");
        fs::create_dir_all(a.join("scripts")).unwrap();
        write_executable(&a.join("scripts/foo"), "#!/bin/sh\n");

        let b = tmp.path().join("b");
        write_file(&b.join("config.toml"), "[scripts.bar]\ninterval = 60\n");
        fs::create_dir_all(b.join("scripts")).unwrap();
        write_executable(&b.join("scripts/bar"), "#!/bin/sh\n");

        let result = walk(tmp.path()).expect("ok");
        assert_eq!(result.len(), 2);
        for p in &result {
            assert_eq!(
                p.polled_scripts.len(),
                1,
                "{} should keep its polled script",
                p.name
            );
        }
    }
}
