use async_trait::async_trait;
use freedesktop_desktop_entry::DesktopEntry;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, MatchScore, ProviderCapabilities, ProviderId,
    ProviderSource, Query, ShellExecutor,
};

/// Information about a desktop application.
#[derive(Debug, Clone)]
struct AppInfo {
    id: String,
    name: String,
    generic_name: Option<String>,
    #[allow(dead_code)]
    keywords: Vec<String>,
    exec: String,
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
    async fn scan_apps(&mut self) -> Result<(), DomainError> {
        let mut apps = Vec::new();

        // Scan in common locations
        let data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
            format!("{}/.local/share", std::env::var("HOME").unwrap_or_default())
        });
        let data_dirs = std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());

        let mut dirs = vec![data_home];
        dirs.extend(data_dirs.split(':').map(|s| s.to_string()));

        for base_dir in dirs {
            let app_dir = Path::new(&base_dir).join("applications");
            if app_dir.exists() {
                self.scan_directory(&app_dir, &mut apps).await?;
            }
        }

        // Sort by name
        apps.sort_by(|a, b| a.name.cmp(&b.name));

        *self.apps.write().await = apps;
        Ok(())
    }

    /// Scan a single directory for .desktop files.
    async fn scan_directory(&self, dir: &Path, apps: &mut Vec<AppInfo>) -> Result<(), DomainError> {
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
                                    apps.push(AppInfo {
                                        id: name.to_string(),
                                        name: app_name,
                                        generic_name,
                                        keywords,
                                        exec,
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

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            searchable: true,
            streamable: false,
        }
    }

    async fn search(&self, q: &Query) -> Result<Vec<Match>, DomainError> {
        let apps = self.apps.read().await;
        let query_lower = q.text.to_lowercase();
        let mut matches = Vec::new();

        for app in apps.iter() {
            let name_lower = app.name.to_lowercase();
            let generic_lower = app.generic_name.as_ref().map(|s| s.to_lowercase());

            // Simple substring matching with scoring based on position
            let name_score = if name_lower.contains(&query_lower) {
                let pos = name_lower.find(&query_lower).unwrap_or(0);
                // Earlier match = higher score
                1.0 - (pos as f32 / name_lower.len() as f32 * 0.5)
            } else {
                0.0
            };

            let generic_score = if let Some(gn_lower) = &generic_lower {
                if gn_lower.contains(&query_lower) {
                    0.6 // Lower weight than name
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let combined_score = name_score.max(generic_score);

            if combined_score > 0.1 {
                matches.push(Match {
                    id: app.id.clone(),
                    provider: self.id.clone(),
                    title: app.name.clone(),
                    subtitle: app.generic_name.clone(),
                    icon: None, // Icons can be loaded separately
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
        async fn execute(&self, _command: &[String]) -> Result<String, DomainError> {
            Ok(String::new())
        }

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
            .scan_directory(&apps_dir, &mut Vec::new())
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
        let mut apps = Vec::new();
        provider.scan_directory(&apps_dir, &mut apps).await.unwrap();
        *provider.apps.write().await = apps;

        // Search for Firefox
        let query = Query::new("fox");
        let matches = provider.search(&query).await.unwrap();

        assert!(!matches.is_empty());
        assert_eq!(matches[0].title, "Firefox");
    }

    #[tokio::test]
    async fn search_no_results() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(vec![AppInfo {
                id: "firefox".to_string(),
                name: "Firefox".to_string(),
                generic_name: Some("Web Browser".to_string()),
                keywords: vec![],
                exec: "firefox".to_string(),
            }]),
            executor,
        };

        let query = Query::new("xyz123nonexistent");
        let matches = provider.search(&query).await.unwrap();

        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn invoke_launch_action() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = DesktopAppsProvider {
            id: ProviderId::from("test"),
            apps: RwLock::new(vec![AppInfo {
                id: "firefox".to_string(),
                name: "Firefox".to_string(),
                generic_name: Some("Web Browser".to_string()),
                keywords: vec![],
                exec: "firefox %u".to_string(),
            }]),
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
