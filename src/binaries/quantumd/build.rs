//! Stage first-party plugin view bundles into `$OUT_DIR/embedded-plugins`
//! so `main.rs` can compile them in with `include_dir!`.
//!
//! Why staging instead of `include_dir!("$CARGO_MANIFEST_DIR/../../ui/plugins")`:
//! `include_dir` 0.7 follows directory symlinks and offers no exclusion
//! filter, so pointing it at the plugin sources would embed every view's
//! pnpm `node_modules` symlink farm (tens of megabytes per view) plus
//! Svelte sources and tooling configs. Staging copies only what
//! `quantum_plugins::walk_embedded` consumes: each view's `view.toml`
//! and its built `dist/` output.
//!
//! Constraint: embedded views require their `dist/` to exist at compile
//! time. Build them first, for each plugin:
//! `pnpm -C src/ui/plugins/<name>/views/<name> build`
//! A view without `dist/` is skipped with a cargo warning; the daemon
//! still builds, it just ships without that embedded view.

use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let out_dir = std::env::var("OUT_DIR")?;
    let plugins_source = Path::new(&manifest_dir).join("../../ui/plugins");
    let staging_root = Path::new(&out_dir).join("embedded-plugins");

    if staging_root.exists() {
        fs::remove_dir_all(&staging_root)?;
    }
    fs::create_dir_all(&staging_root)?;

    if !plugins_source.is_dir() {
        println!(
            "cargo:warning=first-party plugins directory not found at {}; embedding no plugins",
            plugins_source.display()
        );
        return Ok(());
    }

    for plugin_entry in fs::read_dir(&plugins_source)? {
        let plugin_entry = plugin_entry?;
        let plugin_dir = plugin_entry.path();
        let plugin_name = plugin_entry.file_name();
        let plugin_name = plugin_name.to_string_lossy();
        if !plugin_dir.is_dir() || plugin_name.starts_with('.') {
            continue;
        }
        let views_dir = plugin_dir.join("views");
        if !views_dir.is_dir() {
            continue;
        }
        for view_entry in fs::read_dir(&views_dir)? {
            let view_entry = view_entry?;
            let view_dir = view_entry.path();
            let view_name = view_entry.file_name();
            let view_name = view_name.to_string_lossy();
            if !view_dir.is_dir() || view_name.starts_with('.') {
                continue;
            }
            stage_view(&plugin_name, &view_name, &view_dir, &staging_root)?;
        }
    }

    Ok(())
}

/// Copy one view's `view.toml` and `dist/` into the staging tree,
/// preserving the `<plugin>/views/<view>/` layout that
/// `quantum_plugins::walk_embedded` expects. Emits precise
/// `rerun-if-changed` paths so cargo never has to scan the pnpm
/// `node_modules` symlink farms; the cost is that brand-new plugins
/// added under `src/ui/plugins/` need one manual rebuild to be noticed.
fn stage_view(
    plugin_name: &str,
    view_name: &str,
    view_dir: &Path,
    staging_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let dist_dir = view_dir.join("dist");
    let view_toml = view_dir.join("view.toml");
    println!("cargo:rerun-if-changed={}", dist_dir.display());
    println!("cargo:rerun-if-changed={}", view_toml.display());

    if !dist_dir.join("index.html").is_file() {
        println!(
            "cargo:warning=embedded view '{plugin_name}/{view_name}' has no dist/index.html; \
             run `pnpm -C src/ui/plugins/{plugin_name}/views/{view_name} build` first; skipping"
        );
        return Ok(());
    }

    let staged_view_dir = staging_root.join(plugin_name).join("views").join(view_name);
    fs::create_dir_all(&staged_view_dir)?;
    if view_toml.is_file() {
        fs::copy(&view_toml, staged_view_dir.join("view.toml"))?;
    }
    copy_directory_recursive(&dist_dir, &staged_view_dir.join("dist"))?;
    Ok(())
}

fn copy_directory_recursive(
    source: &Path,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory_recursive(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}
