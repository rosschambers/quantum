//! Resolve a StatusNotifierItem's icon to a [`quantum_domain::IconRef`].
//!
//! StatusNotifierItem exposes an icon three ways, in descending preference:
//! a freedesktop icon name (`IconName`), an application-private theme
//! directory (`IconThemePath`) holding raster or vector files, and inline
//! ARGB32 pixmaps (`IconPixmap`). This module walks that priority chain.
//!
//! The theme-path branch inlines files as data URIs rather than returning a
//! path because the webview only serves filesystem icons under the standard
//! XDG icon roots (see `allowed_icon_roots` in
//! `src/ui/host/src/scheme.rs`). Applications such as Steam ship an
//! `IconThemePath` outside those roots, so an [`IconRef::Path`] to such a file
//! would be rejected by the scheme handler; encoding the bytes into an
//! [`IconRef::DataUri`] sidesteps the restriction entirely.

use std::path::{Path, PathBuf};

use base64::Engine;

use super::pixmap::{best_pixmap, pixmap_to_data_uri};

/// Resolve an icon for a StatusNotifierItem using the real freedesktop theme
/// lookup for the highest-priority branch.
///
/// See [`resolve_icon_with`] for the full priority chain; this wrapper simply
/// injects the environment-dependent theme lookup so the chain itself stays
/// unit-testable.
pub fn resolve_icon(
    icon_name: &str,
    icon_theme_path: &str,
    pixmaps: &[(i32, i32, Vec<u8>)],
) -> Option<quantum_domain::IconRef> {
    resolve_icon_with(icon_name, icon_theme_path, pixmaps, |name| {
        freedesktop_icons::lookup(name).with_size(48).find()
    })
}

/// Resolve an icon with the theme lookup injected as `theme_lookup`.
///
/// Priority chain:
/// 1. A non-empty `icon_name` that `theme_lookup` resolves to a path becomes
///    an [`IconRef::Path`].
/// 2. Otherwise a non-empty `icon_name` plus a non-empty `icon_theme_path`
///    are matched against `<icon_theme_path>/<icon_name>.{png,svg}`, including
///    one level of subdirectories under `icon_theme_path`; the first existing
///    file's bytes are inlined as an [`IconRef::DataUri`].
/// 3. Otherwise the best inline pixmap is encoded as an [`IconRef::DataUri`].
/// 4. Otherwise [`None`].
///
/// Every filesystem call is guarded: a missing or unreadable path falls
/// through to the next priority rather than panicking.
fn resolve_icon_with(
    icon_name: &str,
    icon_theme_path: &str,
    pixmaps: &[(i32, i32, Vec<u8>)],
    theme_lookup: impl Fn(&str) -> Option<PathBuf>,
) -> Option<quantum_domain::IconRef> {
    if !icon_name.is_empty() {
        if let Some(found) = theme_lookup(icon_name) {
            return Some(quantum_domain::IconRef::Path(found));
        }

        if !icon_theme_path.is_empty() {
            if let Some(data_uri) = theme_path_data_uri(icon_name, icon_theme_path) {
                return Some(quantum_domain::IconRef::DataUri(data_uri));
            }
        }
    }

    if let Some((width, height, bytes)) = best_pixmap(pixmaps) {
        if let Some(data_uri) = pixmap_to_data_uri(*width, *height, bytes) {
            return Some(quantum_domain::IconRef::DataUri(data_uri));
        }
    }

    None
}

/// Search `icon_theme_path` for `<icon_name>.{png,svg}` and inline the first
/// match as a data URI.
///
/// Candidates are generated deterministically: for each extension in
/// `["png", "svg"]` the direct file `<icon_theme_path>/<icon_name>.<ext>` is
/// checked first, then the same file inside each immediate subdirectory of
/// `icon_theme_path`. The first readable file wins; its MIME type is chosen by
/// extension. Returns [`None`] when nothing matches or the bytes cannot be
/// read.
fn theme_path_data_uri(icon_name: &str, icon_theme_path: &str) -> Option<String> {
    let root = Path::new(icon_theme_path);

    for extension in ["png", "svg"] {
        let file_name = format!("{icon_name}.{extension}");

        let direct = root.join(&file_name);
        if let Some(data_uri) = read_as_data_uri(&direct, extension) {
            return Some(data_uri);
        }

        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let subdirectory = entry.path();
            if !subdirectory.is_dir() {
                continue;
            }
            let nested = subdirectory.join(&file_name);
            if let Some(data_uri) = read_as_data_uri(&nested, extension) {
                return Some(data_uri);
            }
        }
    }

    None
}

/// Read `path` and encode it as a base64 data URI for the given `extension`,
/// or [`None`] if the file cannot be read.
fn read_as_data_uri(path: &Path, extension: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let payload = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mime = match extension {
        "svg" => "image/svg+xml",
        _ => "image/png",
    };
    Some(format!("data:{mime};base64,{payload}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_uri(icon: &quantum_domain::IconRef) -> &str {
        match icon {
            quantum_domain::IconRef::DataUri(value) => value.as_str(),
            other => panic!("expected DataUri, got {other:?}"),
        }
    }

    fn no_theme_match(_name: &str) -> Option<PathBuf> {
        None
    }

    #[test]
    fn priority_one_theme_lookup_wins_over_theme_path_and_pixmaps() {
        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::write(directory.path().join("steam.png"), [1u8, 2, 3, 4]).expect("write file");
        let theme_path = directory.path().to_string_lossy().into_owned();
        let pixmaps = vec![(2, 2, vec![0u8; 16])];
        let found = PathBuf::from("/usr/share/icons/hicolor/48x48/apps/steam.png");
        let found_for_stub = found.clone();

        let resolved = resolve_icon_with("steam", &theme_path, &pixmaps, move |_| {
            Some(found_for_stub.clone())
        })
        .expect("resolved icon");

        match resolved {
            quantum_domain::IconRef::Path(path) => assert_eq!(path, found),
            other => panic!("expected Path, got {other:?}"),
        }
    }

    #[test]
    fn priority_two_direct_png_file_becomes_data_uri() {
        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::write(directory.path().join("steam.png"), [1u8, 2, 3, 4]).expect("write file");
        let theme_path = directory.path().to_string_lossy().into_owned();

        let resolved =
            resolve_icon_with("steam", &theme_path, &[], no_theme_match).expect("resolved icon");

        assert!(data_uri(&resolved).starts_with("data:image/png;base64,"));
    }

    #[test]
    fn priority_two_nested_svg_file_becomes_data_uri() {
        let directory = tempfile::tempdir().expect("tempdir");
        let nested = directory.path().join("apps");
        std::fs::create_dir(&nested).expect("create subdirectory");
        std::fs::write(nested.join("steam.svg"), b"<svg></svg>").expect("write file");
        let theme_path = directory.path().to_string_lossy().into_owned();

        let resolved =
            resolve_icon_with("steam", &theme_path, &[], no_theme_match).expect("resolved icon");

        assert!(data_uri(&resolved).starts_with("data:image/svg+xml;base64,"));
    }

    #[test]
    fn priority_three_falls_back_to_pixmap() {
        let pixmaps = vec![(2, 2, vec![0u8; 16])];

        let resolved = resolve_icon_with("", "", &pixmaps, no_theme_match).expect("resolved icon");

        assert!(data_uri(&resolved).starts_with("data:image/png;base64,"));
    }

    #[test]
    fn nothing_resolves_to_none() {
        let resolved = resolve_icon_with("", "", &[], no_theme_match);

        assert!(resolved.is_none());
    }
}
