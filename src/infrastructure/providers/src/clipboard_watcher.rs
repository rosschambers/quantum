//! Configurable clipboard watcher.
//!
//! Spawns `wl-paste --watch` and, on each clipboard change, lists the offered
//! MIME types, classifies the selection, pulls the chosen payload, builds a
//! [`quantum_domain::ClipboardEntry`] (with an image thumbnail via
//! [`crate::clipboard_capture`]), dedups consecutive identical text, and appends
//! it to the [`quantum_domain::ClipboardStore`].
//!
//! Only the two pure helpers ([`resolve_clipboard_watcher`] and
//! [`parse_list_types`]) are unit-tested; the live spawn loop needs a real
//! Wayland session and is exercised manually.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use quantum_domain::{ClipboardEntry, ClipboardStore};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::clipboard_capture::{classify, thumbnail, ClipKind};

/// Resolve the base argv used to watch the clipboard.
///
/// A non-empty `config` override wins and is split with shell-word semantics.
/// Otherwise, if `wl-paste` is resolvable via `which`, the default watch argv
/// `["wl-paste", "--watch"]` is returned. When neither applies the result is
/// empty, signalling the watcher cannot start. The per-change type listing and
/// payload pulls are performed by separate `wl-paste` invocations, so this argv
/// is only the long-lived `--watch` process.
pub fn resolve_clipboard_watcher(
    config: Option<&str>,
    which: impl Fn(&str) -> Option<PathBuf>,
) -> Vec<String> {
    if let Some(raw) = config {
        if !raw.is_empty() {
            return shell_words::split(raw).unwrap_or_default();
        }
    }
    if which("wl-paste").is_some() {
        return vec!["wl-paste".to_string(), "--watch".to_string()];
    }
    Vec::new()
}

/// Split `wl-paste --list-types` output into trimmed, non-empty MIME-type lines.
pub fn parse_list_types(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect()
}

/// Choose the MIME type to pull the payload under, given the offered `types`
/// and the resolved [`ClipKind`]. For images an explicit `image/png` is
/// preferred; for text a `text/plain` variant is preferred; otherwise the first
/// offered type is used.
fn preferred_type(kind: ClipKind, types: &[String]) -> Option<String> {
    match kind {
        ClipKind::Image => types
            .iter()
            .find(|mime| *mime == "image/png")
            .or_else(|| types.iter().find(|mime| mime.starts_with("image/")))
            .cloned(),
        ClipKind::Text | ClipKind::File => types
            .iter()
            .find(|mime| mime.starts_with("text/plain"))
            .or_else(|| types.iter().find(|mime| mime.starts_with("text/")))
            .or_else(|| types.first())
            .cloned(),
        ClipKind::Binary => types.first().cloned(),
    }
}

/// A short single-line preview of copied text: collapse runs of whitespace to a
/// single space, trim, and cap the length.
fn text_preview(full: &str) -> String {
    let collapsed = full.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 120;
    if collapsed.chars().count() > MAX {
        collapsed.chars().take(MAX).collect::<String>() + "…"
    } else {
        collapsed
    }
}

/// Current time as whole seconds and as a nanosecond-resolution id source.
fn now_unix_and_nanos() -> (u64, u128) {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(elapsed) => (elapsed.as_secs(), elapsed.as_nanos()),
        Err(error) => {
            tracing::warn!(%error, "system clock before Unix epoch; using zero timestamp");
            (0, 0)
        }
    }
}

/// Watches the Wayland clipboard and records changes into a
/// [`ClipboardStore`].
pub struct ClipboardWatcher {
    watch_argv: Vec<String>,
    store: Arc<dyn ClipboardStore>,
    blob_dir: PathBuf,
}

impl ClipboardWatcher {
    /// Construct a watcher from a resolved watch argv (see
    /// [`resolve_clipboard_watcher`]), the shared store, and the directory the
    /// store writes blobs to (used to fill each blob-backed entry's
    /// `blob_path`).
    pub fn new(watch_argv: Vec<String>, store: Arc<dyn ClipboardStore>, blob_dir: PathBuf) -> Self {
        Self {
            watch_argv,
            store,
            blob_dir,
        }
    }

    /// Spawn the background watch task. Returns immediately; the task runs until
    /// the `wl-paste --watch` process ends or the future is dropped. A watcher
    /// with an empty argv logs a warning and does nothing.
    pub fn start(self) {
        if self.watch_argv.is_empty() {
            tracing::warn!("clipboard watcher has no command; not starting");
            return;
        }
        tokio::spawn(async move {
            self.run().await;
        });
    }

    /// The watch loop. `wl-paste --watch <cmd>` normally runs a command per
    /// change; here we run it with no command but read its per-change line
    /// emissions, treating each line as a change signal and then re-reading the
    /// current clipboard state out of band.
    async fn run(self) {
        // `wl-paste --watch` runs a program on each change. Point it at a
        // trivial program that prints a newline, so each change surfaces as one
        // line on the watcher's stdout that we can await.
        let mut argv = self.watch_argv.clone();
        argv.push("printf".to_string());
        argv.push("\\n".to_string());

        let mut command = Command::new(&argv[0]);
        command
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                tracing::warn!(program = %argv[0], %error, "failed to spawn clipboard watcher");
                return;
            }
        };

        let Some(stdout) = child.stdout.take() else {
            tracing::warn!("clipboard watcher child had no stdout");
            return;
        };

        tracing::info!("clipboard watcher started");
        let mut last_text_hash: Option<u64> = None;
        let mut lines = BufReader::new(stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(_)) => {
                    self.handle_change(&mut last_text_hash).await;
                }
                Ok(None) => {
                    tracing::warn!("clipboard watcher stream ended");
                    break;
                }
                Err(error) => {
                    tracing::warn!(%error, "error reading clipboard watcher stream");
                    break;
                }
            }
        }
    }

    /// Handle one clipboard change: list types, classify, pull the payload, and
    /// append an entry. Dedups consecutive identical text via `last_text_hash`.
    async fn handle_change(&self, last_text_hash: &mut Option<u64>) {
        let types = match self.list_types().await {
            Some(types) if !types.is_empty() => types,
            _ => return,
        };
        let kind = classify(&types);
        let Some(mime) = preferred_type(kind, &types) else {
            tracing::warn!("clipboard change offered no usable MIME type");
            return;
        };

        let bytes = match self.pull(&mime).await {
            Some(bytes) if !bytes.is_empty() => bytes,
            _ => return,
        };

        let (created_unix, nanos) = now_unix_and_nanos();
        let id = format!("{nanos}");
        let size_bytes = bytes.len() as u64;

        match kind {
            ClipKind::Text | ClipKind::File => {
                let full = String::from_utf8_lossy(&bytes).to_string();
                let hash = hash_text(&full);
                if *last_text_hash == Some(hash) {
                    return;
                }
                *last_text_hash = Some(hash);
                let entry = ClipboardEntry::Text {
                    id,
                    created_unix,
                    size_bytes,
                    preview: text_preview(&full),
                    full,
                };
                self.append(entry, None).await;
            }
            ClipKind::Image => {
                *last_text_hash = None;
                let thumb = thumbnail(&bytes).unwrap_or_default();
                let (width, height) = image_dimensions(&bytes);
                let entry = ClipboardEntry::Image {
                    id: id.clone(),
                    created_unix,
                    size_bytes,
                    preview_thumb: thumb,
                    blob_path: self.blob_path_string(&id),
                    width,
                    height,
                };
                self.append(entry, Some(bytes)).await;
            }
            ClipKind::Binary => {
                *last_text_hash = None;
                let entry = ClipboardEntry::Binary {
                    id: id.clone(),
                    created_unix,
                    size_bytes,
                    mime,
                    blob_path: self.blob_path_string(&id),
                };
                self.append(entry, Some(bytes)).await;
            }
        }
    }

    /// The `<id>.bin` path string inside the store's blob directory.
    fn blob_path_string(&self, id: &str) -> String {
        self.blob_dir
            .join(format!("{id}.bin"))
            .to_string_lossy()
            .to_string()
    }

    /// Append `entry` (and its optional blob) to the store, logging any failure.
    async fn append(&self, entry: ClipboardEntry, blob: Option<Vec<u8>>) {
        if let Err(error) = self.store.append(entry, blob).await {
            tracing::warn!(%error, "failed to append clipboard entry");
        }
    }

    /// Run `wl-paste --list-types` and parse the offered MIME types.
    async fn list_types(&self) -> Option<Vec<String>> {
        let output = Command::new("wl-paste")
            .arg("--list-types")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await;
        match output {
            Ok(output) => Some(parse_list_types(&String::from_utf8_lossy(&output.stdout))),
            Err(error) => {
                tracing::warn!(%error, "failed to run wl-paste --list-types");
                None
            }
        }
    }

    /// Run `wl-paste --type <mime>` and return the raw bytes.
    async fn pull(&self, mime: &str) -> Option<Vec<u8>> {
        let output = Command::new("wl-paste")
            .arg("--type")
            .arg(mime)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await;
        match output {
            Ok(output) => Some(output.stdout),
            Err(error) => {
                tracing::warn!(%error, mime, "failed to run wl-paste --type");
                None
            }
        }
    }
}

/// A stable hash of copied text, used to dedup consecutive identical copies.
fn hash_text(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Decode just the dimensions of an image, returning `(0, 0)` when the bytes
/// cannot be decoded.
fn image_dimensions(bytes: &[u8]) -> (u32, u32) {
    match image::load_from_memory(bytes) {
        Ok(image) => {
            use image::GenericImageView as _;
            image.dimensions()
        }
        Err(error) => {
            tracing::warn!(%error, "failed to read clipboard image dimensions");
            (0, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins_over_probe() {
        let argv = resolve_clipboard_watcher(Some("wl-paste --watch --primary"), |_| {
            Some(PathBuf::from("/usr/bin/wl-paste"))
        });
        assert_eq!(argv, vec!["wl-paste", "--watch", "--primary"]);
    }

    #[test]
    fn empty_override_falls_through_to_probe() {
        let argv = resolve_clipboard_watcher(Some(""), |name| {
            (name == "wl-paste").then(|| PathBuf::from("/usr/bin/wl-paste"))
        });
        assert_eq!(argv, vec!["wl-paste", "--watch"]);
    }

    #[test]
    fn probe_returns_default_watch_argv() {
        let argv = resolve_clipboard_watcher(None, |name| {
            (name == "wl-paste").then(|| PathBuf::from("/usr/bin/wl-paste"))
        });
        assert_eq!(argv, vec!["wl-paste", "--watch"]);
    }

    #[test]
    fn nothing_available_returns_empty() {
        let argv = resolve_clipboard_watcher(None, |_| None);
        assert!(argv.is_empty());
    }

    #[test]
    fn parse_list_types_trims_and_drops_blank_lines() {
        let stdout = "text/plain\n  text/html  \n\nimage/png\n";
        assert_eq!(
            parse_list_types(stdout),
            vec!["text/plain", "text/html", "image/png"]
        );
    }

    #[test]
    fn parse_list_types_empty_is_empty() {
        assert!(parse_list_types("").is_empty());
    }

    #[test]
    fn preferred_type_prefers_png_for_images() {
        let types = vec!["image/bmp".to_string(), "image/png".to_string()];
        assert_eq!(
            preferred_type(ClipKind::Image, &types),
            Some("image/png".to_string())
        );
    }

    #[test]
    fn preferred_type_prefers_plain_text() {
        let types = vec!["text/html".to_string(), "text/plain".to_string()];
        assert_eq!(
            preferred_type(ClipKind::Text, &types),
            Some("text/plain".to_string())
        );
    }

    #[test]
    fn text_preview_collapses_whitespace_and_caps_length() {
        assert_eq!(text_preview("  hello   world \n line "), "hello world line");
        let long: String = "a".repeat(200);
        let preview = text_preview(&long);
        assert!(preview.chars().count() <= 121);
        assert!(preview.ends_with('…'));
    }
}
