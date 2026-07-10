//! `ApplicationCatalog` implementation: the applications offered by the
//! explorer's "Open with" menu.
//!
//! `DesktopApplicationCatalog` scans the XDG `applications` directories for
//! `.desktop` entries, keeps only the displayable ones, and returns them as
//! domain [`ApplicationInfo`] values. The scan touches the filesystem, so
//! `list_applications` runs it on a blocking thread; a scan failure for any one
//! directory or file is skipped rather than propagated, because a missing or
//! unreadable applications directory must never take down the menu.

use std::collections::HashSet;

use async_trait::async_trait;
use quantum_domain::{ApplicationCatalog, ApplicationInfo};

use crate::opener::desktop_file_search_dirs;

/// An [`ApplicationCatalog`] backed by a scan of the XDG desktop-entry
/// directories.
#[derive(Debug, Default, Clone)]
pub struct DesktopApplicationCatalog;

impl DesktopApplicationCatalog {
    /// Construct a catalog. The scan reads the environment at call time, so no
    /// configuration is captured here.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ApplicationCatalog for DesktopApplicationCatalog {
    async fn list_applications(&self) -> Vec<ApplicationInfo> {
        tokio::task::spawn_blocking(scan_applications)
            .await
            .unwrap_or_default()
    }
}

/// Scan the XDG applications directories for displayable desktop entries.
///
/// Directories are visited in XDG precedence order; the first entry seen for a
/// given identifier wins, so a per-user override shadows a system entry. The
/// result is de-duplicated by identifier and sorted by display name.
fn scan_applications() -> Vec<ApplicationInfo> {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg_data_home = std::env::var("XDG_DATA_HOME").ok();
    let xdg_data_dirs = std::env::var("XDG_DATA_DIRS").ok();
    let dirs = desktop_file_search_dirs(xdg_data_home.as_deref(), xdg_data_dirs.as_deref(), &home);

    let mut seen: HashSet<String> = HashSet::new();
    let mut applications: Vec<ApplicationInfo> = Vec::new();

    for directory in &dirs {
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            let identifier = match file_name.strip_suffix(".desktop") {
                Some(stem) => stem.to_string(),
                None => continue,
            };
            if seen.contains(&identifier) {
                continue;
            }
            let contents = match std::fs::read_to_string(entry.path()) {
                Ok(contents) => contents,
                Err(_) => continue,
            };
            if let Some((name, displayable)) = parse_desktop_entry(&contents) {
                if !displayable {
                    // Mark the identifier as seen so a lower-precedence entry
                    // for the same application does not resurrect it.
                    seen.insert(identifier);
                    continue;
                }
                seen.insert(identifier.clone());
                applications.push(ApplicationInfo {
                    id: identifier,
                    name,
                });
            }
        }
    }

    applications.sort_by(|a, b| a.name.cmp(&b.name));
    applications
}

/// Parse the `[Desktop Entry]` group of a desktop-entry file.
///
/// Returns `Some((name, displayable))` when the file has a `[Desktop Entry]`
/// group carrying a `Name`. `displayable` is true only when the entry is an
/// `Application` that is neither `NoDisplay=true` nor `Hidden=true`. Returns
/// `None` when there is no `[Desktop Entry]` group or it lacks a `Name`. Keys
/// outside the `[Desktop Entry]` group are ignored, and the first occurrence of
/// each key wins.
pub fn parse_desktop_entry(content: &str) -> Option<(String, bool)> {
    let mut in_group = false;
    let mut name: Option<String> = None;
    let mut entry_type: Option<String> = None;
    let mut no_display = false;
    let mut hidden = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "Name" if name.is_none() => name = Some(value.to_string()),
            "Type" if entry_type.is_none() => entry_type = Some(value.to_string()),
            "NoDisplay" => no_display = value == "true",
            "Hidden" => hidden = value == "true",
            _ => {}
        }
    }

    let name = name?;
    let is_application = entry_type.as_deref() == Some("Application");
    let displayable = is_application && !no_display && !hidden;
    Some((name, displayable))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_normal_entry_yields_name_and_displayable() {
        let content = "\
[Desktop Entry]
Type=Application
Name=Firefox
Exec=firefox %u
";
        assert_eq!(
            parse_desktop_entry(content),
            Some(("Firefox".to_string(), true))
        );
    }

    #[test]
    fn parse_no_display_entry_is_not_displayable() {
        let content = "\
[Desktop Entry]
Type=Application
Name=Hidden Helper
NoDisplay=true
";
        assert_eq!(
            parse_desktop_entry(content),
            Some(("Hidden Helper".to_string(), false))
        );
    }

    #[test]
    fn parse_hidden_entry_is_not_displayable() {
        let content = "\
[Desktop Entry]
Type=Application
Name=Removed
Hidden=true
";
        assert_eq!(
            parse_desktop_entry(content),
            Some(("Removed".to_string(), false))
        );
    }

    #[test]
    fn parse_link_type_entry_is_not_displayable() {
        let content = "\
[Desktop Entry]
Type=Link
Name=Homepage
URL=https://example.com
";
        assert_eq!(
            parse_desktop_entry(content),
            Some(("Homepage".to_string(), false))
        );
    }

    #[test]
    fn parse_file_without_group_yields_none() {
        let content = "Name=Orphan\nType=Application\n";
        assert_eq!(parse_desktop_entry(content), None);
    }

    #[test]
    fn parse_file_without_name_yields_none() {
        let content = "[Desktop Entry]\nType=Application\nExec=thing\n";
        assert_eq!(parse_desktop_entry(content), None);
    }

    #[test]
    fn parse_ignores_keys_outside_the_desktop_entry_group() {
        let content = "\
[Desktop Entry]
Type=Application
Name=Editor

[Desktop Action new]
Name=New Window
";
        assert_eq!(
            parse_desktop_entry(content),
            Some(("Editor".to_string(), true))
        );
    }
}
