//! `FileOpener` implementation: launch files with their associated handlers.
//!
//! `ProcessFileOpener` is self-contained: it shells out to the desktop launch
//! utilities (`xdg-open`, `gio launch`, and a terminal emulator) and never
//! depends on the providers crate. Every launch is fire-and-forget: the child
//! process is spawned detached, with its standard streams pointed at the null
//! device, and the handle is dropped without `kill_on_drop` so the launched
//! program survives the daemon.

use async_trait::async_trait;
use quantum_domain::{FileOpener, FilesError};
use std::path::Path;
use std::process::Stdio;

/// The default colon-separated system data directories used when
/// `$XDG_DATA_DIRS` is unset, per the XDG Base Directory specification.
const DEFAULT_XDG_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// The terminal launched when neither a configured override nor `$TERMINAL`
/// names one. `xdg-terminal-exec` is the freedesktop reference resolver.
const DEFAULT_TERMINAL: &str = "xdg-terminal-exec";

/// A [`FileOpener`] that launches files through the desktop's own utilities.
#[derive(Debug, Default, Clone)]
pub struct ProcessFileOpener {
    /// The configured `files_terminal` value, if any. The caller reads it from
    /// configuration; the opener only needs the resulting `Option`.
    terminal_override: Option<String>,
}

impl ProcessFileOpener {
    /// Construct an opener, taking the configured terminal override (the
    /// `files_terminal` setting) verbatim.
    pub fn new(terminal_override: Option<String>) -> Self {
        Self { terminal_override }
    }
}

/// Build the argument vector that opens `path` with its default handler.
fn open_command(path: &str) -> Vec<String> {
    vec!["xdg-open".to_string(), path.to_string()]
}

/// Return `value` only when it is `Some` and not the empty string.
fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|candidate| !candidate.is_empty())
}

/// Choose the terminal launch argument vector. The program is the configured
/// override if it is present and non-empty, otherwise the value of `$TERMINAL`
/// if present and non-empty, otherwise [`DEFAULT_TERMINAL`]. The argument vector
/// is a single element: the working directory is applied via
/// [`tokio::process::Command::current_dir`], not passed as an argument, because
/// most terminals open in the directory they were spawned from.
fn terminal_command(
    terminal_override: Option<&str>,
    terminal_env: Option<&str>,
    directory: &str,
) -> Vec<String> {
    // The working directory is applied through `Command::current_dir`, so it is
    // deliberately not part of the argument vector.
    let _ = directory;
    let program = non_empty(terminal_override)
        .or_else(|| non_empty(terminal_env))
        .unwrap_or(DEFAULT_TERMINAL);
    vec![program.to_string()]
}

/// Build the ordered list of `applications` directories to search for a
/// `.desktop` file. The per-user directory comes first
/// (`$XDG_DATA_HOME/applications`, defaulting to `~/.local/share/applications`),
/// followed by each colon-separated `$XDG_DATA_DIRS` entry suffixed with
/// `/applications` (defaulting to `/usr/local/share` and `/usr/share`).
pub(crate) fn desktop_file_search_dirs(
    xdg_data_home: Option<&str>,
    xdg_data_dirs: Option<&str>,
    home: &str,
) -> Vec<String> {
    let mut dirs = Vec::new();

    let data_home = match non_empty(xdg_data_home) {
        Some(value) => value.to_string(),
        None => format!("{home}/.local/share"),
    };
    dirs.push(format!("{data_home}/applications"));

    let data_dirs = non_empty(xdg_data_dirs).unwrap_or(DEFAULT_XDG_DATA_DIRS);
    for entry in data_dirs.split(':') {
        if entry.is_empty() {
            continue;
        }
        dirs.push(format!("{entry}/applications"));
    }

    dirs
}

/// Append `.desktop` to a desktop identifier when it is not already present.
fn normalize_desktop_id(desktop_id: &str) -> String {
    if desktop_id.ends_with(".desktop") {
        desktop_id.to_string()
    } else {
        format!("{desktop_id}.desktop")
    }
}

/// Spawn `argv` detached: standard input, output, and error are pointed at the
/// null device and the child handle is dropped without `kill_on_drop`, so the
/// launched process outlives the daemon. Returns `Ok` as soon as the child is
/// launched; a spawn failure (for example a missing binary) maps to
/// [`FilesError::Io`]. Fire-and-forget: the exit status is never awaited.
fn spawn_detached(argv: &[String], current_dir: Option<&str>) -> Result<(), FilesError> {
    let (program, arguments) = argv
        .split_first()
        .ok_or_else(|| FilesError::Io("empty command".to_string()))?;
    let mut command = tokio::process::Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(directory) = current_dir {
        command.current_dir(directory);
    }
    command
        .spawn()
        .map(|_child| ())
        .map_err(|error| FilesError::Io(format!("{program}: {error}")))
}

/// Resolve a desktop identifier to the first `.desktop` file that exists across
/// the ordered search directories.
fn resolve_desktop_file(
    desktop_id: &str,
    xdg_data_home: Option<&str>,
    xdg_data_dirs: Option<&str>,
    home: &str,
) -> Option<String> {
    let normalized = normalize_desktop_id(desktop_id);
    desktop_file_search_dirs(xdg_data_home, xdg_data_dirs, home)
        .into_iter()
        .map(|directory| Path::new(&directory).join(&normalized))
        .find(|candidate| candidate.exists())
        .map(|candidate| candidate.to_string_lossy().to_string())
}

#[async_trait]
impl FileOpener for ProcessFileOpener {
    async fn open(&self, path: &str) -> Result<(), FilesError> {
        spawn_detached(&open_command(path), None)
    }

    async fn open_with(&self, path: &str, desktop_id: &str) -> Result<(), FilesError> {
        let home = std::env::var("HOME").unwrap_or_default();
        let xdg_data_home = std::env::var("XDG_DATA_HOME").ok();
        let xdg_data_dirs = std::env::var("XDG_DATA_DIRS").ok();
        let desktop_file = resolve_desktop_file(
            desktop_id,
            xdg_data_home.as_deref(),
            xdg_data_dirs.as_deref(),
            &home,
        )
        .ok_or_else(|| FilesError::NotFound(desktop_id.to_string()))?;
        let argv = vec![
            "gio".to_string(),
            "launch".to_string(),
            desktop_file,
            path.to_string(),
        ];
        spawn_detached(&argv, None)
    }

    async fn open_terminal(&self, directory: &str) -> Result<(), FilesError> {
        let terminal_env = std::env::var("TERMINAL").ok();
        let argv = terminal_command(
            self.terminal_override.as_deref(),
            terminal_env.as_deref(),
            directory,
        );
        spawn_detached(&argv, Some(directory))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_command_is_xdg_open_with_path() {
        assert_eq!(
            open_command("/tmp/report.pdf"),
            vec!["xdg-open".to_string(), "/tmp/report.pdf".to_string()]
        );
    }

    #[test]
    fn terminal_command_prefers_non_empty_override() {
        assert_eq!(
            terminal_command(Some("kitty"), Some("alacritty"), "/tmp"),
            vec!["kitty".to_string()]
        );
    }

    #[test]
    fn terminal_command_empty_override_falls_through_to_env() {
        assert_eq!(
            terminal_command(Some(""), Some("alacritty"), "/tmp"),
            vec!["alacritty".to_string()]
        );
    }

    #[test]
    fn terminal_command_uses_env_when_override_absent() {
        assert_eq!(
            terminal_command(None, Some("foot"), "/home/user"),
            vec!["foot".to_string()]
        );
    }

    #[test]
    fn terminal_command_defaults_to_xdg_terminal_exec() {
        assert_eq!(
            terminal_command(None, None, "/home/user"),
            vec!["xdg-terminal-exec".to_string()]
        );
        assert_eq!(
            terminal_command(Some(""), Some(""), "/home/user"),
            vec!["xdg-terminal-exec".to_string()]
        );
    }

    #[test]
    fn desktop_file_search_dirs_with_explicit_env() {
        let dirs =
            desktop_file_search_dirs(Some("/data/home"), Some("/opt/a:/opt/b"), "/home/user");
        assert_eq!(
            dirs,
            vec![
                "/data/home/applications".to_string(),
                "/opt/a/applications".to_string(),
                "/opt/b/applications".to_string(),
            ]
        );
    }

    #[test]
    fn desktop_file_search_dirs_defaults_when_env_absent() {
        let dirs = desktop_file_search_dirs(None, None, "/home/user");
        assert_eq!(
            dirs,
            vec![
                "/home/user/.local/share/applications".to_string(),
                "/usr/local/share/applications".to_string(),
                "/usr/share/applications".to_string(),
            ]
        );
    }

    #[test]
    fn desktop_file_search_dirs_treats_empty_env_as_absent() {
        let dirs = desktop_file_search_dirs(Some(""), Some(""), "/home/user");
        assert_eq!(
            dirs,
            vec![
                "/home/user/.local/share/applications".to_string(),
                "/usr/local/share/applications".to_string(),
                "/usr/share/applications".to_string(),
            ]
        );
    }

    #[test]
    fn normalize_desktop_id_appends_suffix_when_missing() {
        assert_eq!(normalize_desktop_id("firefox"), "firefox.desktop");
    }

    #[test]
    fn normalize_desktop_id_keeps_existing_suffix() {
        assert_eq!(normalize_desktop_id("firefox.desktop"), "firefox.desktop");
    }
}
