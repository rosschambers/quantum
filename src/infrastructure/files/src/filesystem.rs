//! `FileSystemPort` implementation: directory listing and metadata reads.
//!
//! `LocalFileSystem` reads the host filesystem through the standard library.
//! Blocking `std::fs` work runs on Tokio's blocking pool so a large synchronous
//! walk never parks the async runtime. User names are resolved by reading
//! `/etc/passwd` directly; modification times come from the unix `mtime` seconds
//! exposed by [`std::os::unix::fs::MetadataExt`].

use async_trait::async_trait;
use quantum_domain::{
    classify_permissions, content_kind_for_name, DriveInfo, FileEntry, FileEntryKind,
    FileOperation, FileSystemPort, FilesError,
};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::operations;
use crate::operations::map_io_error;

/// A [`FileSystemPort`] backed by the local host filesystem.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFileSystem;

impl LocalFileSystem {
    /// Construct a new local filesystem adapter.
    pub fn new() -> Self {
        Self
    }
}

/// Build the nine-character `rwxrwxrwx` permission string from unix mode bits.
fn permission_string(mode: u32) -> String {
    const FLAGS: [(u32, char); 9] = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    FLAGS
        .iter()
        .map(
            |(bit, character)| {
                if mode & bit != 0 {
                    *character
                } else {
                    '-'
                }
            },
        )
        .collect()
}

/// Resolve a numeric user id to a user name by reading `/etc/passwd`. Falls back
/// to the numeric id rendered as a string when the id is absent or the file is
/// unreadable.
fn resolve_owner(uid: u32) -> String {
    match owner_from_passwd(uid) {
        Some(name) => name,
        None => uid.to_string(),
    }
}

/// Look up a user name for `uid` in `/etc/passwd`. Each line is
/// `name:password:uid:gid:...`; the first line whose uid field matches wins.
fn owner_from_passwd(uid: u32) -> Option<String> {
    let contents = std::fs::read_to_string("/etc/passwd").ok()?;
    for line in contents.lines() {
        let mut fields = line.split(':');
        let name = match fields.next() {
            Some(name) => name,
            None => continue,
        };
        // Skip the password placeholder field.
        if fields.next().is_none() {
            continue;
        }
        let entry_uid = match fields.next() {
            Some(field) => field,
            None => continue,
        };
        if entry_uid.parse::<u32>() == Ok(uid) {
            return Some(name.to_string());
        }
    }
    None
}

/// Build a [`FileEntry`] for `path` from its own metadata. Uses
/// `symlink_metadata` so a symlink is classified as a symlink rather than
/// followed to its target.
fn entry_from_path(path: &Path) -> Result<FileEntry, FilesError> {
    let path_string = path.to_string_lossy().to_string();
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| map_io_error(&path_string, &error))?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_symlink() {
        FileEntryKind::Symlink
    } else if file_type.is_dir() {
        FileEntryKind::Directory
    } else {
        FileEntryKind::File
    };

    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path_string.clone());

    let mode = metadata.mode();
    let uid = metadata.uid();
    let is_directory = matches!(kind, FileEntryKind::Directory);
    let permission_class =
        classify_permissions(uid == 0, mode & 0o200 != 0, mode & 0o111 != 0, is_directory);

    let symlink_target = if matches!(kind, FileEntryKind::Symlink) {
        std::fs::read_link(path)
            .ok()
            .map(|target| target.to_string_lossy().to_string())
    } else {
        None
    };

    Ok(FileEntry {
        content_kind: content_kind_for_name(&name),
        name,
        path: path_string,
        kind,
        size: metadata.len(),
        recursive_size: None,
        modified_epoch_seconds: metadata.mtime(),
        owner: resolve_owner(uid),
        permissions: permission_string(mode),
        permission_class,
        symlink_target,
    })
}

/// List a directory synchronously. Errors with [`FilesError::NotFound`] for a
/// missing path and [`FilesError::Unsupported`] for a path that is not a
/// directory.
fn list_directory_blocking(path: PathBuf) -> Result<Vec<FileEntry>, FilesError> {
    let path_string = path.to_string_lossy().to_string();
    // `metadata` follows symlinks so a symlink to a directory can be listed.
    let metadata = std::fs::metadata(&path).map_err(|error| map_io_error(&path_string, &error))?;
    if !metadata.is_dir() {
        return Err(FilesError::Unsupported(format!(
            "not a directory: {path_string}"
        )));
    }
    let reader = std::fs::read_dir(&path).map_err(|error| map_io_error(&path_string, &error))?;
    let mut entries = Vec::new();
    for item in reader {
        let item = item.map_err(|error| map_io_error(&path_string, &error))?;
        entries.push(entry_from_path(&item.path())?);
    }
    Ok(entries)
}

/// Read at most `max_bytes` bytes of a file and decode them lossily as UTF-8.
fn read_text_preview_blocking(path: PathBuf, max_bytes: usize) -> Result<String, FilesError> {
    let path_string = path.to_string_lossy().to_string();
    let file = std::fs::File::open(&path).map_err(|error| map_io_error(&path_string, &error))?;
    let mut buffer = Vec::new();
    file.take(max_bytes as u64)
        .read_to_end(&mut buffer)
        .map_err(|error| map_io_error(&path_string, &error))?;
    Ok(String::from_utf8_lossy(&buffer).to_string())
}

/// Decode the image at `path`, downscale it so its longest edge is at most
/// `max_dimension` (never upscaling), re-encode it as PNG in memory, and return
/// a base64 `data:` URI. A missing path yields [`FilesError::NotFound`]; a file
/// that cannot be decoded as an image yields [`FilesError::Unsupported`]. This
/// is CPU-bound and synchronous, so callers run it on the blocking pool.
fn read_image_preview_blocking(path: PathBuf, max_dimension: u32) -> Result<String, FilesError> {
    use base64::Engine as _;
    use image::GenericImageView as _;

    let path_string = path.to_string_lossy().to_string();
    if !path.exists() {
        return Err(FilesError::NotFound(path_string));
    }
    let image = image::open(&path)
        .map_err(|error| FilesError::Unsupported(format!("{path_string}: {error}")))?;

    let (width, height) = image.dimensions();
    // `resize` preserves aspect ratio and fits the image inside the box, so a
    // square box clamps the longest edge. Only downscale, never enlarge.
    let scaled = if width.max(height) > max_dimension {
        image.resize(
            max_dimension,
            max_dimension,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };

    let mut buffer = std::io::Cursor::new(Vec::new());
    scaled
        .write_to(&mut buffer, image::ImageFormat::Png)
        .map_err(|error| FilesError::Unsupported(format!("{path_string}: {error}")))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(buffer.get_ref());
    Ok(format!("data:image/png;base64,{encoded}"))
}

/// Walk `root` recursively without following symlinks, collecting up to `limit`
/// entries whose file name contains `query` case-insensitively.
fn search_blocking(
    root: PathBuf,
    query: String,
    limit: usize,
) -> Result<Vec<FileEntry>, FilesError> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();
    let mut stack = vec![root];
    while let Some(directory) = stack.pop() {
        if results.len() >= limit {
            break;
        }
        let reader = match std::fs::read_dir(&directory) {
            Ok(reader) => reader,
            Err(_) => continue,
        };
        for item in reader {
            if results.len() >= limit {
                break;
            }
            let item = match item {
                Ok(item) => item,
                Err(_) => continue,
            };
            let entry_path = item.path();
            let name = item.file_name().to_string_lossy().to_string();
            if name.to_lowercase().contains(&query_lower) {
                if let Ok(entry) = entry_from_path(&entry_path) {
                    results.push(entry);
                }
            }
            // `DirEntry::file_type` does not follow symlinks, so a symlink to a
            // directory is never descended into.
            match item.file_type() {
                Ok(file_type) if file_type.is_dir() => stack.push(entry_path),
                _ => {}
            }
        }
    }
    Ok(results)
}

/// Filesystem types treated as real disks worth listing in the sidebar even
/// when their backing source is not under `/dev/`. Network and remote
/// filesystems (`cifs`, `smb3`, `nfs`, `nfs4`) are included so mounted shares
/// appear in the drives sidebar; `statvfs` works on these and `mounts_blocking`
/// skips any mount whose `statvfs` fails, so a briefly-unavailable network
/// mount is handled gracefully.
const DISK_FILESYSTEM_ALLOWLIST: [&str; 15] = [
    "ext2", "ext3", "ext4", "btrfs", "xfs", "vfat", "exfat", "ntfs", "ntfs3", "f2fs", "zfs",
    "cifs", "smb3", "nfs", "nfs4",
];

/// Decode the octal escapes that `/proc/self/mounts` uses for characters that
/// would otherwise break its space-separated fields (a space becomes `\040`, a
/// tab `\011`, a newline `\012`, a backslash `\134`). Escapes are always three
/// octal digits; any other backslash sequence is left untouched.
fn unescape_mount_field(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        // An escape is a backslash followed by exactly three octal digits.
        if bytes[index] == b'\\' && index + 4 <= bytes.len() {
            let octal = &field[index + 1..index + 4];
            if octal
                .bytes()
                .all(|byte| byte.is_ascii_digit() && byte <= b'7')
            {
                if let Ok(value) = u8::from_str_radix(octal, 8) {
                    output.push(value);
                    index += 4;
                    continue;
                }
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    // Decode after substitution so multibyte paths survive untouched bytes.
    String::from_utf8_lossy(&output).to_string()
}

/// Derive a drive label from a mount point: the last path segment, with the
/// root mount `/` shown as `System`.
fn mount_label(mount_point: &str) -> String {
    if mount_point == "/" {
        return "System".to_string();
    }
    Path::new(mount_point)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| mount_point.to_string())
}

/// Parse `/proc/self/mounts` content into the list of mount points worth
/// showing. A line is kept when its filesystem type is in the disk allowlist or
/// its source begins with `/dev/`; pseudo filesystems such as `proc`, `sysfs`,
/// `tmpfs`, and `cgroup2` are dropped. Mount points are deduplicated in
/// first-seen order and their octal escapes decoded.
fn parse_mounts(content: &str) -> Vec<String> {
    let mut mount_points: Vec<String> = Vec::new();
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let source = match fields.next() {
            Some(source) => source,
            None => continue,
        };
        let mount_point = match fields.next() {
            Some(mount_point) => mount_point,
            None => continue,
        };
        let filesystem_type = match fields.next() {
            Some(filesystem_type) => filesystem_type,
            None => continue,
        };
        let keep =
            DISK_FILESYSTEM_ALLOWLIST.contains(&filesystem_type) || source.starts_with("/dev/");
        if !keep {
            continue;
        }
        let mount_point = unescape_mount_field(mount_point);
        if !mount_points.contains(&mount_point) {
            mount_points.push(mount_point);
        }
    }
    mount_points
}

/// Read `/proc/self/mounts` and resolve each kept mount point's usage through
/// `statvfs`. A mount point whose `statvfs` fails is logged and skipped rather
/// than failing the whole enumeration. Free space uses `f_bavail`
/// (available to unprivileged callers), not `f_bfree`.
fn mounts_blocking() -> Result<Vec<DriveInfo>, FilesError> {
    let content = std::fs::read_to_string("/proc/self/mounts")
        .map_err(|error| FilesError::Io(format!("/proc/self/mounts: {error}")))?;
    let mut drives = Vec::new();
    for mount_point in parse_mounts(&content) {
        let stats = match rustix::fs::statvfs(mount_point.as_str()) {
            Ok(stats) => stats,
            Err(error) => {
                tracing::warn!(mount_point = %mount_point, %error, "statvfs failed for mount point");
                continue;
            }
        };
        let total_bytes = stats.f_blocks.saturating_mul(stats.f_frsize);
        let free_bytes = stats.f_bavail.saturating_mul(stats.f_frsize);
        drives.push(DriveInfo {
            label: mount_label(&mount_point),
            mount_point,
            total_bytes,
            free_bytes,
        });
    }
    Ok(drives)
}

/// Run blocking filesystem work on Tokio's blocking pool, mapping a join
/// failure to [`FilesError::Io`].
async fn run_blocking<T, F>(work: F) -> Result<T, FilesError>
where
    F: FnOnce() -> Result<T, FilesError> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        Err(error) => Err(FilesError::Io(format!(
            "filesystem task join error: {error}"
        ))),
    }
}

#[async_trait]
impl FileSystemPort for LocalFileSystem {
    async fn list_directory(&self, path: &str) -> Result<Vec<FileEntry>, FilesError> {
        let owned = PathBuf::from(path);
        run_blocking(move || list_directory_blocking(owned)).await
    }

    async fn stat(&self, path: &str) -> Result<FileEntry, FilesError> {
        let owned = PathBuf::from(path);
        run_blocking(move || entry_from_path(&owned)).await
    }

    async fn mounts(&self) -> Result<Vec<DriveInfo>, FilesError> {
        run_blocking(mounts_blocking).await
    }

    async fn read_text_preview(&self, path: &str, max_bytes: usize) -> Result<String, FilesError> {
        let owned = PathBuf::from(path);
        run_blocking(move || read_text_preview_blocking(owned, max_bytes)).await
    }

    async fn read_image_preview(
        &self,
        path: &str,
        max_dimension: u32,
    ) -> Result<String, FilesError> {
        let owned = PathBuf::from(path);
        run_blocking(move || read_image_preview_blocking(owned, max_dimension)).await
    }

    async fn perform(&self, operation: FileOperation) -> Result<(), FilesError> {
        match operation {
            FileOperation::Copy {
                sources,
                destination,
            } => run_blocking(move || operations::copy_into(&sources, &destination)).await,
            FileOperation::Move {
                sources,
                destination,
            } => run_blocking(move || operations::move_into(&sources, &destination)).await,
            FileOperation::Rename { path, new_name } => {
                run_blocking(move || operations::rename(&path, &new_name)).await
            }
            FileOperation::Duplicate { path } => {
                run_blocking(move || operations::duplicate(&path)).await
            }
            FileOperation::NewFolder { parent, name } => {
                run_blocking(move || operations::new_folder(&parent, &name)).await
            }
            FileOperation::NewFile { parent, name } => {
                run_blocking(move || operations::new_file(&parent, &name)).await
            }
            FileOperation::Delete { paths } => {
                run_blocking(move || operations::delete(&paths)).await
            }
            FileOperation::Trash { paths } => operations::run_trash(&paths).await,
            FileOperation::Compress { paths, destination } => {
                operations::run_compress(&paths, &destination).await
            }
            FileOperation::Extract { path } => operations::run_extract(&path).await,
        }
    }

    async fn search(
        &self,
        root: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<FileEntry>, FilesError> {
        let owned_root = PathBuf::from(root);
        let owned_query = query.to_string();
        run_blocking(move || search_blocking(owned_root, owned_query, limit)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantum_domain::PermissionClass;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::Path;
    use tempfile::TempDir;

    /// Build a temporary directory populated with a regular file, a dotfile, an
    /// executable file, a read-only file, a subdirectory, and a symlink to the
    /// regular file. Returns the tempdir and the absolute path of the regular
    /// file (the symlink target).
    fn build_fixture() -> (TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let root = dir.path();

        let regular = root.join("notes.txt");
        fs::write(&regular, b"hello world").expect("write regular file");

        fs::write(root.join(".hidden"), b"secret").expect("write dotfile");

        let executable = root.join("run.sh");
        fs::write(&executable, b"#!/bin/sh\n").expect("write executable");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
            .expect("set executable mode");

        let read_only = root.join("readme.md");
        fs::write(&read_only, b"docs").expect("write read-only file");
        fs::set_permissions(&read_only, fs::Permissions::from_mode(0o444))
            .expect("set read-only mode");

        fs::create_dir(root.join("subdir")).expect("create subdirectory");

        symlink(&regular, root.join("link")).expect("create symlink");

        (dir, regular.to_string_lossy().to_string())
    }

    fn find<'a>(entries: &'a [FileEntry], name: &str) -> &'a FileEntry {
        entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("entry {name} not present"))
    }

    #[tokio::test]
    async fn list_directory_returns_every_entry_including_dotfiles() {
        let (dir, target) = build_fixture();
        let filesystem = LocalFileSystem::new();
        let entries = filesystem
            .list_directory(&dir.path().to_string_lossy())
            .await
            .expect("list directory");

        assert_eq!(entries.len(), 6, "expected all six entries");

        assert_eq!(find(&entries, "notes.txt").kind, FileEntryKind::File);
        assert_eq!(find(&entries, ".hidden").kind, FileEntryKind::File);
        assert_eq!(find(&entries, "run.sh").kind, FileEntryKind::File);
        assert_eq!(find(&entries, "readme.md").kind, FileEntryKind::File);
        assert_eq!(find(&entries, "subdir").kind, FileEntryKind::Directory);

        let link = find(&entries, "link");
        assert_eq!(link.kind, FileEntryKind::Symlink);
        assert_eq!(link.symlink_target.as_deref(), Some(target.as_str()));
    }

    #[tokio::test]
    async fn list_directory_classifies_permissions() {
        let (dir, _target) = build_fixture();
        let filesystem = LocalFileSystem::new();
        let entries = filesystem
            .list_directory(&dir.path().to_string_lossy())
            .await
            .expect("list directory");

        assert_eq!(
            find(&entries, "run.sh").permission_class,
            PermissionClass::Executable
        );
        assert_eq!(
            find(&entries, "readme.md").permission_class,
            PermissionClass::ReadOnly
        );
        assert_eq!(
            find(&entries, "notes.txt").permission_class,
            PermissionClass::Normal
        );
        assert_eq!(
            find(&entries, "subdir").permission_class,
            PermissionClass::Normal
        );
    }

    #[tokio::test]
    async fn list_directory_missing_path_is_not_found() {
        let filesystem = LocalFileSystem::new();
        let error = filesystem
            .list_directory("/nonexistent/quantum/path")
            .await
            .expect_err("missing path should error");
        assert!(matches!(error, FilesError::NotFound(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn list_directory_on_a_file_is_unsupported() {
        let (dir, target) = build_fixture();
        let _ = dir;
        let filesystem = LocalFileSystem::new();
        let error = filesystem
            .list_directory(&target)
            .await
            .expect_err("listing a file should error");
        assert!(matches!(error, FilesError::Unsupported(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn stat_returns_a_single_entry() {
        let (dir, target) = build_fixture();
        let _ = dir;
        let filesystem = LocalFileSystem::new();
        let entry = filesystem.stat(&target).await.expect("stat file");
        assert_eq!(entry.name, "notes.txt");
        assert_eq!(entry.kind, FileEntryKind::File);
        assert_eq!(entry.size, "hello world".len() as u64);
    }

    #[tokio::test]
    async fn stat_missing_path_is_not_found() {
        let filesystem = LocalFileSystem::new();
        let error = filesystem
            .stat("/nonexistent/quantum/path")
            .await
            .expect_err("missing path should error");
        assert!(matches!(error, FilesError::NotFound(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn read_text_preview_truncates_to_max_bytes() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let file = dir.path().join("long.txt");
        fs::write(&file, b"abcdefghij").expect("write file");
        let filesystem = LocalFileSystem::new();
        let preview = filesystem
            .read_text_preview(&file.to_string_lossy(), 4)
            .await
            .expect("read preview");
        assert_eq!(preview, "abcd");
    }

    #[tokio::test]
    async fn read_text_preview_missing_path_is_not_found() {
        let filesystem = LocalFileSystem::new();
        let error = filesystem
            .read_text_preview("/nonexistent/quantum/file", 16)
            .await
            .expect_err("missing path should error");
        assert!(matches!(error, FilesError::NotFound(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn search_finds_files_by_case_insensitive_substring() {
        let (dir, _target) = build_fixture();
        let nested = dir.path().join("subdir");
        fs::write(nested.join("Deep-NOTES.log"), b"x").expect("write nested file");
        let filesystem = LocalFileSystem::new();
        let matches = filesystem
            .search(&dir.path().to_string_lossy(), "notes", 100)
            .await
            .expect("search");
        let names: Vec<&str> = matches.iter().map(|entry| entry.name.as_str()).collect();
        assert!(names.contains(&"notes.txt"), "got {names:?}");
        assert!(names.contains(&"Deep-NOTES.log"), "got {names:?}");
    }

    #[tokio::test]
    async fn search_respects_limit() {
        let dir = tempfile::tempdir().expect("create tempdir");
        for index in 0..5 {
            fs::write(dir.path().join(format!("match-{index}.txt")), b"x").expect("write file");
        }
        let filesystem = LocalFileSystem::new();
        let matches = filesystem
            .search(&dir.path().to_string_lossy(), "match", 2)
            .await
            .expect("search");
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn build_fixture_paths_are_absolute() {
        let (dir, target) = build_fixture();
        assert!(Path::new(&target).is_absolute());
        drop(dir);
    }

    #[test]
    fn parse_mounts_keeps_only_real_disks_and_deduplicates() {
        let content = concat!(
            "proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0\n",
            "tmpfs /run tmpfs rw,nosuid,nodev 0 0\n",
            "sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0\n",
            "cgroup2 /sys/fs/cgroup cgroup2 rw,nosuid,nodev,noexec,relatime 0 0\n",
            "/dev/nvme0n1p2 / ext4 rw,relatime 0 0\n",
            "/dev/nvme0n1p1 /boot vfat rw,relatime 0 0\n",
            "/dev/nvme0n1p2 / ext4 rw,relatime 0 0\n",
        );
        let mount_points = parse_mounts(content);
        assert_eq!(mount_points, vec!["/".to_string(), "/boot".to_string()]);
    }

    #[test]
    fn parse_mounts_keeps_network_shares_while_excluding_pseudo_filesystems() {
        let content = concat!(
            "proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0\n",
            "tmpfs /run tmpfs rw,nosuid,nodev 0 0\n",
            "//server/share /share cifs rw,relatime 0 0\n",
            "nfsserver:/export /data nfs4 rw,relatime 0 0\n",
        );
        let mount_points = parse_mounts(content);
        assert!(
            mount_points.contains(&"/share".to_string()),
            "cifs share should be kept, got {mount_points:?}"
        );
        assert!(
            mount_points.contains(&"/data".to_string()),
            "nfs4 mount should be kept, got {mount_points:?}"
        );
        assert!(
            !mount_points.contains(&"/proc".to_string()),
            "pseudo filesystems must still be excluded, got {mount_points:?}"
        );
        assert!(
            !mount_points.contains(&"/run".to_string()),
            "pseudo filesystems must still be excluded, got {mount_points:?}"
        );
    }

    #[test]
    fn unescape_mount_field_decodes_octal_space() {
        assert_eq!(unescape_mount_field("/mnt/my\\040drive"), "/mnt/my drive");
    }

    #[test]
    fn unescape_mount_field_decodes_tab_newline_and_backslash() {
        assert_eq!(unescape_mount_field("a\\011b"), "a\tb");
        assert_eq!(unescape_mount_field("a\\012b"), "a\nb");
        assert_eq!(unescape_mount_field("a\\134b"), "a\\b");
    }

    #[test]
    fn mount_label_uses_last_segment_with_root_as_system() {
        assert_eq!(mount_label("/"), "System");
        assert_eq!(mount_label("/boot"), "boot");
        assert_eq!(mount_label("/mnt/usb-backup"), "usb-backup");
    }

    #[tokio::test]
    async fn read_image_preview_downscales_preserving_aspect_and_encodes_png() {
        use base64::Engine;
        use image::GenericImageView;

        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("wide.png");
        let source = image::RgbImage::new(1024, 512);
        source.save(&path).expect("save source image");

        let filesystem = LocalFileSystem::new();
        let preview = filesystem
            .read_image_preview(&path.to_string_lossy(), 512)
            .await
            .expect("read image preview");

        assert!(
            preview.starts_with("data:image/png;base64,"),
            "got {preview:.40}"
        );
        let payload = preview
            .strip_prefix("data:image/png;base64,")
            .expect("data uri prefix");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("decode base64 payload");
        let decoded = image::load_from_memory(&bytes).expect("decode png payload");
        let (width, height) = decoded.dimensions();
        assert!(width <= 512, "width {width} should be clamped to 512");
        assert!(height <= 256, "height {height} should be clamped to 256");
    }

    #[tokio::test]
    async fn read_image_preview_on_text_is_unsupported() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("notes.txt");
        fs::write(&path, b"this is plainly not an image").expect("write text file");
        let filesystem = LocalFileSystem::new();
        let error = filesystem
            .read_image_preview(&path.to_string_lossy(), 512)
            .await
            .expect_err("a text file is not an image");
        assert!(matches!(error, FilesError::Unsupported(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn read_image_preview_missing_path_errors() {
        let filesystem = LocalFileSystem::new();
        let error = filesystem
            .read_image_preview("/nonexistent/quantum/picture.png", 512)
            .await
            .expect_err("missing path should error");
        assert!(
            matches!(error, FilesError::NotFound(_) | FilesError::Unsupported(_)),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn mounts_reports_the_root_filesystem() {
        let filesystem = LocalFileSystem::new();
        let drives = filesystem.mounts().await.expect("enumerate mounts");
        let root = drives
            .iter()
            .find(|drive| drive.mount_point == "/")
            .expect("root filesystem present");
        assert_eq!(root.label, "System");
        assert!(root.total_bytes > 0, "root total_bytes should be positive");
    }
}
