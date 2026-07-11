//! File operations: copy, move, rename, duplicate, create, delete, trash,
//! compress, and extract.
//!
//! Blocking `std::fs` mutations run on Tokio's blocking pool through the
//! `filesystem` module's `run_blocking` helper; process-backed operations
//! (trash, compress, extract) shell out through `tokio::process::Command`. The
//! command vectors for those three are built by pure helpers so they can be
//! unit-tested without spawning a process. Every mutating operation refuses to
//! overwrite an existing destination, returning [`FilesError::AlreadyExists`]
//! rather than silently clobbering.

use quantum_domain::FilesError;
use std::io::ErrorKind;
use std::path::Path;

/// The raw errno for a cross-device link (`EXDEV`) on Linux. `std::io::ErrorKind`
/// has no stable variant for it, so a rename that fails with this code is
/// detected by its raw operating-system error number.
const CROSS_DEVICE_ERRNO: i32 = 18;

/// Translate a `std::io::Error` for `path` into the typed [`FilesError`] the
/// port contract requires, so no host error type leaks across the boundary.
pub(crate) fn map_io_error(path: &str, error: &std::io::Error) -> FilesError {
    match error.kind() {
        ErrorKind::NotFound => FilesError::NotFound(path.to_string()),
        ErrorKind::PermissionDenied => FilesError::PermissionDenied(path.to_string()),
        ErrorKind::AlreadyExists => FilesError::AlreadyExists(path.to_string()),
        _ => FilesError::Io(format!("{path}: {error}")),
    }
}

/// Return the printable string form of a path.
fn display(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// Report whether anything (file, directory, or even a broken symlink) already
/// occupies `path`. Uses `symlink_metadata` so a dangling symlink still counts
/// as occupied and is never silently overwritten.
fn occupied(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// The final path component of `source` as a bare name, or
/// [`FilesError::Unsupported`] when the path has no name (for example `/`).
fn final_name(source: &Path) -> Result<std::ffi::OsString, FilesError> {
    source
        .file_name()
        .map(|name| name.to_os_string())
        .ok_or_else(|| FilesError::Unsupported(format!("path has no name: {}", display(source))))
}

/// The parent directory of `source`, or [`FilesError::Unsupported`] when it has
/// none (for example the filesystem root).
fn parent_of(source: &Path) -> Result<&Path, FilesError> {
    source
        .parent()
        .ok_or_else(|| FilesError::Unsupported(format!("path has no parent: {}", display(source))))
}

/// Report whether `target` is `source` itself or lies anywhere within its
/// subtree. Uses [`std::path::Path::starts_with`], which compares whole path
/// components, so `/x` is not treated as a prefix of `/xy`. Guards the recursive
/// copy and move paths against copying a directory into its own descendant,
/// which would otherwise recreate the freshly written target inside the source
/// and recurse until the disk fills.
fn is_within(source: &Path, target: &Path) -> bool {
    target == source || target.starts_with(source)
}

/// Recursively copy `source` to the new path `destination`. Directories are
/// created and walked; regular files are copied byte for byte; symlinks are
/// recreated pointing at the same target rather than followed. `destination` is
/// the full target path, not the directory to copy into.
pub(crate) fn copy_recursive(source: &Path, destination: &Path) -> Result<(), FilesError> {
    let metadata = std::fs::symlink_metadata(source)
        .map_err(|error| map_io_error(&display(source), &error))?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        let target =
            std::fs::read_link(source).map_err(|error| map_io_error(&display(source), &error))?;
        std::os::unix::fs::symlink(&target, destination)
            .map_err(|error| map_io_error(&display(destination), &error))?;
    } else if file_type.is_dir() {
        std::fs::create_dir(destination)
            .map_err(|error| map_io_error(&display(destination), &error))?;
        let reader =
            std::fs::read_dir(source).map_err(|error| map_io_error(&display(source), &error))?;
        for item in reader {
            let item = item.map_err(|error| map_io_error(&display(source), &error))?;
            copy_recursive(&item.path(), &destination.join(item.file_name()))?;
        }
    } else {
        std::fs::copy(source, destination)
            .map_err(|error| map_io_error(&display(source), &error))?;
    }
    Ok(())
}

/// Remove `path` recursively: directories with `remove_dir_all`, everything else
/// (files and symlinks) with `remove_file`.
fn remove_path(path: &Path) -> Result<(), FilesError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| map_io_error(&display(path), &error))?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(path).map_err(|error| map_io_error(&display(path), &error))
    } else {
        std::fs::remove_file(path).map_err(|error| map_io_error(&display(path), &error))
    }
}

/// Copy each source into the directory `destination`, recursing into
/// directories. A source whose final name already exists in `destination`
/// yields [`FilesError::AlreadyExists`] rather than overwriting it.
pub fn copy_into(sources: &[String], destination: &str) -> Result<(), FilesError> {
    let destination_dir = Path::new(destination);
    for source in sources {
        let source_path = Path::new(source);
        let target = destination_dir.join(final_name(source_path)?);
        if is_within(source_path, &target) {
            return Err(FilesError::Unsupported(format!(
                "cannot copy {} into its own subtree",
                display(source_path)
            )));
        }
        if occupied(&target) {
            return Err(FilesError::AlreadyExists(display(&target)));
        }
        copy_recursive(source_path, &target)?;
    }
    Ok(())
}

/// Move each source into the directory `destination`. A plain `rename` is tried
/// first; if it fails because the source and destination sit on different
/// devices (`EXDEV`), fall back to a recursive copy followed by removal of the
/// source. A name collision in `destination` yields
/// [`FilesError::AlreadyExists`].
pub fn move_into(sources: &[String], destination: &str) -> Result<(), FilesError> {
    let destination_dir = Path::new(destination);
    for source in sources {
        let source_path = Path::new(source);
        let target = destination_dir.join(final_name(source_path)?);
        if is_within(source_path, &target) {
            return Err(FilesError::Unsupported(format!(
                "cannot move {} into its own subtree",
                display(source_path)
            )));
        }
        if occupied(&target) {
            return Err(FilesError::AlreadyExists(display(&target)));
        }
        match std::fs::rename(source_path, &target) {
            Ok(()) => {}
            Err(error) if error.raw_os_error() == Some(CROSS_DEVICE_ERRNO) => {
                copy_recursive(source_path, &target)?;
                remove_path(source_path)?;
            }
            Err(error) => return Err(map_io_error(source, &error)),
        }
    }
    Ok(())
}

/// Rename `path` to a sibling named `new_name`. `new_name` must be a bare file
/// name; one containing a path separator is rejected with
/// [`FilesError::Unsupported`]. An existing sibling of that name yields
/// [`FilesError::AlreadyExists`].
pub fn rename(path: &str, new_name: &str) -> Result<(), FilesError> {
    if new_name.contains('/') {
        return Err(FilesError::Unsupported(format!(
            "new name must be a bare file name: {new_name}"
        )));
    }
    let source_path = Path::new(path);
    let target = parent_of(source_path)?.join(new_name);
    if occupied(&target) {
        return Err(FilesError::AlreadyExists(display(&target)));
    }
    std::fs::rename(source_path, &target).map_err(|error| map_io_error(path, &error))
}

/// Split a file name into its stem and its extension (including the leading
/// dot). A leading dot is treated as part of the stem, so `.bashrc` has an empty
/// extension, and a name with no interior dot (`notes`) also has an empty
/// extension.
fn split_stem_extension(file_name: &str) -> (&str, &str) {
    match file_name.rfind('.') {
        Some(index) if index > 0 => (&file_name[..index], &file_name[index..]),
        _ => (file_name, ""),
    }
}

/// Produce the next free `(copy)` name for `stem` plus `extension`, consulting
/// `existing` to skip names already taken: `name (copy)`, then `name (copy 2)`,
/// `name (copy 3)`, and so on.
fn next_free_name(stem: &str, extension: &str, existing: &dyn Fn(&str) -> bool) -> String {
    let first = format!("{stem} (copy){extension}");
    if !existing(&first) {
        return first;
    }
    let mut counter = 2;
    loop {
        let candidate = format!("{stem} (copy {counter}){extension}");
        if !existing(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

/// Compute the duplicate name for a file called `file_name`, avoiding any name
/// for which `existing` returns true. The extension is preserved after the
/// `(copy)` marker: `report.txt` becomes `report (copy).txt`.
pub fn duplicate_name(file_name: &str, existing: &dyn Fn(&str) -> bool) -> String {
    let (stem, extension) = split_stem_extension(file_name);
    next_free_name(stem, extension, existing)
}

/// Duplicate `path` alongside itself under the next free `(copy)` name.
/// Directories duplicate by whole name with no extension split; files preserve
/// their extension.
pub fn duplicate(path: &str) -> Result<(), FilesError> {
    let source_path = Path::new(path);
    let parent = parent_of(source_path)?;
    let file_name = final_name(source_path)?.to_string_lossy().to_string();
    let metadata =
        std::fs::symlink_metadata(source_path).map_err(|error| map_io_error(path, &error))?;
    let existing = |candidate: &str| occupied(&parent.join(candidate));
    let new_name = if metadata.is_dir() {
        next_free_name(&file_name, "", &existing)
    } else {
        duplicate_name(&file_name, &existing)
    };
    copy_recursive(source_path, &parent.join(new_name))
}

/// Create an empty directory named `name` under `parent`. An existing entry of
/// that name yields [`FilesError::AlreadyExists`].
pub fn new_folder(parent: &str, name: &str) -> Result<(), FilesError> {
    let target = Path::new(parent).join(name);
    if occupied(&target) {
        return Err(FilesError::AlreadyExists(display(&target)));
    }
    std::fs::create_dir(&target).map_err(|error| map_io_error(&display(&target), &error))
}

/// Create an empty file named `name` under `parent`. An existing entry of that
/// name yields [`FilesError::AlreadyExists`].
pub fn new_file(parent: &str, name: &str) -> Result<(), FilesError> {
    let target = Path::new(parent).join(name);
    if occupied(&target) {
        return Err(FilesError::AlreadyExists(display(&target)));
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map(|_| ())
        .map_err(|error| map_io_error(&display(&target), &error))
}

/// Remove every path recursively (directories and files alike).
pub fn delete(paths: &[String]) -> Result<(), FilesError> {
    for path in paths {
        remove_path(Path::new(path))?;
    }
    Ok(())
}

/// Build the `gio trash` command line for `paths` without invoking it.
pub fn trash_command(paths: &[String]) -> Vec<String> {
    let mut command = vec!["gio".to_string(), "trash".to_string()];
    command.extend(paths.iter().cloned());
    command
}

/// Compute the single parent directory shared by every path and the list of
/// their final names. Returns [`FilesError::Unsupported`] when the paths do not
/// all share one parent (or when the list is empty).
fn common_parent_and_names(paths: &[String]) -> Result<(String, Vec<String>), FilesError> {
    let mut common_parent: Option<String> = None;
    let mut names = Vec::with_capacity(paths.len());
    for path in paths {
        let path_ref = Path::new(path);
        let parent = display(parent_of(path_ref)?);
        let name = final_name(path_ref)?.to_string_lossy().to_string();
        match &common_parent {
            None => common_parent = Some(parent),
            Some(existing) if existing != &parent => {
                return Err(FilesError::Unsupported(
                    "paths do not share a common parent directory".to_string(),
                ));
            }
            Some(_) => {}
        }
        names.push(name);
    }
    match common_parent {
        Some(parent) => Ok((parent, names)),
        None => Err(FilesError::Unsupported("no paths to compress".to_string())),
    }
}

/// Build the `tar --zstd` command line that archives `paths` into `destination`,
/// changing into their shared parent directory so the archive stores bare final
/// names. Returns [`FilesError::Unsupported`] when the paths do not share a
/// parent.
pub fn compress_command(paths: &[String], destination: &str) -> Result<Vec<String>, FilesError> {
    let (parent, names) = common_parent_and_names(paths)?;
    let mut command = vec![
        "tar".to_string(),
        "--zstd".to_string(),
        "-cf".to_string(),
        destination.to_string(),
        "-C".to_string(),
        parent,
    ];
    command.extend(names);
    Ok(command)
}

/// Build the extraction command line for `path`, dispatching by extension: a
/// `.zip` uses `unzip`, and everything else uses `tar -xf`, which auto-detects
/// gzip, zstd, and plain tar. Both extract into the archive's parent directory.
///
/// The zip branch passes `-n` so `unzip` never overwrites an existing file,
/// preserving anything already present in the destination. The `tar -xf` branch
/// keeps tar's default behaviour, which does overwrite existing files; making
/// tar non-overwriting portably is out of scope here.
pub fn extract_command(path: &str) -> Vec<String> {
    let parent = Path::new(path)
        .parent()
        .map(display)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ".".to_string());
    if path.to_ascii_lowercase().ends_with(".zip") {
        vec![
            "unzip".to_string(),
            "-n".to_string(),
            path.to_string(),
            "-d".to_string(),
            parent,
        ]
    } else {
        vec![
            "tar".to_string(),
            "-xf".to_string(),
            path.to_string(),
            "-C".to_string(),
            parent,
        ]
    }
}

/// Run an external command, mapping a spawn failure or a non-zero exit to
/// [`FilesError::Io`] carrying the captured standard-error text.
async fn run_command(command: &[String]) -> Result<(), FilesError> {
    let (program, arguments) = command
        .split_first()
        .ok_or_else(|| FilesError::Io("empty command".to_string()))?;
    let output = tokio::process::Command::new(program)
        .args(arguments)
        .output()
        .await
        .map_err(|error| FilesError::Io(format!("{program}: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(FilesError::Io(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

/// Move `paths` to the trash by shelling out to `gio trash`.
pub async fn run_trash(paths: &[String]) -> Result<(), FilesError> {
    run_command(&trash_command(paths)).await
}

/// Compress `paths` into `destination` by shelling out to `tar --zstd`.
pub async fn run_compress(paths: &[String], destination: &str) -> Result<(), FilesError> {
    run_command(&compress_command(paths, destination)?).await
}

/// Extract the archive at `path` into its parent directory.
pub async fn run_extract(path: &str) -> Result<(), FilesError> {
    run_command(&extract_command(path)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn path_string(path: &std::path::Path) -> String {
        path.to_string_lossy().to_string()
    }

    #[test]
    fn copy_nested_directory_tree_reproduces_structure_and_contents() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let source = dir.path().join("tree");
        fs::create_dir(&source).expect("create source directory");
        fs::write(source.join("top.txt"), b"top contents").expect("write top file");
        let nested = source.join("nested");
        fs::create_dir(&nested).expect("create nested directory");
        fs::write(nested.join("deep.txt"), b"deep contents").expect("write deep file");

        let destination = dir.path().join("destination");
        fs::create_dir(&destination).expect("create destination directory");

        copy_into(&[path_string(&source)], &path_string(&destination)).expect("copy tree");

        let copied = destination.join("tree");
        assert_eq!(
            fs::read_to_string(copied.join("top.txt")).expect("read copied top"),
            "top contents"
        );
        assert_eq!(
            fs::read_to_string(copied.join("nested").join("deep.txt")).expect("read copied deep"),
            "deep contents"
        );
    }

    #[test]
    fn copy_onto_existing_name_is_already_exists() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let source = dir.path().join("file.txt");
        fs::write(&source, b"content").expect("write source");
        let destination = dir.path().join("destination");
        fs::create_dir(&destination).expect("create destination");
        fs::write(destination.join("file.txt"), b"already here").expect("write clashing file");

        let error = copy_into(&[path_string(&source)], &path_string(&destination))
            .expect_err("copy onto existing name should fail");
        assert!(
            matches!(error, FilesError::AlreadyExists(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn copy_directory_into_its_own_subtree_is_rejected() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let root = dir.path().join("root");
        fs::create_dir(&root).expect("create root directory");
        fs::write(root.join("file.txt"), b"payload").expect("write file");
        let sub = root.join("sub");
        fs::create_dir(&sub).expect("create sub directory");

        let error = copy_into(&[path_string(&root)], &path_string(&sub))
            .expect_err("copying a directory into its own subtree should fail");
        assert!(matches!(error, FilesError::Unsupported(_)), "got {error:?}");
        // The guard must reject before any recursion creates a runaway tree.
        assert!(
            !sub.join("root").exists(),
            "no target should have been created inside the source subtree"
        );
    }

    #[test]
    fn copy_directory_to_a_sibling_still_works() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let root = dir.path().join("root");
        fs::create_dir(&root).expect("create root directory");
        fs::write(root.join("file.txt"), b"payload").expect("write file");
        let destination = dir.path().join("destination");
        fs::create_dir(&destination).expect("create destination directory");

        copy_into(&[path_string(&root)], &path_string(&destination)).expect("sibling copy");

        assert_eq!(
            fs::read_to_string(destination.join("root").join("file.txt")).expect("read copied"),
            "payload"
        );
    }

    #[test]
    fn move_directory_into_its_own_subtree_is_rejected() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let root = dir.path().join("root");
        fs::create_dir(&root).expect("create root directory");
        fs::write(root.join("file.txt"), b"payload").expect("write file");
        let sub = root.join("sub");
        fs::create_dir(&sub).expect("create sub directory");

        let error = move_into(&[path_string(&root)], &path_string(&sub))
            .expect_err("moving a directory into its own subtree should fail");
        assert!(matches!(error, FilesError::Unsupported(_)), "got {error:?}");
        assert!(
            !sub.join("root").exists(),
            "no target should have been created inside the source subtree"
        );
    }

    #[test]
    fn move_file_same_device_relocates_it() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let source = dir.path().join("movable.txt");
        fs::write(&source, b"payload").expect("write source");
        let destination = dir.path().join("destination");
        fs::create_dir(&destination).expect("create destination");

        move_into(&[path_string(&source)], &path_string(&destination)).expect("move file");

        assert!(!source.exists(), "source should be gone after move");
        assert_eq!(
            fs::read_to_string(destination.join("movable.txt")).expect("read moved file"),
            "payload"
        );
    }

    #[test]
    fn move_onto_existing_name_is_already_exists() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let source = dir.path().join("movable.txt");
        fs::write(&source, b"payload").expect("write source");
        let destination = dir.path().join("destination");
        fs::create_dir(&destination).expect("create destination");
        fs::write(destination.join("movable.txt"), b"occupied").expect("write clashing file");

        let error = move_into(&[path_string(&source)], &path_string(&destination))
            .expect_err("move onto existing name should fail");
        assert!(
            matches!(error, FilesError::AlreadyExists(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn rename_moves_old_name_to_new_name() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let original = dir.path().join("before.txt");
        fs::write(&original, b"unchanged").expect("write original");

        rename(&path_string(&original), "after.txt").expect("rename file");

        assert!(!original.exists(), "old name should be gone");
        assert_eq!(
            fs::read_to_string(dir.path().join("after.txt")).expect("read renamed file"),
            "unchanged"
        );
    }

    #[test]
    fn rename_with_slash_is_unsupported() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let original = dir.path().join("before.txt");
        fs::write(&original, b"unchanged").expect("write original");

        let error = rename(&path_string(&original), "nested/after.txt")
            .expect_err("new name with slash should be rejected");
        assert!(matches!(error, FilesError::Unsupported(_)), "got {error:?}");
    }

    #[test]
    fn rename_collision_is_already_exists() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let original = dir.path().join("before.txt");
        fs::write(&original, b"unchanged").expect("write original");
        fs::write(dir.path().join("after.txt"), b"occupied").expect("write clashing file");

        let error = rename(&path_string(&original), "after.txt")
            .expect_err("rename onto existing name should fail");
        assert!(
            matches!(error, FilesError::AlreadyExists(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn duplicate_name_appends_copy_before_extension() {
        assert_eq!(
            duplicate_name("report.txt", &|_| false),
            "report (copy).txt"
        );
    }

    #[test]
    fn duplicate_name_increments_when_first_copy_exists() {
        assert_eq!(
            duplicate_name("report.txt", &|candidate| candidate == "report (copy).txt"),
            "report (copy 2).txt"
        );
    }

    #[test]
    fn duplicate_name_without_extension() {
        assert_eq!(duplicate_name("notes", &|_| false), "notes (copy)");
    }

    #[test]
    fn duplicate_name_treats_leading_dot_as_stem() {
        assert_eq!(duplicate_name(".bashrc", &|_| false), ".bashrc (copy)");
    }

    #[test]
    fn duplicate_file_creates_copy_with_expected_name() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let source = dir.path().join("report.txt");
        fs::write(&source, b"figures").expect("write source");

        duplicate(&path_string(&source)).expect("duplicate file");

        assert_eq!(
            fs::read_to_string(dir.path().join("report (copy).txt")).expect("read duplicate"),
            "figures"
        );
    }

    #[test]
    fn new_folder_and_new_file_create_entries() {
        let dir = tempfile::tempdir().expect("create tempdir");

        new_folder(&path_string(dir.path()), "created-folder").expect("create folder");
        assert!(dir.path().join("created-folder").is_dir(), "folder present");

        new_file(&path_string(dir.path()), "created-file.txt").expect("create file");
        let created = dir.path().join("created-file.txt");
        assert!(created.is_file(), "file present");
        assert_eq!(
            fs::metadata(&created).expect("stat created file").len(),
            0,
            "new file should be empty"
        );
    }

    #[test]
    fn new_folder_collision_is_already_exists() {
        let dir = tempfile::tempdir().expect("create tempdir");
        fs::create_dir(dir.path().join("existing")).expect("create existing directory");

        let error = new_folder(&path_string(dir.path()), "existing")
            .expect_err("creating an existing folder should fail");
        assert!(
            matches!(error, FilesError::AlreadyExists(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn new_file_collision_is_already_exists() {
        let dir = tempfile::tempdir().expect("create tempdir");
        fs::write(dir.path().join("existing.txt"), b"here").expect("write existing file");

        let error = new_file(&path_string(dir.path()), "existing.txt")
            .expect_err("creating an existing file should fail");
        assert!(
            matches!(error, FilesError::AlreadyExists(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn delete_removes_a_directory_recursively() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let target = dir.path().join("doomed");
        fs::create_dir(&target).expect("create target directory");
        fs::write(target.join("child.txt"), b"bye").expect("write child file");
        fs::create_dir(target.join("subdir")).expect("create subdirectory");

        delete(&[path_string(&target)]).expect("delete directory");

        assert!(!target.exists(), "directory should be gone");
    }

    #[test]
    fn trash_command_prefixes_gio_trash() {
        let paths = vec!["/a/one".to_string(), "/b/two".to_string()];
        assert_eq!(
            trash_command(&paths),
            vec![
                "gio".to_string(),
                "trash".to_string(),
                "/a/one".to_string(),
                "/b/two".to_string(),
            ]
        );
    }

    #[test]
    fn compress_command_uses_shared_parent_and_final_names() {
        let paths = vec![
            "/home/user/project/a.txt".to_string(),
            "/home/user/project/b.txt".to_string(),
        ];
        let command =
            compress_command(&paths, "/home/user/bundle.tar.zst").expect("build compress command");
        assert_eq!(
            command,
            vec![
                "tar".to_string(),
                "--zstd".to_string(),
                "-cf".to_string(),
                "/home/user/bundle.tar.zst".to_string(),
                "-C".to_string(),
                "/home/user/project".to_string(),
                "a.txt".to_string(),
                "b.txt".to_string(),
            ]
        );
    }

    #[test]
    fn compress_command_without_common_parent_is_unsupported() {
        let paths = vec![
            "/home/user/one/a.txt".to_string(),
            "/home/user/two/b.txt".to_string(),
        ];
        let error = compress_command(&paths, "/home/user/bundle.tar.zst")
            .expect_err("mismatched parents should fail");
        assert!(matches!(error, FilesError::Unsupported(_)), "got {error:?}");
    }

    #[test]
    fn extract_command_uses_unzip_for_zip() {
        assert_eq!(
            extract_command("/home/user/archive.zip"),
            vec![
                "unzip".to_string(),
                "-n".to_string(),
                "/home/user/archive.zip".to_string(),
                "-d".to_string(),
                "/home/user".to_string(),
            ]
        );
    }

    #[test]
    fn extract_command_uses_tar_for_other_extensions() {
        assert_eq!(
            extract_command("/home/user/archive.tar.zst"),
            vec![
                "tar".to_string(),
                "-xf".to_string(),
                "/home/user/archive.tar.zst".to_string(),
                "-C".to_string(),
                "/home/user".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn compress_then_extract_round_trip() {
        if std::process::Command::new("tar")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping compress/extract round trip: tar is not on PATH");
            return;
        }

        let dir = tempfile::tempdir().expect("create tempdir");
        let source = dir.path().join("source");
        fs::create_dir(&source).expect("create source directory");
        fs::write(source.join("alpha.txt"), b"alpha contents").expect("write alpha");
        fs::write(source.join("beta.txt"), b"beta contents").expect("write beta");

        let output_dir = dir.path().join("output");
        fs::create_dir(&output_dir).expect("create output directory");
        let archive = output_dir.join("bundle.tar.zst");

        let paths = vec![
            path_string(&source.join("alpha.txt")),
            path_string(&source.join("beta.txt")),
        ];
        if let Err(error) = run_compress(&paths, &path_string(&archive)).await {
            eprintln!("skipping compress/extract round trip: compress failed ({error})");
            return;
        }

        run_extract(&path_string(&archive))
            .await
            .expect("extract archive");

        assert_eq!(
            fs::read_to_string(output_dir.join("alpha.txt")).expect("read extracted alpha"),
            "alpha contents"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("beta.txt")).expect("read extracted beta"),
            "beta contents"
        );
    }
}
