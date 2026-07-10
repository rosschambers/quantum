//! File explorer domain types.
//! Pure serde-friendly data transfer objects and classification helpers that
//! cross the IPC boundary. No imports from other workspace crates and no
//! input/output.

use serde::{Deserialize, Serialize};

/// The kind of thing a directory entry points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileEntryKind {
    Directory,
    File,
    Symlink,
}

/// A coarse permission category used to colour or badge an entry in the
/// explorer. Derived from ownership and mode bits by [`classify_permissions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionClass {
    Executable,
    ReadOnly,
    RootOwned,
    Normal,
}

/// A coarse content category derived from a file's extension by
/// [`content_kind_for_name`], used to pick an icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Image,
    Document,
    Code,
    Archive,
    Music,
    Other,
}

/// A single directory entry as presented to the explorer frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub kind: FileEntryKind,
    pub size: u64,
    /// Total size of a directory's contents, computed lazily. `None` until
    /// requested or for non-directory entries.
    pub recursive_size: Option<u64>,
    pub modified_epoch_seconds: i64,
    pub owner: String,
    /// The nine-character `rwxrwxrwx` permission string.
    pub permissions: String,
    pub permission_class: PermissionClass,
    /// The target path when [`FileEntry::kind`] is [`FileEntryKind::Symlink`].
    pub symlink_target: Option<String>,
    pub content_kind: ContentKind,
}

/// A mounted drive or volume shown in the explorer sidebar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriveInfo {
    pub label: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// Classify an entry into a [`PermissionClass`] from ownership and mode bits.
///
/// Root ownership always wins first. For regular files an execute bit means
/// [`PermissionClass::Executable`], otherwise a missing user-write bit means
/// [`PermissionClass::ReadOnly`], otherwise [`PermissionClass::Normal`].
/// Directories always carry the execute bit, so they are never classified as
/// [`PermissionClass::Executable`]; a writable directory is
/// [`PermissionClass::Normal`] and a non-writable one is
/// [`PermissionClass::ReadOnly`].
pub fn classify_permissions(
    owner_is_root: bool,
    mode_user_write: bool,
    mode_any_execute: bool,
    is_directory: bool,
) -> PermissionClass {
    if owner_is_root {
        return PermissionClass::RootOwned;
    }
    if !is_directory && mode_any_execute {
        return PermissionClass::Executable;
    }
    if !mode_user_write {
        return PermissionClass::ReadOnly;
    }
    PermissionClass::Normal
}

/// Classify a file into a [`ContentKind`] from its name's extension. The match
/// is case-insensitive; an unknown or absent extension yields
/// [`ContentKind::Other`].
pub fn content_kind_for_name(name: &str) -> ContentKind {
    let extension = match name.rsplit_once('.') {
        Some((_, extension)) => extension.to_ascii_lowercase(),
        None => return ContentKind::Other,
    };
    match extension.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" => ContentKind::Image,
        "md" | "txt" | "pdf" | "odt" | "docx" => ContentKind::Document,
        "rs" | "ts" | "js" | "svelte" | "nix" | "toml" | "json" | "sh" | "py" | "css" | "html" => {
            ContentKind::Code
        }
        "zip" | "tar" | "gz" | "zst" | "xz" | "7z" | "rar" => ContentKind::Archive,
        "mp3" | "flac" | "ogg" | "wav" | "opus" => ContentKind::Music,
        _ => ContentKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_entry_kind_serializes_snake_case() {
        let value = serde_json::to_string(&FileEntryKind::Symlink).expect("serde");
        assert_eq!(value, "\"symlink\"");
    }

    #[test]
    fn permission_class_serializes_snake_case() {
        let value = serde_json::to_string(&PermissionClass::RootOwned).expect("serde");
        assert_eq!(value, "\"root_owned\"");
    }

    #[test]
    fn content_kind_serializes_snake_case() {
        let value = serde_json::to_string(&ContentKind::Music).expect("serde");
        assert_eq!(value, "\"music\"");
    }

    #[test]
    fn file_entry_round_trips_through_serde() {
        let entry = FileEntry {
            name: "photo.jpg".to_string(),
            path: "/home/user/photo.jpg".to_string(),
            kind: FileEntryKind::File,
            size: 2048,
            recursive_size: None,
            modified_epoch_seconds: 1_700_000_000,
            owner: "user".to_string(),
            permissions: "rw-r--r--".to_string(),
            permission_class: PermissionClass::Normal,
            symlink_target: None,
            content_kind: ContentKind::Image,
        };
        let json = serde_json::to_value(&entry).expect("serialize");
        assert_eq!(json["name"], "photo.jpg");
        assert_eq!(json["kind"], "file");
        assert_eq!(json["permission_class"], "normal");
        assert_eq!(json["content_kind"], "image");
        let back: FileEntry = serde_json::from_value(json).expect("round trip");
        assert_eq!(back, entry);
    }

    #[test]
    fn drive_info_round_trips_through_serde() {
        let drive = DriveInfo {
            label: "root".to_string(),
            mount_point: "/".to_string(),
            total_bytes: 1_000_000,
            free_bytes: 250_000,
        };
        let json = serde_json::to_value(&drive).expect("serialize");
        let back: DriveInfo = serde_json::from_value(json).expect("round trip");
        assert_eq!(back, drive);
    }

    #[test]
    fn classify_permissions_root_owner_wins_first() {
        // Root ownership takes precedence over every other signal, including an
        // executable, writable regular file.
        let class = classify_permissions(true, true, true, false);
        assert_eq!(class, PermissionClass::RootOwned);
        // Root ownership wins for directories too.
        let directory = classify_permissions(true, true, true, true);
        assert_eq!(directory, PermissionClass::RootOwned);
    }

    #[test]
    fn classify_permissions_directory_is_never_executable() {
        // Directories always have the execute bit set but must not classify as
        // Executable. A writable, executable directory is Normal.
        let class = classify_permissions(false, true, true, true);
        assert_eq!(class, PermissionClass::Normal);
    }

    #[test]
    fn classify_permissions_read_only_directory() {
        let class = classify_permissions(false, false, true, true);
        assert_eq!(class, PermissionClass::ReadOnly);
    }

    #[test]
    fn classify_permissions_executable_file() {
        let class = classify_permissions(false, true, true, false);
        assert_eq!(class, PermissionClass::Executable);
    }

    #[test]
    fn classify_permissions_read_only_file() {
        let class = classify_permissions(false, false, false, false);
        assert_eq!(class, PermissionClass::ReadOnly);
    }

    #[test]
    fn classify_permissions_normal_file() {
        let class = classify_permissions(false, true, false, false);
        assert_eq!(class, PermissionClass::Normal);
    }

    #[test]
    fn content_kind_maps_every_category() {
        assert_eq!(content_kind_for_name("holiday.JPG"), ContentKind::Image);
        assert_eq!(content_kind_for_name("diagram.svg"), ContentKind::Image);
        assert_eq!(content_kind_for_name("README.md"), ContentKind::Document);
        assert_eq!(content_kind_for_name("report.PDF"), ContentKind::Document);
        assert_eq!(content_kind_for_name("main.rs"), ContentKind::Code);
        assert_eq!(content_kind_for_name("app.svelte"), ContentKind::Code);
        assert_eq!(content_kind_for_name("backup.tar"), ContentKind::Archive);
        assert_eq!(content_kind_for_name("bundle.7z"), ContentKind::Archive);
        assert_eq!(content_kind_for_name("track.FLAC"), ContentKind::Music);
        assert_eq!(content_kind_for_name("voice.opus"), ContentKind::Music);
    }

    #[test]
    fn content_kind_is_case_insensitive() {
        assert_eq!(content_kind_for_name("PHOTO.PnG"), ContentKind::Image);
    }

    #[test]
    fn content_kind_unknown_extension_is_other() {
        assert_eq!(content_kind_for_name("mystery.xyz"), ContentKind::Other);
        assert_eq!(content_kind_for_name("no_extension"), ContentKind::Other);
    }
}
