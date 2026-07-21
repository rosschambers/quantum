use async_trait::async_trait;
use freedesktop_desktop_entry::DesktopEntry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, MatchScore, ProviderId, ProviderSource, Query,
    ShellExecutor,
};

/// Information about a desktop application.
///
/// `name_lower`, `generic_name_lower`, and `keywords_lower` are
/// precomputed at scan time so the search hot path does not allocate a
/// new lowercase `String` for every app on every keystroke.
#[derive(Debug, Clone)]
struct AppInfo {
    id: String,
    name: String,
    generic_name: Option<String>,
    exec: String,
    /// The desktop entry's `Icon=` value (a freedesktop icon name or an
    /// absolute path). Resolved to a concrete file path at search time.
    icon: Option<String>,
    name_lower: String,
    generic_name_lower: Option<String>,
    keywords_lower: Vec<String>,
}

impl AppInfo {
    fn new(
        id: String,
        name: String,
        generic_name: Option<String>,
        keywords: Vec<String>,
        exec: String,
        icon: Option<String>,
    ) -> Self {
        let name_lower = name.to_lowercase();
        let generic_name_lower = generic_name.as_ref().map(|s| s.to_lowercase());
        let keywords_lower = keywords.iter().map(|k| k.to_lowercase()).collect();
        Self {
            id,
            name,
            generic_name,
            exec,
            icon,
            name_lower,
            generic_name_lower,
            keywords_lower,
        }
    }
}

/// Resolve a desktop entry icon reference to a concrete file path.
///
/// If `icon` is already an absolute path that exists on disk, it is returned
/// as-is. Otherwise it is treated as a freedesktop icon name and looked up
/// against the installed icon themes (preferring a 48px raster size). Returns
/// `None` when nothing resolves, so callers emit no `IconRef` rather than a
/// name the webview cannot load.
fn resolve_icon_path(icon: Option<&str>) -> Option<std::path::PathBuf> {
    let name = icon?;
    let as_path = std::path::Path::new(name);
    if as_path.is_absolute() && as_path.exists() {
        return Some(as_path.to_path_buf());
    }
    freedesktop_icons::lookup(name).with_size(48).find()
}

/// Parse every `*.desktop` file in `dir` into `(id, AppInfo)` pairs, in
/// directory iteration order.
///
/// This performs blocking filesystem work (`read_dir` plus `read_to_string`)
/// and is intended to run on a blocking thread pool. Unreadable directories
/// and unparseable files are skipped silently, mirroring the previous
/// best-effort behavior. The caller deduplicates by id (first seen wins).
fn parse_desktop_directory(dir: &Path) -> Vec<(String, AppInfo)> {
    let mut parsed: Vec<(String, AppInfo)> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // Directory doesn't exist or can't be read, skip it.
        Err(_) => return parsed,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(de) = DesktopEntry::decode(&path, &content) else {
            continue;
        };
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let app_name = de
            .name(None)
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let generic_name = de.generic_name(None).map(|s| s.to_string());
        let keywords: Vec<String> = de
            .keywords()
            .unwrap_or_default()
            .split(';')
            .filter(|s: &&str| !s.is_empty())
            .map(|s: &str| s.to_string())
            .collect();
        let exec = de.exec().unwrap_or_default().to_string();
        // Prefer the entry's declared Icon= key; fall back to the desktop file
        // stem, which is conventionally also the icon name.
        let icon = de
            .icon()
            .map(|s| s.to_string())
            .or_else(|| Some(name.to_string()));
        parsed.push((
            name.to_string(),
            AppInfo::new(
                name.to_string(),
                app_name,
                generic_name,
                keywords,
                exec,
                icon,
            ),
        ));
    }
    parsed
}

/// Default number of usage-ranked apps shown for an empty query when the
/// caller does not specify a limit.
const DEFAULT_EMPTY_QUERY_LIMIT: usize = 12;

/// Provider for desktop applications (*.desktop files).
pub struct DesktopAppsProvider {
    id: ProviderId,
    apps: RwLock<Vec<AppInfo>>,
    executor: Arc<dyn ShellExecutor>,
    /// Launch-usage tracking, used to rank the default apps shown on an empty
    /// query and updated on every launch. Behind a Mutex because `invoke`
    /// records launches through a shared `&self`.
    usage: tokio::sync::Mutex<crate::app_usage::UsageStore>,
    /// Memoized icon-name to resolved-path lookups. Icon resolution walks the
    /// installed icon themes on disk, so each distinct icon name is resolved at
    /// most once for the lifetime of the provider; repeated keystrokes reuse
    /// the cached result instead of re-walking the filesystem. A `None` value
    /// caches a negative result (the name resolved to nothing). Behind a
    /// synchronous Mutex because it is touched from `search` through `&self`
    /// without crossing an await point while held.
    icon_cache: Mutex<HashMap<String, Option<PathBuf>>>,
}

impl DesktopAppsProvider {
    /// Create a new DesktopAppsProvider by scanning application directories.
    pub async fn new(executor: Arc<dyn ShellExecutor>) -> Result<Self, DomainError> {
        let mut provider = Self {
            id: ProviderId::from("desktop-apps"),
            apps: RwLock::new(Vec::new()),
            executor,
            usage: tokio::sync::Mutex::new(crate::app_usage::UsageStore::load()),
            icon_cache: Mutex::new(HashMap::new()),
        };

        provider.scan_apps().await?;
        Ok(provider)
    }

    /// Scan for desktop files in standard locations.
    ///
    /// Walks `XDG_DATA_HOME` first, then each entry of `XDG_DATA_DIRS`
    /// left-to-right. Per the XDG Base Directory Specification, the
    /// first occurrence of a given desktop id wins; later directories
    /// only contribute ids not already seen.
    async fn scan_apps(&mut self) -> Result<(), DomainError> {
        // Scan in common locations
        let data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
            format!("{}/.local/share", std::env::var("HOME").unwrap_or_default())
        });
        let data_dirs = std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());

        let mut dirs = vec![data_home];
        dirs.extend(data_dirs.split(':').map(|s| s.to_string()));

        let mut by_id: HashMap<String, AppInfo> = HashMap::new();
        for base_dir in dirs {
            let app_dir = Path::new(&base_dir).join("applications");
            if app_dir.exists() {
                self.scan_directory(&app_dir, &mut by_id).await?;
            }
        }

        let mut apps: Vec<AppInfo> = by_id.into_values().collect();

        // Sort by name
        apps.sort_by(|a, b| a.name.cmp(&b.name));

        *self.apps.write().await = apps;
        Ok(())
    }

    /// Scan a single directory for .desktop files, inserting each parsed
    /// entry into `by_id` only if the id is not already present (first
    /// seen wins, per XDG Base Directory Specification).
    ///
    /// The directory walk and file parsing are blocking filesystem work, so
    /// they run on Tokio's blocking pool rather than parking the async
    /// runtime. The parsed entries are merged into `by_id` afterwards,
    /// preserving the first-seen-wins ordering within and across directories.
    async fn scan_directory(
        &self,
        dir: &Path,
        by_id: &mut HashMap<String, AppInfo>,
    ) -> Result<(), DomainError> {
        let owned_dir = dir.to_path_buf();
        let parsed = tokio::task::spawn_blocking(move || parse_desktop_directory(&owned_dir))
            .await
            .map_err(|e| DomainError::Unsupported(format!("desktop apps scan join error: {e}")))?;
        for (id, app) in parsed {
            by_id.entry(id).or_insert(app);
        }
        Ok(())
    }

    /// Resolve an icon name to an [`IconRef`], memoizing the result per icon
    /// name so a given name is only ever walked off disk once.
    ///
    /// The cache stores the resolved [`PathBuf`] (or `None` for a negative
    /// result), keyed by the raw icon name. A poisoned mutex falls back to an
    /// uncached resolution rather than failing the search.
    fn resolve_icon_cached(&self, icon: Option<&str>) -> Option<quantum_domain::IconRef> {
        let name = icon?;
        if let Ok(cache) = self.icon_cache.lock() {
            if let Some(cached) = cache.get(name) {
                return cached.clone().map(quantum_domain::IconRef::Path);
            }
        }
        let resolved = resolve_icon_path(Some(name));
        if let Ok(mut cache) = self.icon_cache.lock() {
            cache.insert(name.to_string(), resolved.clone());
        }
        resolved.map(quantum_domain::IconRef::Path)
    }

    /// Build a [`Match`] for an app, resolving its icon to a concrete file
    /// path. An unresolved icon yields no `IconRef` rather than a name the
    /// webview cannot load. Icon resolution is memoized, so this should only
    /// be called for matches that survive sorting and truncation.
    fn match_from_app(&self, app: &AppInfo, score: f32) -> Match {
        let icon = self.resolve_icon_cached(app.icon.as_deref());
        Match {
            id: app.id.clone(),
            provider: self.id.clone(),
            title: app.name.clone(),
            subtitle: app.generic_name.clone(),
            icon,
            score: MatchScore::new(score),
            action: Action::Launch {
                desktop_id: app.id.clone(),
            },
            actions: vec![],
        }
    }

    /// Tokenize a desktop entry `Exec=` value into an argv vector following the
    /// freedesktop Desktop Entry Specification quoting and field-code rules.
    ///
    /// Rules honored:
    /// - Double-quoted arguments may contain spaces. Inside double quotes a
    ///   backslash escapes the characters the specification reserves: `"`,
    ///   `` ` ``, `$`, and `\`. A backslash before any other character is kept
    ///   literally.
    /// - `%%` is unescaped to a single literal `%`.
    /// - The defined field codes (`%f %F %u %U %d %D %n %N %i %c %k %v %m`)
    ///   are removed wherever they appear.
    /// - A `%` followed by any other character is a non-standard or deprecated
    ///   field code: the `%` is dropped and the following character is kept.
    ///   A trailing lone `%` is dropped. This means a Steam-style
    ///   `%command%` placeholder (which is not a valid `Exec` field code)
    ///   degrades to `ommand` rather than being passed through verbatim or
    ///   crashing.
    ///
    /// Used for both the displayed/cleaned exec and the argv handed to
    /// `spawn_detached`, so the two always agree.
    fn tokenize_exec(exec: &str) -> Vec<String> {
        const FIELD_CODES: &[char] = &[
            'f', 'F', 'u', 'U', 'd', 'D', 'n', 'N', 'i', 'c', 'k', 'v', 'm',
        ];
        let chars: Vec<char> = exec.chars().collect();
        let mut tokens: Vec<String> = Vec::new();
        let mut current = String::new();
        // Whether a token is currently open. Set when a quote opens or a
        // character is appended, so that an explicit empty quoted argument
        // ("") is preserved while a field code that resolves to nothing is
        // dropped.
        let mut token_open = false;
        let mut in_quotes = false;
        let mut index = 0;
        while index < chars.len() {
            let current_char = chars[index];
            if in_quotes {
                match current_char {
                    '"' => {
                        in_quotes = false;
                        index += 1;
                    }
                    '\\' => match chars.get(index + 1) {
                        Some(next) if matches!(next, '"' | '`' | '$' | '\\') => {
                            current.push(*next);
                            index += 2;
                        }
                        _ => {
                            current.push('\\');
                            index += 1;
                        }
                    },
                    '%' if chars.get(index + 1) == Some(&'%') => {
                        current.push('%');
                        index += 2;
                    }
                    other => {
                        current.push(other);
                        index += 1;
                    }
                }
                continue;
            }

            if current_char.is_whitespace() {
                if token_open {
                    tokens.push(std::mem::take(&mut current));
                    token_open = false;
                }
                index += 1;
            } else if current_char == '"' {
                in_quotes = true;
                token_open = true;
                index += 1;
            } else if current_char == '%' {
                match chars.get(index + 1) {
                    Some('%') => {
                        current.push('%');
                        token_open = true;
                        index += 2;
                    }
                    Some(code) if FIELD_CODES.contains(code) => {
                        // Defined field code: remove it entirely.
                        index += 2;
                    }
                    Some(_) => {
                        // Non-standard field code: drop the `%`, keep the
                        // following character (processed on the next iteration).
                        index += 1;
                    }
                    None => {
                        // Trailing lone `%`: drop it.
                        index += 1;
                    }
                }
            } else {
                current.push(current_char);
                token_open = true;
                index += 1;
            }
        }
        if token_open {
            tokens.push(current);
        }
        tokens
    }
}

#[async_trait]
impl ProviderSource for DesktopAppsProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, q: &Query) -> Result<Vec<Match>, DomainError> {
        let apps = self.apps.read().await;

        // Empty query: return a usage-ranked set of default apps rather than
        // running the substring scorer (which would otherwise match every app
        // via the `"".contains("")` accident). Apps with no launch history
        // fall back to the existing alphabetical order of `apps`.
        if q.text.trim().is_empty() {
            let limit = q
                .limit
                .map(|l| l as usize)
                .unwrap_or(DEFAULT_EMPTY_QUERY_LIMIT);
            let ids: Vec<String> = apps.iter().map(|a| a.id.clone()).collect();
            let ranked_ids = self.usage.lock().await.rank(&ids);
            // Index apps by id once so the ranked-id lookup is O(1) per id
            // rather than a linear scan of every app per ranked id.
            let by_id: HashMap<&str, &AppInfo> = apps.iter().map(|a| (a.id.as_str(), a)).collect();
            // Resolve icons only for the bounded set that survives truncation.
            let survivors: Vec<&AppInfo> = ranked_ids
                .iter()
                .take(limit)
                .filter_map(|id| by_id.get(id.as_str()).copied())
                .collect();
            // Preserve the ranked order: the launcher renders matches as-is,
            // but downstream aggregation may re-sort by score, so give earlier
            // (more relevant) defaults a marginally higher score.
            let count = survivors.len();
            let matches: Vec<Match> = survivors
                .iter()
                .enumerate()
                .map(|(idx, app)| {
                    let score = 1.0 - (idx as f32 / count.max(1) as f32 * 0.001);
                    self.match_from_app(app, score)
                })
                .collect();
            return Ok(matches);
        }

        let query_lower = q.text.to_lowercase();
        // Collect (score, app) pairs first and defer icon resolution until
        // after sorting and truncation, so the per-keystroke filesystem work
        // is bounded by `limit` rather than the number of scoring matches.
        let mut scored: Vec<(f32, &AppInfo)> = Vec::new();

        for app in apps.iter() {
            let name_lower = app.name_lower.as_str();
            let generic_lower = app.generic_name_lower.as_deref();

            // Simple substring matching with scoring based on position
            let name_score = if name_lower.contains(&query_lower) {
                let pos = name_lower.find(&query_lower).unwrap_or(0);
                // Earlier match = higher score
                1.0 - (pos as f32 / name_lower.len() as f32 * 0.5)
            } else {
                0.0
            };

            let generic_score = if let Some(gn_lower) = generic_lower {
                if gn_lower.contains(&query_lower) {
                    0.6 // Lower weight than name
                } else {
                    0.0
                }
            } else {
                0.0
            };

            // Keywords are XDG Keywords= entries. We weight them below name
            // and generic name so a literal name hit always outranks a
            // keyword-only hit, but searching by category ("browser",
            // "editor") still surfaces relevant apps whose name does not
            // contain the term.
            let keyword_score = if app.keywords_lower.iter().any(|k| k.contains(&query_lower)) {
                0.4
            } else {
                0.0
            };

            let combined_score = name_score.max(generic_score).max(keyword_score);

            if combined_score > 0.1 {
                scored.push((combined_score, app));
            }
        }

        // Sort by score descending
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Limit results before resolving icons, so icon resolution touches at
        // most `limit` apps per keystroke instead of every scoring match.
        if let Some(limit) = q.limit {
            scored.truncate(limit as usize);
        }

        let matches: Vec<Match> = scored
            .into_iter()
            .map(|(score, app)| self.match_from_app(app, score))
            .collect();

        Ok(matches)
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Launch { desktop_id } => {
                let apps = self.apps.read().await;
                if let Some(app) = apps.iter().find(|a| &a.id == desktop_id) {
                    // Use the same spec-aware tokenizer for the argv as for the
                    // cleaned display string, so quoted arguments survive intact
                    // and field codes are removed consistently.
                    let command: Vec<String> = Self::tokenize_exec(&app.exec);

                    if !command.is_empty() {
                        self.executor.spawn_detached(&command).await?;
                        // Record the launch for usage-ranked defaults. A
                        // persistence failure is logged, not fatal: the launch
                        // itself already succeeded.
                        if let Err(err) = self.usage.lock().await.record(desktop_id) {
                            tracing::warn!("failed to persist app usage: {err}");
                        }
                        return Ok(ActionOutcome {
                            message: Some(format!("Launched {}", app.name)),
                        });
                    }
                }
                Err(DomainError::Unsupported(format!(
                    "app not found: {}",
                    desktop_id
                )))
            }
            _ => Err(DomainError::Unsupported(
                "only Launch action supported".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A usage store backed by a throwaway temp file so tests never touch the
    /// real `$XDG_DATA_HOME` and never share state with each other.
    fn test_usage() -> tokio::sync::Mutex<crate::app_usage::UsageStore> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "quantum-test-usage-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        tokio::sync::Mutex::new(crate::app_usage::UsageStore::with_path(path))
    }

    struct FakeExecutor {
        spawned: Arc<RwLock<Vec<Vec<String>>>>,
    }

    impl FakeExecutor {
        fn new() -> Self {
            Self {
                spawned: Arc::new(RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl ShellExecutor for FakeExecutor {
        async fn run_with_timeout(
            &self,
            _command: &[String],
            _timeout_ms: u64,
        ) -> Result<quantum_domain::ShellOutput, DomainError> {
            Ok(quantum_domain::ShellOutput {
                stdout: String::new(),
                stderr: String::new(),
                status: 0,
            })
        }

        async fn spawn_detached(&self, command: &[String]) -> Result<(), DomainError> {
            self.spawned.write().await.push(command.to_vec());
            Ok(())
        }
    }

    #[tokio::test]
    async fn clean_exec_strips_field_codes() {
        let exec = "firefox %u %U";
        let tokens = DesktopAppsProvider::tokenize_exec(exec);
        assert_eq!(tokens, vec!["firefox".to_string()]);
    }

    #[test]
    fn tokenize_exec_strips_simple_field_code() {
        assert_eq!(
            DesktopAppsProvider::tokenize_exec("firefox %u"),
            vec!["firefox".to_string()]
        );
    }

    #[test]
    fn tokenize_exec_keeps_quoted_argument_with_space() {
        assert_eq!(
            DesktopAppsProvider::tokenize_exec("\"my app\" --flag"),
            vec!["my app".to_string(), "--flag".to_string()]
        );
    }

    #[test]
    fn tokenize_exec_removes_field_code_between_args() {
        assert_eq!(
            DesktopAppsProvider::tokenize_exec("app %U file"),
            vec!["app".to_string(), "file".to_string()]
        );
    }

    #[test]
    fn tokenize_exec_unescapes_double_percent() {
        assert_eq!(
            DesktopAppsProvider::tokenize_exec("app %%literal"),
            vec!["app".to_string(), "%literal".to_string()]
        );
    }

    #[test]
    fn tokenize_exec_multiple_quoted_args() {
        assert_eq!(
            DesktopAppsProvider::tokenize_exec("app \"a b\" c"),
            vec!["app".to_string(), "a b".to_string(), "c".to_string()]
        );
    }

    /// A backslash inside double quotes escapes the characters the spec
    /// reserves (`"`, `` ` ``, `$`, `\`), so `"a\"b"` is a single argument
    /// containing a literal double quote.
    #[test]
    fn tokenize_exec_escaped_quote_inside_quotes() {
        assert_eq!(
            DesktopAppsProvider::tokenize_exec("app \"a\\\"b\""),
            vec!["app".to_string(), "a\"b".to_string()]
        );
    }

    /// Steam writes a `%command%` placeholder into launch options, which is
    /// NOT a valid Desktop Entry `Exec` field code. `%c` is a real field code
    /// and is stripped wherever it appears, and a trailing lone `%` is
    /// dropped, so the non-standard token degrades to `ommand` rather than
    /// crashing or being passed through verbatim.
    #[test]
    fn tokenize_exec_nonstandard_steam_field_code_degrades() {
        assert_eq!(
            DesktopAppsProvider::tokenize_exec("steam %command%"),
            vec!["steam".to_string(), "ommand".to_string()]
        );
    }

    /// All defined field codes are removed and `%%` collapses to a literal
    /// percent.
    #[test]
    fn tokenize_exec_removes_all_defined_field_codes() {
        assert_eq!(
            DesktopAppsProvider::tokenize_exec("app %f %F %u %U %d %D %n %N %i %c %k %v %m tail"),
            vec!["app".to_string(), "tail".to_string()]
        );
    }

    #[tokio::test]
    async fn scan_temp_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let apps_dir = temp_dir.path().join("applications");
        std::fs::create_dir(&apps_dir).unwrap();

        // Copy test fixtures
        let fixture_content = r#"[Desktop Entry]
Name=TestApp
GenericName=Test Application
Exec=testapp %u
Type=Application"#;

        std::fs::write(apps_dir.join("testapp.desktop"), fixture_content).unwrap();

        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(Vec::new()),
            executor,
            usage: test_usage(),
            icon_cache: Mutex::new(HashMap::new()),
        };

        provider
            .scan_directory(&apps_dir, &mut HashMap::new())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn search_with_fixture_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let apps_dir = temp_dir.path().join("applications");
        std::fs::create_dir(&apps_dir).unwrap();

        // Create test fixtures
        std::fs::write(
            apps_dir.join("firefox.desktop"),
            r#"[Desktop Entry]
Name=Firefox
GenericName=Web Browser
Exec=firefox %u
Type=Application"#,
        )
        .unwrap();

        std::fs::write(
            apps_dir.join("code.desktop"),
            r#"[Desktop Entry]
Name=Visual Studio Code
GenericName=Text Editor
Exec=code %u
Type=Application"#,
        )
        .unwrap();

        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(Vec::new()),
            executor,
            usage: test_usage(),
            icon_cache: Mutex::new(HashMap::new()),
        };

        // Manually scan the temp directory
        let mut by_id: HashMap<String, AppInfo> = HashMap::new();
        provider
            .scan_directory(&apps_dir, &mut by_id)
            .await
            .unwrap();
        *provider.apps.write().await = by_id.into_values().collect();

        // Search for Firefox
        let query = Query::new("fox");
        let matches = provider.search(&query).await.unwrap();

        assert!(!matches.is_empty());
        assert_eq!(matches[0].title, "Firefox");
    }

    /// A search query that matches only via Keywords= (not Name or
    /// GenericName) must still produce a hit. Real-world example: the
    /// firefox.desktop file shipped by Mozilla declares
    /// `Keywords=Internet;WWW;Browser;Web;Explorer;` — typing "browser"
    /// into the launcher must surface Firefox even though "browser" only
    /// appears in keywords, not in the title.
    #[tokio::test]
    async fn search_matches_via_keywords_only() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(vec![AppInfo::new(
                "firefox".to_string(),
                "Firefox".to_string(),
                Some("Internet".to_string()),
                vec![
                    "WWW".to_string(),
                    "Browser".to_string(),
                    "Web".to_string(),
                    "Explorer".to_string(),
                ],
                "firefox".to_string(),
                None,
            )]),
            executor,
            usage: test_usage(),
            icon_cache: Mutex::new(HashMap::new()),
        };

        let query = Query::new("browser");
        let matches = provider.search(&query).await.unwrap();

        assert!(
            !matches.is_empty(),
            "expected at least one match when query hits Keywords= entry"
        );
        assert_eq!(matches[0].id, "firefox");
    }

    /// Even when a keyword matches, a name match for a different query
    /// must outrank a keyword-only match. Otherwise typing the literal
    /// name of an app would be ambiguous with keyword hits.
    #[tokio::test]
    async fn search_name_match_outranks_keyword_match() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(vec![
                AppInfo::new(
                    "firefox".to_string(),
                    "Firefox".to_string(),
                    None,
                    vec!["Browser".to_string()],
                    "firefox".to_string(),
                    None,
                ),
                AppInfo::new(
                    "browser-chooser".to_string(),
                    "Browser Chooser".to_string(),
                    None,
                    vec![],
                    "chooser".to_string(),
                    None,
                ),
            ]),
            executor,
            usage: test_usage(),
            icon_cache: Mutex::new(HashMap::new()),
        };

        let query = Query::new("browser");
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(
            matches[0].id, "browser-chooser",
            "name match should outrank keyword-only match"
        );
    }

    /// An app whose icon name cannot be resolved against any installed icon
    /// theme must yield `icon: None` in the produced `Match` — never a bogus
    /// `IconRef::Name` that the webview cannot load.
    #[tokio::test]
    async fn search_unresolvable_icon_yields_none() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(vec![AppInfo::new(
                "firefox".to_string(),
                "Firefox".to_string(),
                Some("Web Browser".to_string()),
                vec![],
                "firefox".to_string(),
                Some("definitely-not-a-real-icon-name-xyz123".to_string()),
            )]),
            executor,
            usage: test_usage(),
            icon_cache: Mutex::new(HashMap::new()),
        };

        let query = Query::new("firefox");
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(matches.len(), 1);
        assert!(
            matches[0].icon.is_none(),
            "unresolvable icon name must produce no IconRef, got {:?}",
            matches[0].icon
        );
    }

    /// `resolve_icon_path` returns an absolute path verbatim when the file
    /// exists on disk. This exercises our own resolution branch
    /// deterministically; the icon-name -> theme-file lookup is delegated to
    /// the `freedesktop-icons` crate and depends on process-global system
    /// state, so it is not asserted here.
    #[test]
    fn resolve_icon_path_returns_existing_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let icon = dir.path().join("quantum-test-icon.png");
        std::fs::write(&icon, b"\x89PNG\r\n\x1a\n").unwrap();

        let resolved = resolve_icon_path(Some(icon.to_str().unwrap()));
        assert_eq!(resolved.as_deref(), Some(icon.as_path()));
    }

    /// An absolute path that does not exist is not returned verbatim; it
    /// falls through to a (failing) name lookup and yields None.
    #[test]
    fn resolve_icon_path_rejects_missing_absolute_path() {
        let resolved = resolve_icon_path(Some("/nonexistent/quantum/icon-xyz123.png"));
        assert!(resolved.is_none());
    }

    /// `None` icon input resolves to `None`.
    #[test]
    fn resolve_icon_path_none_input_yields_none() {
        assert!(resolve_icon_path(None).is_none());
    }

    /// An app carrying an absolute, existing icon path produces
    /// `IconRef::Path` pointing at that file.
    #[tokio::test]
    async fn search_app_with_absolute_icon_path_yields_path() {
        let dir = tempfile::tempdir().unwrap();
        let icon = dir.path().join("quantum-test-icon.png");
        std::fs::write(&icon, b"\x89PNG\r\n\x1a\n").unwrap();

        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(vec![AppInfo::new(
                "quantum-test".to_string(),
                "Quantum Test".to_string(),
                None,
                vec![],
                "quantum-test".to_string(),
                Some(icon.to_str().unwrap().to_string()),
            )]),
            executor,
            usage: test_usage(),
            icon_cache: Mutex::new(HashMap::new()),
        };

        let query = Query::new("quantum");
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(matches.len(), 1);
        match &matches[0].icon {
            Some(quantum_domain::IconRef::Path(p)) => {
                assert_eq!(p, &icon, "resolved icon path mismatch");
            }
            other => panic!("expected IconRef::Path, got {other:?}"),
        }
    }

    /// An empty query returns a non-empty, bounded list of default apps when
    /// apps exist, rather than nothing or every app via the substring scorer.
    #[tokio::test]
    async fn empty_query_returns_default_apps() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(vec![
                AppInfo::new(
                    "firefox".to_string(),
                    "Firefox".to_string(),
                    None,
                    vec![],
                    "firefox".to_string(),
                    None,
                ),
                AppInfo::new(
                    "chromium".to_string(),
                    "Chromium".to_string(),
                    None,
                    vec![],
                    "chromium".to_string(),
                    None,
                ),
            ]),
            executor,
            usage: test_usage(),
            icon_cache: Mutex::new(HashMap::new()),
        };

        let query = Query::new("");
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(
            matches.len(),
            2,
            "empty query should return the default apps"
        );
    }

    /// An empty query respects the requested limit.
    #[tokio::test]
    async fn empty_query_respects_limit() {
        let executor = Arc::new(FakeExecutor::new());
        let apps: Vec<AppInfo> = (0..5)
            .map(|i| {
                AppInfo::new(
                    format!("app{i}"),
                    format!("App {i}"),
                    None,
                    vec![],
                    format!("app{i}"),
                    None,
                )
            })
            .collect();
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(apps),
            executor,
            usage: test_usage(),
            icon_cache: Mutex::new(HashMap::new()),
        };

        let query = Query {
            text: String::new(),
            providers: vec![],
            limit: Some(2),
        };
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(matches.len(), 2, "empty query should honor the limit");
    }

    /// Recording a launch moves that app to the front of the empty-query
    /// default results.
    #[tokio::test]
    async fn recorded_launch_moves_app_to_front_of_defaults() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            // Alphabetical order would put "alpha" first.
            apps: RwLock::new(vec![
                AppInfo::new(
                    "alpha".to_string(),
                    "Alpha".to_string(),
                    None,
                    vec![],
                    "alpha".to_string(),
                    None,
                ),
                AppInfo::new(
                    "zeta".to_string(),
                    "Zeta".to_string(),
                    None,
                    vec![],
                    "zeta".to_string(),
                    None,
                ),
            ]),
            executor: executor.clone(),
            usage: test_usage(),
            icon_cache: Mutex::new(HashMap::new()),
        };

        // Launch zeta so it gains usage history.
        provider
            .invoke(&Action::Launch {
                desktop_id: "zeta".to_string(),
            })
            .await
            .unwrap();

        let matches = provider.search(&Query::new("")).await.unwrap();
        assert_eq!(
            matches[0].id, "zeta",
            "the recently launched app should rank first in defaults"
        );
    }

    #[tokio::test]
    async fn search_no_results() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(vec![AppInfo::new(
                "firefox".to_string(),
                "Firefox".to_string(),
                Some("Web Browser".to_string()),
                vec![],
                "firefox".to_string(),
                None,
            )]),
            executor,
            usage: test_usage(),
            icon_cache: Mutex::new(HashMap::new()),
        };

        let query = Query::new("xyz123nonexistent");
        let matches = provider.search(&query).await.unwrap();

        assert!(matches.is_empty());
    }

    /// Regression test: when the same desktop id (e.g. firefox.desktop)
    /// exists in both XDG_DATA_HOME and an XDG_DATA_DIRS entry, scan_apps
    /// must keep only the first-seen one (XDG_DATA_HOME wins) and produce
    /// exactly one AppInfo for that id.
    #[tokio::test]
    async fn scan_apps_dedupes_across_xdg_dirs_first_seen_wins() {
        use tokio::sync::Mutex;
        // Env is process-global; serialize tests that mutate it.
        // Tokio's async Mutex is safe to hold across await points.
        static ENV_LOCK: Mutex<()> = Mutex::const_new(());
        let _guard = ENV_LOCK.lock().await;

        let home_dir = tempfile::tempdir().unwrap();
        let system_dir = tempfile::tempdir().unwrap();

        let home_apps = home_dir.path().join("applications");
        let system_apps = system_dir.path().join("applications");
        std::fs::create_dir(&home_apps).unwrap();
        std::fs::create_dir(&system_apps).unwrap();

        // Two installs of firefox.desktop with DIFFERENT Exec lines so we
        // can verify which directory's copy won.
        std::fs::write(
            home_apps.join("firefox.desktop"),
            r#"[Desktop Entry]
Name=Firefox
GenericName=Web Browser
Exec=firefox-from-home %u
Type=Application"#,
        )
        .unwrap();

        std::fs::write(
            system_apps.join("firefox.desktop"),
            r#"[Desktop Entry]
Name=Firefox
GenericName=Web Browser
Exec=firefox-from-system %u
Type=Application"#,
        )
        .unwrap();

        // Snapshot and override env for the duration of this test.
        let prev_data_home = std::env::var_os("XDG_DATA_HOME");
        let prev_data_dirs = std::env::var_os("XDG_DATA_DIRS");
        // SAFETY: serialized by ENV_LOCK above.
        std::env::set_var("XDG_DATA_HOME", home_dir.path());
        std::env::set_var("XDG_DATA_DIRS", system_dir.path());

        let executor = Arc::new(FakeExecutor::new());
        let result = DesktopAppsProvider::new(executor).await;

        // Restore env before asserting so a panic still cleans up.
        match prev_data_home {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match prev_data_dirs {
            Some(v) => std::env::set_var("XDG_DATA_DIRS", v),
            None => std::env::remove_var("XDG_DATA_DIRS"),
        }

        let provider = result.unwrap();

        let query = Query::new("firefox");
        let matches = provider.search(&query).await.unwrap();
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one firefox match after dedup, got {}",
            matches.len()
        );
        assert_eq!(matches[0].id, "firefox");

        // The winning copy must be the one from XDG_DATA_HOME.
        let apps = provider.apps.read().await;
        let firefox = apps
            .iter()
            .find(|a| a.id == "firefox")
            .expect("firefox AppInfo present");
        assert_eq!(
            firefox.exec, "firefox-from-home %u",
            "XDG_DATA_HOME entry should win over XDG_DATA_DIRS entry"
        );
    }

    #[tokio::test]
    async fn invoke_launch_action() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(vec![AppInfo::new(
                "firefox".to_string(),
                "Firefox".to_string(),
                Some("Web Browser".to_string()),
                vec![],
                "firefox %u".to_string(),
                None,
            )]),
            executor: executor.clone(),
            usage: test_usage(),
            icon_cache: Mutex::new(HashMap::new()),
        };

        let action = Action::Launch {
            desktop_id: "firefox".to_string(),
        };
        let result = provider.invoke(&action).await.unwrap();

        assert!(result.message.is_some());

        let spawned = executor.spawned.read().await;
        assert!(!spawned.is_empty());
        assert_eq!(spawned[0], vec!["firefox".to_string()]);
    }
}
