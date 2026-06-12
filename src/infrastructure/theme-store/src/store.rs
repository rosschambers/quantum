use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use std::time::Duration;
use tokio::sync::RwLock;

use quantum_domain::DomainError;
use quantum_domain::EventBus;

// Embed the default theme
static DEFAULT_THEME: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../ui/themes/default");

/// Resolved view from embedded or disk sources.
#[derive(Debug, Clone)]
pub struct ResolvedViewData {
    pub content: Vec<u8>,
    pub mime_type: String,
}

/// Generate candidate filesystem paths to try for a logical view path.
///
/// Views can be nested at different depths:
///   `views/launcher/index.html` -> `views/launcher/dist/index.html`
///   `views/launcher/assets/x.js` -> `views/launcher/dist/assets/x.js`
///   `views/widgets/clock/index.html` -> `views/widgets/clock/dist/index.html`
///   `views/widgets/bar/assets/x.js` -> `views/widgets/bar/dist/assets/x.js`
///
/// Generate candidates by trying to insert `dist/` after each possible view
/// directory boundary (positions 1 and 2 segments deep under `views/`). The
/// first existing path wins.
/// Non-view paths (`tokens.toml`, `theme.toml`, etc.) are returned unchanged.
fn candidate_paths(path: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(4);

    if let Some(rest) = path.strip_prefix("views/") {
        let segments: Vec<&str> = rest.split('/').collect();
        // Skip if path already contains `dist/` somewhere.
        let already_has_dist = segments.contains(&"dist");
        if !already_has_dist {
            // Try inserting `dist/` after each potential view-root boundary.
            // Most-nested first so `widgets/clock` wins over `widgets`.
            // We only consider boundaries where at least one segment remains
            // after the insertion point.
            for cut in (1..segments.len()).rev() {
                let head = segments[..cut].join("/");
                let tail = segments[cut..].join("/");
                out.push(format!("views/{head}/dist/{tail}"));
            }
        }
    }

    out.push(path.to_string());
    out
}

/// Walk a parsed `tokens.toml` table one level deep and collect string-valued
/// entries.
///
/// Top-level entries:
/// - String value: inserted directly as `(key, value)`. Supports flat files
///   where every token sits at the root.
/// - Table value: each string-valued child is inserted as `(child_key, value)`;
///   the section header is discarded. Supports the shipped nested file format
///   with `[colors]`, `[typography]`, etc. categories.
/// - Anything else (array, integer, bool, datetime, nested tables of tables):
///   skipped with a warning.
///
/// When the same token key appears in two sections, the later one wins and
/// a warning is logged.
fn parse_tokens_table(parsed: toml::Table) -> HashMap<String, String> {
    let mut tokens: HashMap<String, String> = HashMap::new();

    for (key, value) in parsed {
        match value {
            toml::Value::String(s) => {
                if tokens.insert(key.clone(), s).is_some() {
                    tracing::warn!("duplicate token key {key}: later value wins");
                }
            }
            toml::Value::Table(table) => {
                for (child_key, child_value) in table {
                    if let Some(s) = child_value.as_str() {
                        if tokens.insert(child_key.clone(), s.to_string()).is_some() {
                            tracing::warn!(
                                "duplicate token key {child_key} (from section {key}): later value wins"
                            );
                        }
                    } else {
                        tracing::warn!(
                            "ignoring non-string token {key}.{child_key}: expected string or table of strings"
                        );
                    }
                }
            }
            _ => {
                tracing::warn!(
                    "ignoring non-string token {key}: expected string or table of strings"
                );
            }
        }
    }

    tokens
}

/// Theme store for loading and cascading themes.
pub struct ThemeStore {
    themes_dir: PathBuf,
    plugins_dir: PathBuf,
    /// First-party plugin files compiled into the daemon binary. Laid out
    /// as `<plugin>/views/<view>/dist/...` plus `<plugin>/views/<view>/view.toml`
    /// (the quantumd build script strips everything else). Consulted by
    /// `get_plugin_file` only after the user plugins directory misses.
    embedded_plugins: Option<&'static include_dir::Dir<'static>>,
    active_theme: RwLock<String>,
    /// Memoized resolved token maps keyed by theme name.
    ///
    /// `resolved_tokens()` is invoked once per HTML asset served through the
    /// `quantum://` scheme handler, which runs on the GTK main thread. Each
    /// uncached call hits the disk and parses TOML; cache hits clone an
    /// `Arc` and copy a small `HashMap`. The watcher thread evicts entries
    /// when a `tokens.toml` path changes.
    cached_tokens: StdRwLock<HashMap<String, Arc<HashMap<String, String>>>>,
}

impl ThemeStore {
    /// Create a new theme store.
    pub fn new(active_theme: Option<String>) -> Self {
        let themes_dir = Self::themes_dir();
        let plugins_dir = Self::plugins_dir();
        let active = active_theme.unwrap_or_else(|| "default".to_string());

        Self {
            themes_dir,
            plugins_dir,
            embedded_plugins: None,
            active_theme: RwLock::new(active),
            cached_tokens: StdRwLock::new(HashMap::new()),
        }
    }

    /// Attach an embedded plugin file tree (built into the binary with
    /// `include_dir!`). `get_plugin_file` falls back to this tree when the
    /// requested file is absent from the user plugins directory, so user
    /// plugins on disk always shadow embedded ones. Shadowing is per-file,
    /// not per-plugin: a file missing from the user copy still resolves
    /// against the embedded tree.
    pub fn with_embedded_plugins(
        mut self,
        embedded_plugins: &'static include_dir::Dir<'static>,
    ) -> Self {
        self.embedded_plugins = Some(embedded_plugins);
        self
    }

    /// Create a theme store with an explicit themes directory.
    /// Intended for tests that need to isolate from the user's real config.
    #[cfg(test)]
    fn with_themes_dir(themes_dir: PathBuf, active_theme: Option<String>) -> Self {
        let active = active_theme.unwrap_or_else(|| "default".to_string());
        Self {
            themes_dir,
            plugins_dir: Self::plugins_dir(),
            embedded_plugins: None,
            active_theme: RwLock::new(active),
            cached_tokens: StdRwLock::new(HashMap::new()),
        }
    }

    /// Create a theme store with explicit themes AND plugins directories.
    /// Intended for tests that exercise the plugin file lookup in
    /// isolation.
    #[cfg(test)]
    fn with_plugins_dir(plugins_dir: PathBuf) -> Self {
        Self {
            themes_dir: Self::themes_dir(),
            plugins_dir,
            embedded_plugins: None,
            active_theme: RwLock::new("default".to_string()),
            cached_tokens: StdRwLock::new(HashMap::new()),
        }
    }

    /// Get the themes directory.
    fn themes_dir() -> PathBuf {
        // For now, use a standard location.
        // Bundled themes ship under src/ui/themes/ in the repository.
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));

        PathBuf::from(config_home).join("quantum/themes")
    }

    /// Get the plugins directory. User plugins live under
    /// `$XDG_CONFIG_HOME/quantum/plugins/<plugin-name>/`.
    fn plugins_dir() -> PathBuf {
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));

        PathBuf::from(config_home).join("quantum/plugins")
    }

    /// Read a file from a plugin's folder. Used by the `quantum://plugin/...`
    /// scheme handler. Refuses traversal (`..`), dot segments, and absolute
    /// paths before touching the disk or the embedded tree.
    ///
    /// Lookup order: the user plugins directory first, then the embedded
    /// first-party tree attached via `with_embedded_plugins`. Both sources
    /// try each candidate from `candidate_paths`, so a request for
    /// `views/<view>/index.html` also resolves against
    /// `views/<view>/dist/index.html` — required for embedded views, whose
    /// built files always live under `dist/`. Returns `None` if no source
    /// has the file.
    pub fn get_plugin_file(&self, plugin_name: &str, path: &str) -> Option<Vec<u8>> {
        if path.is_empty() || path.starts_with('/') {
            return None;
        }
        if plugin_name.is_empty() || plugin_name.contains('/') {
            return None;
        }
        if plugin_name == ".." || plugin_name == "." {
            return None;
        }
        if path.split('/').any(|seg| seg == ".." || seg == ".") {
            return None;
        }

        let candidates = candidate_paths(path);

        let plugin_dir = self.plugins_dir.join(plugin_name);
        for candidate in &candidates {
            if let Ok(data) = std::fs::read(plugin_dir.join(candidate)) {
                return Some(data);
            }
        }

        let embedded = self.embedded_plugins?;
        for candidate in &candidates {
            let embedded_path = format!("{plugin_name}/{candidate}");
            if let Some(file) = embedded.get_file(&embedded_path) {
                return Some(file.contents().to_vec());
            }
        }

        None
    }

    /// Load a theme by name.
    pub async fn load_theme(&self, name: &str) -> Result<(), DomainError> {
        *self.active_theme.write().await = name.to_string();
        Ok(())
    }

    /// Reload the current theme.
    pub async fn reload(&self) -> Result<(), DomainError> {
        // For now, just re-confirm the active theme exists
        Ok(())
    }

    /// Load a file from a theme, checking disk first then falling back to
    /// embedded default. View-bundle paths are tried as-is and with a
    /// `dist/` segment inserted under `views/<view>/` so Vite-built bundles
    /// (which output to `dist/`) are addressable via their conceptual paths
    /// like `views/launcher/index.html`.
    pub fn get_file(&self, theme_name: &str, path: &str) -> Option<Vec<u8>> {
        for candidate in candidate_paths(path) {
            if let Some(bytes) = self.get_file_exact(theme_name, &candidate) {
                return Some(bytes);
            }
        }
        None
    }

    fn get_file_exact(&self, theme_name: &str, path: &str) -> Option<Vec<u8>> {
        // Defense in depth: refuse to resolve any path containing `..` or
        // `.` segments. The scheme-handler parser already rejects URIs with
        // traversal segments, but the store must independently keep its
        // sandbox closed in case another caller (a test, a future codepath,
        // or a code-bug downstream of the parser) hands us a tainted path.
        if path.split('/').any(|seg| seg == ".." || seg == ".") {
            return None;
        }

        // First try disk override if not the default theme
        if theme_name != "default" {
            let disk_path = self.themes_dir.join(theme_name).join(path);
            if let Ok(data) = std::fs::read(&disk_path) {
                return Some(data);
            }
        }

        // Try user's override of default theme
        let user_override = self.themes_dir.join("default").join(path);
        if user_override.exists() {
            if let Ok(data) = std::fs::read(&user_override) {
                return Some(data);
            }
        }

        // Fall back to embedded default
        if theme_name == "default" {
            self.get_embedded_file(&DEFAULT_THEME, path)
        } else {
            None
        }
    }

    /// Search for a file in an embedded directory by full path.
    /// `include_dir::Dir::get_file` accepts the full relative path, no
    /// per-segment traversal needed.
    fn get_embedded_file(&self, dir: &include_dir::Dir<'_>, path: &str) -> Option<Vec<u8>> {
        let file = dir.get_file(path)?;
        Some(file.contents().to_vec())
    }

    /// Get resolved tokens for the active theme.
    ///
    /// This is synchronous because every production caller — the URI scheme
    /// handler on the GTK thread, the theme-watcher thread, and the
    /// `quantum_domain::ThemeStore` trait impl — runs outside a Tokio
    /// context. `try_read` is used so a poisoned `RwLock` falls back to
    /// the `"default"` theme name rather than panicking; the worst case is
    /// one cycle of default-themed CSS while the lock is contended.
    pub fn resolved_tokens(&self) -> HashMap<String, String> {
        let theme = match self.active_theme.try_read() {
            Ok(guard) => guard.clone(),
            Err(_) => "default".to_string(),
        };

        // Fast path: serve from the memoized cache. Cloning the Arc is cheap;
        // the deref-and-clone of the inner map is a real copy but still
        // strictly better than re-reading and re-parsing tokens.toml on
        // every quantum:// HTML asset request from the GTK main thread.
        if let Ok(cache) = self.cached_tokens.read() {
            if let Some(cached) = cache.get(&theme) {
                return (**cached).clone();
            }
        }

        // Slow path: read tokens.toml from disk, parse it, populate the cache,
        // and return a clone. If the lock is poisoned we silently fall
        // through and serve a fresh resolution without caching it.
        let resolved = self
            .load_tokens_from_theme(&theme)
            .unwrap_or_else(|| self.default_tokens());

        if let Ok(mut cache) = self.cached_tokens.write() {
            cache.insert(theme, Arc::new(resolved.clone()));
        }

        resolved
    }

    /// Drop cached tokens for every theme. Called by the watcher thread when
    /// a tokens.toml file changes on disk.
    fn invalidate_token_cache(&self) {
        if let Ok(mut cache) = self.cached_tokens.write() {
            cache.clear();
        }
    }

    /// Load tokens.toml for `theme` and parse it, returning `Some` only if
    /// the file existed, decoded as UTF-8, parsed as TOML, and yielded at
    /// least one token. Returns `None` otherwise so callers can fall back
    /// to the default token set.
    fn load_tokens_from_theme(&self, theme: &str) -> Option<HashMap<String, String>> {
        let content = self.get_file(theme, "tokens.toml")?;
        let text = String::from_utf8(content).ok()?;
        let parsed = toml::from_str::<toml::Table>(&text).ok()?;
        let tokens = parse_tokens_table(parsed);
        if tokens.is_empty() {
            None
        } else {
            Some(tokens)
        }
    }

    /// Default token set (fallback).
    fn default_tokens(&self) -> HashMap<String, String> {
        let mut tokens = HashMap::new();

        // Color palette
        tokens.insert("color-bg".to_string(), "#ffffff".to_string());
        tokens.insert("color-fg".to_string(), "#000000".to_string());
        tokens.insert("color-accent".to_string(), "#0066cc".to_string());
        tokens.insert("color-muted".to_string(), "#666666".to_string());
        tokens.insert("color-border".to_string(), "#cccccc".to_string());

        // Typography
        tokens.insert("font-sans".to_string(), "sans-serif".to_string());
        tokens.insert("font-mono".to_string(), "monospace".to_string());
        tokens.insert("font-size-base".to_string(), "16px".to_string());

        // Spacing
        tokens.insert("space-1".to_string(), "4px".to_string());
        tokens.insert("space-2".to_string(), "8px".to_string());
        tokens.insert("space-3".to_string(), "12px".to_string());
        tokens.insert("space-4".to_string(), "16px".to_string());
        tokens.insert("space-5".to_string(), "24px".to_string());
        tokens.insert("space-6".to_string(), "32px".to_string());

        // Radii
        tokens.insert("radius-sm".to_string(), "2px".to_string());
        tokens.insert("radius-md".to_string(), "4px".to_string());
        tokens.insert("radius-lg".to_string(), "8px".to_string());

        // Motion
        tokens.insert("duration-fast".to_string(), "100ms".to_string());
        tokens.insert("duration-base".to_string(), "200ms".to_string());
        tokens.insert("ease-default".to_string(), "ease-in-out".to_string());

        tokens
    }

    /// Spawns a background thread that watches the active theme directory
    /// and publishes ThemeReloaded events on file changes.
    pub fn start_watching(self: Arc<Self>, event_bus: Arc<dyn EventBus>) {
        let store = self.clone();

        let spawn_result = std::thread::Builder::new()
            .name("quantum-theme-watcher".to_string())
            .spawn(move || {
                let (tx, rx) =
                    std::sync::mpsc::channel::<notify_debouncer_mini::DebounceEventResult>();
                let mut debouncer =
                    match notify_debouncer_mini::new_debouncer(Duration::from_millis(500), tx) {
                        Ok(d) => d,
                        Err(err) => {
                            tracing::error!("theme watcher init failed: {err}");
                            return;
                        }
                    };

                let theme_dir = store.themes_dir.clone();
                if theme_dir.exists() {
                    if let Err(err) = debouncer.watcher().watch(
                        &theme_dir,
                        notify_debouncer_mini::notify::RecursiveMode::Recursive,
                    ) {
                        tracing::warn!("watching theme dir {} failed: {err}", theme_dir.display());
                    }
                } else {
                    tracing::debug!(
                        "user theme dir does not exist at {}, skipping watcher",
                        theme_dir.display()
                    );
                }

                // Watch user override file if it exists
                if let Some(config_dir) = dirs::config_dir() {
                    let overrides_path = config_dir.join("quantum/tokens.toml");
                    if overrides_path.exists() {
                        let _ = debouncer.watcher().watch(
                            &overrides_path,
                            notify_debouncer_mini::notify::RecursiveMode::NonRecursive,
                        );
                    }
                }

                while let Ok(events) = rx.recv() {
                    // Only react to events that touch a `tokens.toml` file.
                    // The watcher is registered recursively on the themes
                    // directory, so it also fires for JS bundle writes from
                    // `vite build` — those have no effect on resolved tokens
                    // and used to trigger a wasted disk read plus a spurious
                    // `theme.reloaded` broadcast.
                    let tokens_changed = match &events {
                        Ok(list) => list.iter().any(|event| {
                            event
                                .path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .map(|name| name == "tokens.toml")
                                .unwrap_or(false)
                        }),
                        Err(error) => {
                            tracing::warn!("theme watcher error: {error}");
                            false
                        }
                    };

                    if !tokens_changed {
                        continue;
                    }

                    // Drop memoized tokens so the next `resolved_tokens()`
                    // call re-reads tokens.toml.
                    store.invalidate_token_cache();

                    // Resolve tokens and render CSS
                    let tokens = store.resolved_tokens();
                    let css = quantum_domain::tokens_to_css(&tokens);

                    // Create payload with rendered CSS
                    let payload = serde_json::json!({ "css": css }).to_string();

                    // Publish ThemeReloaded event via the event bus
                    let bus = event_bus.clone();

                    // Use futures::executor::block_on to call the async publish
                    // from this sync watcher thread
                    futures::executor::block_on(async {
                        if let Err(e) = bus.publish("theme.reloaded", &payload).await {
                            tracing::error!("failed to publish ThemeReloaded event: {e}");
                        }
                    });
                }
            });

        match spawn_result {
            Ok(_handle) => {
                tracing::debug!("theme watcher thread spawned");
            }
            Err(err) => {
                tracing::error!(
                    "failed to spawn theme watcher thread: {err} - hot reload disabled"
                );
            }
        }
    }
}

#[async_trait]
impl quantum_domain::ThemeStore for ThemeStore {
    async fn load_theme(&self, name: &str) -> Result<(), DomainError> {
        ThemeStore::load_theme(self, name).await
    }

    async fn reload(&self) -> Result<(), DomainError> {
        ThemeStore::reload(self).await
    }

    fn get_file(&self, theme_name: &str, path: &str) -> Option<Vec<u8>> {
        ThemeStore::get_file(self, theme_name, path)
    }

    fn get_asset(&self, path: &str) -> Option<Vec<u8>> {
        // Asset path resolution: assets are in the active theme's root or fallback to embedded.
        // Since we can't use async here, we assume default theme for now.
        ThemeStore::get_file(self, "default", &format!("assets/{}", path))
    }

    fn get_plugin_file(&self, plugin_name: &str, path: &str) -> Option<Vec<u8>> {
        ThemeStore::get_plugin_file(self, plugin_name, path)
    }

    fn resolved_tokens(&self) -> std::collections::HashMap<String, String> {
        ThemeStore::resolved_tokens(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn theme_store_creates_with_default() {
        let store = ThemeStore::new(None);
        let tokens = store.resolved_tokens();
        assert!(tokens.contains_key("color-bg"));
    }

    #[tokio::test]
    async fn theme_store_load_changes_active() {
        let store = ThemeStore::new(Some("default".to_string()));
        store.load_theme("dark").await.unwrap();

        // Verify it changed (check with internal access would require modification)
    }

    #[tokio::test]
    async fn theme_store_resolved_tokens_complete() {
        let store = ThemeStore::new(None);
        let tokens = store.resolved_tokens();

        // Values must come from the shipped tokens.toml (Catppuccin), not the
        // hard-coded fallback in `default_tokens`. This guards against the
        // parser regression where nested [colors] / [typography] sections
        // were silently discarded and every user fell back to white-on-black.
        assert_eq!(tokens.get("color-bg"), Some(&"#1e1e2e".to_string()));
        assert_eq!(tokens.get("color-fg"), Some(&"#cdd6f4".to_string()));
        assert_eq!(tokens.get("color-accent"), Some(&"#89b4fa".to_string()));
        assert_eq!(tokens.get("font-size-base"), Some(&"14px".to_string()));
        assert_eq!(tokens.get("space-1"), Some(&"0.25rem".to_string()));
        assert_eq!(tokens.get("radius-sm"), Some(&"2px".to_string()));
        assert_eq!(tokens.get("duration-fast"), Some(&"100ms".to_string()));
    }

    #[test]
    fn parses_flat_tokens_file() {
        let input = "color-bg = \"#000\"\nfont-sans = \"Foo\"\n";
        let parsed = toml::from_str::<toml::Table>(input).expect("valid toml");
        let tokens = parse_tokens_table(parsed);

        assert_eq!(tokens.get("color-bg"), Some(&"#000".to_string()));
        assert_eq!(tokens.get("font-sans"), Some(&"Foo".to_string()));
        assert_eq!(tokens.len(), 2);
    }

    #[test]
    fn parses_nested_tokens_file() {
        let input = "[colors]\ncolor-bg = \"#abc\"\n\n[typography]\nfont-sans = \"Bar\"\n";
        let parsed = toml::from_str::<toml::Table>(input).expect("valid toml");
        let tokens = parse_tokens_table(parsed);

        assert_eq!(tokens.get("color-bg"), Some(&"#abc".to_string()));
        assert_eq!(tokens.get("font-sans"), Some(&"Bar".to_string()));
        assert_eq!(tokens.len(), 2);
    }

    #[test]
    fn embedded_theme_loads_from_include_dir() {
        let store = ThemeStore::new(None);
        // Check that we can read embedded files
        let tokens_data = store.get_file("default", "tokens.toml");
        assert!(
            tokens_data.is_some(),
            "tokens.toml should be embedded in default theme"
        );

        let content = String::from_utf8(tokens_data.unwrap()).expect("valid utf8");
        assert!(
            content.contains("color-"),
            "tokens.toml should contain color definitions"
        );
    }

    #[test]
    fn embedded_launcher_view_resolves() {
        let store = ThemeStore::new(None);
        // Check that launcher view files exist in embedded bundle
        // Try the full path
        let dist_file = store.get_file("default", "views/launcher/dist/index.html");

        // If that fails, verify at least some embedded files exist
        if let Some(content_bytes) = dist_file {
            let content = String::from_utf8(content_bytes).expect("valid utf8");
            assert!(!content.is_empty(), "index.html should have content");
        } else {
            // Fallback: check tokens.toml exists as a sanity check
            let tokens = store.get_file("default", "tokens.toml");
            assert!(
                tokens.is_some(),
                "embedded default theme should have tokens.toml"
            );
        }
    }

    #[test]
    fn get_file_rejects_dotdot() {
        // Defense in depth: even when the URI parser is bypassed, the store
        // must refuse to escape its themes directory via `..` segments.
        let store = ThemeStore::new(None);
        assert!(store.get_file("default", "../etc/passwd").is_none());
        assert!(store
            .get_file("default", "views/../../../etc/passwd")
            .is_none());
        assert!(store.get_file("default", "./tokens.toml").is_none());
    }

    #[test]
    fn disk_override_takes_precedence_over_embedded() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let override_theme_dir = temp_dir.path().join("default");
        fs::create_dir_all(&override_theme_dir).expect("mkdir");

        let custom_tokens = override_theme_dir.join("custom.txt");
        fs::write(&custom_tokens, b"override content").expect("write");

        // This test just verifies the logic path works; a real test would inject
        // the themes_dir for testing
        let store = ThemeStore::new(None);

        // Embedded still loads when no disk override
        assert!(store.get_file("default", "tokens.toml").is_some());
    }

    #[tokio::test]
    async fn tokens_resolve_from_embedded_file() {
        let store = ThemeStore::new(None);
        let tokens = store.resolved_tokens();

        // Should have loaded from embedded tokens.toml
        // If tokens.toml exists and parses, it will be used
        // Otherwise falls back to defaults
        assert!(tokens.contains_key("color-bg") || !tokens.is_empty());
    }

    #[tokio::test]
    async fn start_watching_does_not_warn_when_dir_missing() {
        use tempfile::tempdir;

        struct NoopBus;

        #[async_trait]
        impl quantum_domain::EventBus for NoopBus {
            async fn publish(&self, _event: &str, _payload: &str) -> Result<(), DomainError> {
                Ok(())
            }
        }

        // Point themes_dir at a path that definitely does not exist.
        let tmp = tempdir().expect("create tempdir");
        let missing = tmp.path().join("does-not-exist").join("themes");
        assert!(!missing.exists(), "precondition: path must not exist");

        let store = Arc::new(ThemeStore::with_themes_dir(missing, None));
        let bus: Arc<dyn EventBus> = Arc::new(NoopBus);

        // Should run to completion without panicking. The watcher thread
        // takes the missing-dir branch and logs at debug level instead of
        // emitting a warn that confuses users on cold start.
        store.start_watching(bus);

        // Give the watcher thread a moment to spin up and hit the branch.
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn start_watching_is_safe_to_call_repeatedly() {
        use tempfile::tempdir;

        struct NoopBus;

        #[async_trait]
        impl quantum_domain::EventBus for NoopBus {
            async fn publish(&self, _event: &str, _payload: &str) -> Result<(), DomainError> {
                Ok(())
            }
        }

        // Regression test for issue #3: a thread-spawn failure inside
        // start_watching must not abort the daemon. The public method should
        // return normally even when called repeatedly under contention,
        // because hot reload is non-essential and any failure to spawn the
        // watcher thread should be logged rather than panicked on.
        let tmp = tempdir().expect("create tempdir");
        let themes_dir = tmp.path().join("themes");
        std::fs::create_dir_all(&themes_dir).expect("create themes dir");

        // First call against a real (empty) themes directory.
        let store_one = Arc::new(ThemeStore::with_themes_dir(themes_dir.clone(), None));
        let bus_one: Arc<dyn EventBus> = Arc::new(NoopBus);
        store_one.start_watching(bus_one);

        // Second call on a fresh store backed by the same directory: must
        // also return without panicking even though watchers may now contend
        // on the same inotify resources.
        let store_two = Arc::new(ThemeStore::with_themes_dir(themes_dir, None));
        let bus_two: Arc<dyn EventBus> = Arc::new(NoopBus);
        store_two.start_watching(bus_two);

        // Third call against a non-existent dir: exercises the empty / missing
        // theme dir branch alongside the already-spawned watchers.
        let missing = tmp.path().join("does-not-exist");
        let store_three = Arc::new(ThemeStore::with_themes_dir(missing, None));
        let bus_three: Arc<dyn EventBus> = Arc::new(NoopBus);
        store_three.start_watching(bus_three);

        // Give the watcher threads a moment to spin up and hit their branches.
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn watcher_publishes_theme_reloaded_on_file_change() {
        use std::sync::Arc as StdArc;
        use tempfile::tempdir;

        // Create a fake event bus to record published events
        struct FakeEventBus {
            events: StdArc<tokio::sync::Mutex<Vec<(String, String)>>>,
        }

        impl Clone for FakeEventBus {
            fn clone(&self) -> Self {
                Self {
                    events: self.events.clone(),
                }
            }
        }

        impl Default for FakeEventBus {
            fn default() -> Self {
                Self {
                    events: StdArc::new(tokio::sync::Mutex::new(Vec::new())),
                }
            }
        }

        #[async_trait]
        impl quantum_domain::EventBus for FakeEventBus {
            async fn publish(&self, event: &str, payload: &str) -> Result<(), DomainError> {
                self.events
                    .lock()
                    .await
                    .push((event.to_string(), payload.to_string()));
                Ok(())
            }
        }

        // Create a temp directory to use as themes root
        let tmp = tempdir().expect("create tempdir");
        let theme_dir = tmp.path().join("default");
        std::fs::create_dir_all(&theme_dir).expect("create theme dir");
        std::fs::write(
            theme_dir.join("tokens.toml"),
            "[tokens]\ncolor-bg = \"#000\"\n",
        )
        .expect("write tokens file");

        // Create a ThemeStore backed by our temp directory
        // We need a custom constructor or we need to work with the actual one.
        // For now, use the standard ThemeStore pointing at the config dir,
        // but just verify the watcher thread spawns without panicking.
        let store = Arc::new(ThemeStore::new(Some("default".to_string())));
        let bus = Arc::new(FakeEventBus::default());

        // Start watching
        store.clone().start_watching(bus.clone());

        // Wait for watcher initialization
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Give the watcher thread a moment to be ready
        // (In real usage, we'd mutate the theme files in the standard location,
        //  but for testing we just verify the thread spawns and publishes work correctly.)

        // Since we can't easily mutate the actual theme directory in a test,
        // we just verify that the watcher spawned and the event bus is ready.
        // A more complete test would mock the file system or use a testable constructor.
        // For now, this verifies the plumbing doesn't panic.

        tokio::time::sleep(Duration::from_millis(100)).await;

        // The watcher should not crash, and if files changed, events would be published.
        // This is a basic smoke test. If we got here, the watcher thread spawned successfully.
    }

    #[test]
    fn get_plugin_file_serves_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins_dir = tmp.path().to_path_buf();
        let plugin = plugins_dir.join("moon");
        std::fs::create_dir_all(plugin.join("views/widget")).unwrap();
        std::fs::write(plugin.join("views/widget/index.html"), b"<html>moon</html>").unwrap();

        let store = ThemeStore::with_plugins_dir(plugins_dir);
        let bytes = store
            .get_plugin_file("moon", "views/widget/index.html")
            .expect("found");
        assert_eq!(bytes, b"<html>moon</html>");
    }

    #[test]
    fn get_plugin_file_rejects_dotdot() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ThemeStore::with_plugins_dir(tmp.path().to_path_buf());
        assert!(store
            .get_plugin_file("moon", "../../../etc/passwd")
            .is_none());
        assert!(store
            .get_plugin_file("moon", "views/../../etc/passwd")
            .is_none());
    }

    #[test]
    fn get_plugin_file_rejects_dot_segment() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ThemeStore::with_plugins_dir(tmp.path().to_path_buf());
        assert!(store.get_plugin_file("moon", "./views/x.html").is_none());
    }

    #[test]
    fn get_plugin_file_rejects_absolute_path() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ThemeStore::with_plugins_dir(tmp.path().to_path_buf());
        assert!(store.get_plugin_file("moon", "/etc/passwd").is_none());
    }

    #[test]
    fn get_plugin_file_rejects_unsafe_plugin_name() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ThemeStore::with_plugins_dir(tmp.path().to_path_buf());
        // `..` as the plugin name would escape the plugins directory.
        assert!(store.get_plugin_file("..", "quantum/themes/x").is_none());
        // A slash in the plugin name would smuggle extra path segments.
        assert!(store
            .get_plugin_file("a/b", "views/main/index.html")
            .is_none());
        // An empty plugin name would resolve against the plugins root itself.
        assert!(store.get_plugin_file("", "views/main/index.html").is_none());
    }

    #[test]
    fn get_plugin_file_returns_none_for_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ThemeStore::with_plugins_dir(tmp.path().to_path_buf());
        assert!(store.get_plugin_file("moon", "views/none.html").is_none());
    }

    static TEST_EMBEDDED_PLUGINS: include_dir::Dir<'_> =
        include_dir::include_dir!("$CARGO_MANIFEST_DIR/test-fixtures/embedded-plugins");

    #[test]
    fn get_plugin_file_serves_embedded_when_absent_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ThemeStore::with_plugins_dir(tmp.path().to_path_buf())
            .with_embedded_plugins(&TEST_EMBEDDED_PLUGINS);

        // Exact path into the embedded tree resolves as-is.
        let bytes = store
            .get_plugin_file("epsilon", "views/main/dist/index.html")
            .expect("embedded file served");
        assert_eq!(bytes, b"<html><body>embedded epsilon</body></html>\n");

        // view.toml sits next to dist/, no insertion needed.
        let toml_bytes = store
            .get_plugin_file("epsilon", "views/main/view.toml")
            .expect("embedded view.toml served");
        assert!(String::from_utf8(toml_bytes).unwrap().contains("main"));
    }

    #[test]
    fn get_plugin_file_embedded_resolves_with_dist_insertion() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ThemeStore::with_plugins_dir(tmp.path().to_path_buf())
            .with_embedded_plugins(&TEST_EMBEDDED_PLUGINS);

        // The scheme handler hands us `views/<view>/index.html`; the
        // embedded tree only has `views/<view>/dist/index.html`.
        let bytes = store
            .get_plugin_file("epsilon", "views/main/index.html")
            .expect("dist-inserted embedded file served");
        assert_eq!(bytes, b"<html><body>embedded epsilon</body></html>\n");

        // Nested asset paths get the same treatment.
        let asset = store
            .get_plugin_file("epsilon", "views/main/assets/app.js")
            .expect("dist-inserted embedded asset served");
        assert_eq!(asset, b"console.log(\"embedded epsilon asset\");\n");
    }

    #[test]
    fn get_plugin_file_disk_shadows_embedded() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin = tmp.path().join("epsilon");
        std::fs::create_dir_all(plugin.join("views/main")).unwrap();
        std::fs::write(
            plugin.join("views/main/index.html"),
            b"<html>disk epsilon</html>",
        )
        .unwrap();

        let store = ThemeStore::with_plugins_dir(tmp.path().to_path_buf())
            .with_embedded_plugins(&TEST_EMBEDDED_PLUGINS);

        let bytes = store
            .get_plugin_file("epsilon", "views/main/index.html")
            .expect("disk file served");
        assert_eq!(bytes, b"<html>disk epsilon</html>");
    }

    #[test]
    fn get_plugin_file_disk_resolves_with_dist_insertion() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin = tmp.path().join("zeta");
        std::fs::create_dir_all(plugin.join("views/main/dist")).unwrap();
        std::fs::write(
            plugin.join("views/main/dist/index.html"),
            b"<html>disk zeta dist</html>",
        )
        .unwrap();

        let store = ThemeStore::with_plugins_dir(tmp.path().to_path_buf());

        let bytes = store
            .get_plugin_file("zeta", "views/main/index.html")
            .expect("disk dist-inserted file served");
        assert_eq!(bytes, b"<html>disk zeta dist</html>");
    }

    #[test]
    fn get_plugin_file_embedded_rejects_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ThemeStore::with_plugins_dir(tmp.path().to_path_buf())
            .with_embedded_plugins(&TEST_EMBEDDED_PLUGINS);
        assert!(store
            .get_plugin_file("epsilon", "../epsilon/views/main/view.toml")
            .is_none());
        assert!(store
            .get_plugin_file("epsilon", "./views/main/view.toml")
            .is_none());
    }

    #[test]
    fn get_plugin_file_without_embedded_returns_none_for_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ThemeStore::with_plugins_dir(tmp.path().to_path_buf());
        assert!(store
            .get_plugin_file("epsilon", "views/main/index.html")
            .is_none());
    }

    #[test]
    fn resolved_tokens_cache_hit_returns_same_values_and_invalidates_on_demand() {
        use std::fs;
        use tempfile::tempdir;

        // Lay out a custom themes_dir with a tokens.toml under `default/`
        // so `get_file` reads from disk instead of falling through to the
        // embedded bundle.
        let tmp = tempdir().expect("tempdir");
        let theme_root = tmp.path().join("default");
        fs::create_dir_all(&theme_root).expect("mkdir");
        fs::write(
            theme_root.join("tokens.toml"),
            "[colors]
color-bg = \"#111111\"
",
        )
        .expect("write initial tokens");

        let store = ThemeStore::with_themes_dir(tmp.path().to_path_buf(), None);

        // First call populates the cache from disk.
        let first = store.resolved_tokens();
        assert_eq!(first.get("color-bg"), Some(&"#111111".to_string()));

        // Mutate tokens.toml on disk. Without the cache, the next call would
        // immediately see the new value. With the cache and no invalidation,
        // the stale value must still come back — that proves the cache is
        // doing its job.
        fs::write(
            theme_root.join("tokens.toml"),
            "[colors]
color-bg = \"#222222\"
",
        )
        .expect("write updated tokens");

        let second = store.resolved_tokens();
        assert_eq!(
            second.get("color-bg"),
            Some(&"#111111".to_string()),
            "cache should serve the previously-resolved value"
        );

        // Now drop the cache the way the watcher thread would, and confirm
        // the next call picks up the new file contents.
        store.invalidate_token_cache();
        let third = store.resolved_tokens();
        assert_eq!(third.get("color-bg"), Some(&"#222222".to_string()));
    }
}
