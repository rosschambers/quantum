use async_trait::async_trait;
use freedesktop_desktop_entry::DesktopEntry;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
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
    ) -> Self {
        let name_lower = name.to_lowercase();
        let generic_name_lower = generic_name.as_ref().map(|s| s.to_lowercase());
        let keywords_lower = keywords.iter().map(|k| k.to_lowercase()).collect();
        Self {
            id,
            name,
            generic_name,
            exec,
            name_lower,
            generic_name_lower,
            keywords_lower,
        }
    }
}

/// Provider for desktop applications (*.desktop files).
pub struct DesktopAppsProvider {
    id: ProviderId,
    apps: RwLock<Vec<AppInfo>>,
    executor: Arc<dyn ShellExecutor>,
}

impl DesktopAppsProvider {
    /// Create a new DesktopAppsProvider by scanning application directories.
    pub async fn new(executor: Arc<dyn ShellExecutor>) -> Result<Self, DomainError> {
        let mut provider = Self {
            id: ProviderId::from("desktop-apps"),
            apps: RwLock::new(Vec::new()),
            executor,
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
    async fn scan_directory(
        &self,
        dir: &Path,
        by_id: &mut HashMap<String, AppInfo>,
    ) -> Result<(), DomainError> {
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            if let Ok(de) = DesktopEntry::decode(&path, &content) {
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

                                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                                    by_id.entry(name.to_string()).or_insert_with(|| {
                                        AppInfo::new(
                                            name.to_string(),
                                            app_name,
                                            generic_name,
                                            keywords,
                                            exec,
                                        )
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // Directory doesn't exist or can't be read, skip it
            }
        }
        Ok(())
    }

    /// Strip desktop entry field codes from exec string.
    fn clean_exec(exec: &str) -> String {
        exec.split_whitespace()
            .map(|part| {
                if part.starts_with('%') {
                    String::new()
                } else {
                    part.to_string()
                }
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[async_trait]
impl ProviderSource for DesktopAppsProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, q: &Query) -> Result<Vec<Match>, DomainError> {
        let apps = self.apps.read().await;
        let query_lower = q.text.to_lowercase();
        let mut matches = Vec::new();

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
                // Resolve the app's icon: try Icon= field first, fall back to
                // the desktop entry name (which is typically the icon name).
                let icon_name = match &app.id {
                    id if id.ends_with(".desktop") => id.trim_end_matches(".desktop"),
                    id => id,
                };
                matches.push(Match {
                    id: app.id.clone(),
                    provider: self.id.clone(),
                    title: app.name.clone(),
                    subtitle: app.generic_name.clone(),
                    icon: Some(quantum_domain::IconRef::Name(
                        icon_name.to_string(),
                    )),
                    score: MatchScore::new(combined_score),
                    action: Action::Launch {
                        desktop_id: app.id.clone(),
                    },
                });
            }
        }

        // Sort by score descending
        matches.sort_by(|a, b| {
            let a_val = a.score.value();
            let b_val = b.score.value();
            b_val
                .partial_cmp(&a_val)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit results
        if let Some(limit) = q.limit {
            matches.truncate(limit as usize);
        }

        Ok(matches)
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Launch { desktop_id } => {
                let apps = self.apps.read().await;
                if let Some(app) = apps.iter().find(|a| &a.id == desktop_id) {
                    let clean_exec = Self::clean_exec(&app.exec);
                    let command: Vec<String> = clean_exec
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect();

                    if !command.is_empty() {
                        self.executor.spawn_detached(&command).await?;
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
        let cleaned = DesktopAppsProvider::clean_exec(exec);
        assert_eq!(cleaned, "firefox");
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
            )]),
            executor,
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
                ),
                AppInfo::new(
                    "browser-chooser".to_string(),
                    "Browser Chooser".to_string(),
                    None,
                    vec![],
                    "chooser".to_string(),
                ),
            ]),
            executor,
        };

        let query = Query::new("browser");
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(
            matches[0].id, "browser-chooser",
            "name match should outrank keyword-only match"
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
            )]),
            executor,
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
            )]),
            executor: executor.clone(),
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
