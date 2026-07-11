use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use quantum_domain::{
    Action, ActionOutcome, DomainError, HyprlandClient, Match, MatchScore, ProviderId,
    ProviderSource, Query,
};

/// Minimum interval between hyprctl j/clients refreshes. Within this window,
/// successive search calls (one per keystroke during a typing burst) share
/// the same cached snapshot instead of each shelling out.
const REFRESH_TTL: Duration = Duration::from_millis(100);

/// Information about a Hyprland window. Lowercase fields are precomputed
/// once at refresh time so the per-keystroke match loop does not allocate
/// fresh lowercase strings for every window.
#[derive(Debug, Clone)]
struct WindowInfo {
    address: String,
    title: String,
    class: String,
    workspace_id: i64,
    workspace_name: String,
    title_lower: String,
    class_lower: String,
    workspace_name_lower: String,
    /// The window's application icon, resolved from its class at refresh time.
    /// `None` when no icon could be resolved, so the launcher renders no icon
    /// rather than a broken reference the webview cannot load.
    icon: Option<quantum_domain::IconRef>,
}

/// Resolve a freedesktop icon reference to a concrete file path.
///
/// If `icon` is already an absolute path that exists on disk, it is returned
/// as-is. Otherwise it is treated as a freedesktop icon name and looked up
/// against the installed icon themes (preferring a 48px raster size). Returns
/// `None` when nothing resolves. This mirrors the sibling desktop-apps
/// provider's helper; the two are kept independent so neither provider depends
/// on the other.
fn resolve_icon_path(icon: Option<&str>) -> Option<PathBuf> {
    let name = icon?;
    let as_path = std::path::Path::new(name);
    if as_path.is_absolute() && as_path.exists() {
        return Some(as_path.to_path_buf());
    }
    freedesktop_icons::lookup(name).with_size(48).find()
}

/// Resolve a Hyprland window `class` to an application [`IconRef`], using an
/// injected `resolve` function so the resolution order is unit-testable without
/// touching the real icon theme. The order is:
///
/// 1. If the lowercased class matches a `StartupWMClass` index entry, resolve
///    that entry's `Icon=` name (Visual Studio Code's class `Code` maps to icon
///    `vscode`).
/// 2. Otherwise try the lowercased class directly as an icon name (most
///    classes, for example `firefox`, are also their icon name).
/// 3. Otherwise try the original-case class as an icon name.
/// 4. Otherwise `None`.
fn resolve_class_icon_with(
    index: &HashMap<String, String>,
    class: &str,
    resolve: impl Fn(&str) -> Option<PathBuf>,
) -> Option<quantum_domain::IconRef> {
    let class_lower = class.to_lowercase();

    if let Some(name) = index.get(&class_lower) {
        if let Some(path) = resolve(name) {
            return Some(quantum_domain::IconRef::Path(path));
        }
    }

    if let Some(path) = resolve(&class_lower) {
        return Some(quantum_domain::IconRef::Path(path));
    }

    if let Some(path) = resolve(class) {
        return Some(quantum_domain::IconRef::Path(path));
    }

    None
}

/// Resolve a window `class` to an application [`IconRef`] against the real
/// installed icon themes, using [`resolve_class_icon_with`] with the concrete
/// [`resolve_icon_path`] resolver.
fn resolve_class_icon(
    index: &HashMap<String, String>,
    class: &str,
) -> Option<quantum_domain::IconRef> {
    resolve_class_icon_with(index, class, |name| resolve_icon_path(Some(name)))
}

/// Build a map from lowercased window class to icon name by scanning installed
/// desktop entries. Two keys are inserted per entry: its `StartupWMClass` (the
/// authoritative window class, for classes that differ from the icon name) and
/// its desktop-file app id / stem (a secondary fallback). Both map to the
/// entry's `Icon=` value. A `StartupWMClass` key takes precedence over an app
/// id key when both would collide. Directories that cannot be read and files
/// that cannot be parsed are skipped; a total scan failure yields an empty map.
fn build_wm_class_icon_index() -> HashMap<String, String> {
    use freedesktop_desktop_entry::DesktopEntry;

    let data_home = std::env::var("XDG_DATA_HOME")
        .unwrap_or_else(|_| format!("{}/.local/share", std::env::var("HOME").unwrap_or_default()));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());

    let mut base_dirs = vec![data_home];
    base_dirs.extend(data_dirs.split(':').map(|s| s.to_string()));

    let mut index: HashMap<String, String> = HashMap::new();

    for base_dir in base_dirs {
        let app_dir = std::path::Path::new(&base_dir).join("applications");
        let entries = match std::fs::read_dir(&app_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(desktop_entry) = DesktopEntry::decode(&path, &content) else {
                continue;
            };
            let Some(icon) = desktop_entry.icon() else {
                continue;
            };
            let icon = icon.to_string();

            // Secondary key: the app id / file stem. Inserted first so a
            // StartupWMClass key can override it on collision.
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                index
                    .entry(stem.to_lowercase())
                    .or_insert_with(|| icon.clone());
            }

            // Authoritative key: StartupWMClass. Always wins.
            if let Some(wm_class) = desktop_entry.startup_wm_class() {
                index.insert(wm_class.to_lowercase(), icon.clone());
            }
        }
    }

    index
}

/// Parse a Hyprland `j/clients` JSON response into window records, resolving
/// each window's icon through the injected `resolve_icon` function. Keeping the
/// resolver injectable makes the icon-population path unit-testable without the
/// installed icon theme. A malformed response yields an empty list.
fn parse_windows_with(
    response: &str,
    resolve_icon: impl Fn(&str) -> Option<quantum_domain::IconRef>,
) -> Vec<WindowInfo> {
    let mut windows = Vec::new();

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(response) {
        if let Some(clients) = json.as_array() {
            for client in clients {
                if let (Some(address), Some(title), Some(class), Some(workspace_id)) = (
                    client["address"].as_str(),
                    client["title"].as_str(),
                    client["class"].as_str(),
                    client["workspace"]["id"].as_i64(),
                ) {
                    let workspace_name = client["workspace"]["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let title_lower = title.to_lowercase();
                    let class_lower = class.to_lowercase();
                    let workspace_name_lower = workspace_name.to_lowercase();
                    let icon = resolve_icon(class);
                    windows.push(WindowInfo {
                        address: address.to_string(),
                        title: title.to_string(),
                        class: class.to_string(),
                        workspace_id,
                        workspace_name,
                        title_lower,
                        class_lower,
                        workspace_name_lower,
                        icon,
                    });
                }
            }
        }
    }

    windows
}

/// Format a friendly subtitle from a workspace name, id, and window class.
fn format_subtitle(workspace_id: i64, workspace_name: &str, class: &str) -> String {
    if let Some(special) = workspace_name.strip_prefix("special:") {
        let label = if special.is_empty() {
            "scratchpad"
        } else {
            special
        };
        return format!("Special: {} \u{00B7} {}", label, class);
    }

    if !workspace_name.is_empty() {
        return format!("{} \u{00B7} {}", workspace_name, class);
    }

    format!("Workspace {} \u{00B7} {}", workspace_id, class)
}

/// Provider for Hyprland windows.
pub struct HyprlandWindowsProvider {
    id: ProviderId,
    client: Arc<dyn HyprlandClient>,
    windows: RwLock<Vec<WindowInfo>>,
    last_refresh: Mutex<Option<Instant>>,
    /// Map from lowercased window class to icon name, built once at
    /// construction by scanning installed desktop entries. Lets classes whose
    /// name differs from their icon (for example `Code` -> `vscode`) resolve
    /// correctly. Empty when the desktop-entry scan finds nothing.
    wm_class_icon_index: HashMap<String, String>,
}

impl HyprlandWindowsProvider {
    /// Create a new HyprlandWindowsProvider.
    pub async fn new(client: Arc<dyn HyprlandClient>) -> Result<Self, DomainError> {
        let provider = Self {
            id: ProviderId::from("hyprland-windows"),
            client,
            windows: RwLock::new(Vec::new()),
            last_refresh: Mutex::new(None),
            wm_class_icon_index: build_wm_class_icon_index(),
        };

        // Try to load initial windows
        let _ = provider.refresh_windows().await;

        Ok(provider)
    }

    /// Refresh the window cache if the previous refresh is older than
    /// `REFRESH_TTL`. Returns immediately when the cache is still fresh.
    async fn refresh_if_stale(&self) -> Result<(), DomainError> {
        if let Ok(guard) = self.last_refresh.lock() {
            if let Some(last) = *guard {
                if Instant::now().duration_since(last) < REFRESH_TTL {
                    return Ok(());
                }
            }
        }
        self.refresh_windows().await
    }

    /// Parse a Hyprland `j/clients` JSON response into window records, resolving
    /// each window's icon from its class against `index`. Shared by the cached
    /// search path and the one-shot `snapshot` so the parsing lives in one
    /// place. A malformed response yields an empty list.
    fn parse_windows(response: &str, index: &HashMap<String, String>) -> Vec<WindowInfo> {
        parse_windows_with(response, |class| resolve_class_icon(index, class))
    }

    /// Refresh the list of windows from Hyprland.
    async fn refresh_windows(&self) -> Result<(), DomainError> {
        // Request clients from Hyprland
        let response = self.client.command("j/clients").await?;

        *self.windows.write().await = Self::parse_windows(&response, &self.wm_class_icon_index);

        if let Ok(mut guard) = self.last_refresh.lock() {
            *guard = Some(Instant::now());
        }

        Ok(())
    }
}

#[async_trait]
impl ProviderSource for HyprlandWindowsProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, q: &Query) -> Result<Vec<Match>, DomainError> {
        self.refresh_if_stale().await?;

        let windows = self.windows.read().await;
        let query_lower = q.text.to_lowercase();
        let mut matches = Vec::new();

        for window in windows.iter() {
            let workspace_id_str = window.workspace_id.to_string();

            // Check if any field contains the query
            let score = if window.title_lower.contains(&query_lower) {
                1.0
            } else if window.class_lower.contains(&query_lower) {
                0.7
            } else if window.workspace_name_lower.contains(&query_lower)
                || workspace_id_str.contains(&query_lower)
            {
                0.5
            } else {
                0.0
            };

            if score > 0.0 {
                matches.push(Match {
                    id: window.address.clone(),
                    provider: self.id.clone(),
                    title: window.title.clone(),
                    subtitle: Some(format_subtitle(
                        window.workspace_id,
                        &window.workspace_name,
                        &window.class,
                    )),
                    icon: window.icon.clone(),
                    score: MatchScore::new(score),
                    action: Action::Custom {
                        kind: "hyprland_focus".to_string(),
                        payload: json!({ "address": window.address }),
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
            Action::Custom { kind, payload } if kind == "hyprland_focus" => {
                if let Some(address) = payload.get("address").and_then(|a| a.as_str()) {
                    let cmd = format!("dispatch focuswindow address:{}", address);
                    self.client.command(&cmd).await?;
                    return Ok(ActionOutcome {
                        message: Some(format!("Focused window {}", address)),
                    });
                }
                Err(DomainError::InvalidQuery(
                    "missing address in payload".to_string(),
                ))
            }
            _ => Err(DomainError::Unsupported(
                "only hyprland_focus action supported".to_string(),
            )),
        }
    }

    async fn snapshot(&self) -> Option<serde_json::Value> {
        // Query the live window list fresh so a freshly opened kill menu
        // reflects the current windows. On any Hyprland error degrade to an
        // empty list so the menu falls back to its static items.
        let windows = match self.client.command("j/clients").await {
            Ok(response) => Self::parse_windows(&response, &self.wm_class_icon_index),
            Err(_) => return Some(json!({ "windows": [] })),
        };

        let entries: Vec<serde_json::Value> = windows
            .iter()
            .map(|window| {
                json!({
                    "address": window.address,
                    "class": window.class,
                    "title": window.title,
                    "workspace_id": window.workspace_id,
                    "workspace_name": window.workspace_name,
                })
            })
            .collect();

        Some(json!({ "windows": entries }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockHyprlandClient {
        response: String,
    }

    impl MockHyprlandClient {
        fn new(response: String) -> Self {
            Self { response }
        }
    }

    #[async_trait]
    impl HyprlandClient for MockHyprlandClient {
        async fn command(&self, _cmd: &str) -> Result<String, DomainError> {
            Ok(self.response.clone())
        }
    }

    /// Mock that counts how many times `j/clients` is requested so tests
    /// can assert the TTL gate prevents redundant shell-outs.
    struct CountingHyprlandClient {
        response: String,
        clients_calls: AtomicUsize,
    }

    impl CountingHyprlandClient {
        fn new(response: String) -> Self {
            Self {
                response,
                clients_calls: AtomicUsize::new(0),
            }
        }

        fn clients_calls(&self) -> usize {
            self.clients_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl HyprlandClient for CountingHyprlandClient {
        async fn command(&self, cmd: &str) -> Result<String, DomainError> {
            if cmd == "j/clients" {
                self.clients_calls.fetch_add(1, Ordering::SeqCst);
            }
            Ok(self.response.clone())
        }
    }

    /// Mock that always fails, used to assert `snapshot` degrades to an empty
    /// window list instead of panicking when Hyprland is unavailable.
    struct ErroringHyprlandClient;

    #[async_trait]
    impl HyprlandClient for ErroringHyprlandClient {
        async fn command(&self, _cmd: &str) -> Result<String, DomainError> {
            Err(DomainError::Unsupported("hyprland unavailable".to_string()))
        }
    }

    #[tokio::test]
    async fn search_windows_by_title() {
        let response = r#"[
            {
                "address": "0x123",
                "title": "Firefox",
                "class": "firefox",
                "workspace": {"id": 1, "name": "1"}
            },
            {
                "address": "0x456",
                "title": "VSCode",
                "class": "code",
                "workspace": {"id": 1, "name": "1"}
            }
        ]"#
        .to_string();

        let client = Arc::new(MockHyprlandClient::new(response));
        let provider = HyprlandWindowsProvider::new(client).await.unwrap();

        let query = Query::new("Firefox");
        let matches = provider.search(&query).await.unwrap();

        assert!(!matches.is_empty());
        assert_eq!(matches[0].title, "Firefox");
    }

    #[tokio::test]
    async fn search_no_match_returns_empty() {
        let response = r#"[
            {
                "address": "0x123",
                "title": "Firefox",
                "class": "firefox",
                "workspace": {"id": 1, "name": "1"}
            }
        ]"#
        .to_string();

        let client = Arc::new(MockHyprlandClient::new(response));
        let provider = HyprlandWindowsProvider::new(client).await.unwrap();

        let query = Query::new("nonexistent");
        let matches = provider.search(&query).await.unwrap();

        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn invoke_focus_action() {
        let response = r#"[]"#.to_string();
        let client = Arc::new(MockHyprlandClient::new(response));
        let provider = HyprlandWindowsProvider::new(client).await.unwrap();

        let action = Action::Custom {
            kind: "hyprland_focus".to_string(),
            payload: json!({ "address": "0x123" }),
        };
        let result = provider.invoke(&action).await.unwrap();

        assert!(result.message.is_some());
    }

    #[tokio::test]
    async fn invoke_wrong_action_fails() {
        let response = r#"[]"#.to_string();
        let client = Arc::new(MockHyprlandClient::new(response));
        let provider = HyprlandWindowsProvider::new(client).await.unwrap();

        let action = Action::Launch {
            desktop_id: "test".to_string(),
        };
        let result = provider.invoke(&action).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn successive_searches_within_ttl_share_one_refresh() {
        let response = r#"[
            {
                "address": "0x123",
                "title": "Firefox",
                "class": "firefox",
                "workspace": {"id": 1, "name": "1"}
            }
        ]"#
        .to_string();

        let client = Arc::new(CountingHyprlandClient::new(response));
        // `new` does one initial refresh.
        let provider = HyprlandWindowsProvider::new(client.clone()).await.unwrap();
        assert_eq!(client.clients_calls(), 1);

        // Two searches in immediate succession must not trigger any extra
        // j/clients calls because they fall inside REFRESH_TTL.
        let query = Query::new("fire");
        let _ = provider.search(&query).await.unwrap();
        let _ = provider.search(&query).await.unwrap();

        assert_eq!(
            client.clients_calls(),
            1,
            "expected the TTL gate to skip the refresh on back-to-back searches"
        );
    }

    #[tokio::test]
    async fn snapshot_returns_window_list() {
        let response = r#"[
            {
                "address": "0x123",
                "title": "Firefox",
                "class": "firefox",
                "workspace": {"id": 1, "name": "1"}
            },
            {
                "address": "0x456",
                "title": "VSCode",
                "class": "code",
                "workspace": {"id": 2, "name": "code"}
            }
        ]"#
        .to_string();

        let client = Arc::new(MockHyprlandClient::new(response));
        let provider = HyprlandWindowsProvider::new(client).await.unwrap();

        let snapshot = provider.snapshot().await.unwrap();
        let windows = snapshot["windows"]
            .as_array()
            .expect("windows should be an array");

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0]["address"], "0x123");
        assert_eq!(windows[0]["class"], "firefox");
        assert_eq!(windows[0]["title"], "Firefox");
        assert_eq!(windows[0]["workspace_id"], 1);
        assert_eq!(windows[0]["workspace_name"], "1");
        assert_eq!(windows[1]["address"], "0x456");
        assert_eq!(windows[1]["class"], "code");
        assert_eq!(windows[1]["title"], "VSCode");
    }

    #[tokio::test]
    async fn snapshot_on_query_error_returns_empty_windows() {
        let client = Arc::new(ErroringHyprlandClient);
        let provider = HyprlandWindowsProvider::new(client).await.unwrap();

        let snapshot = provider.snapshot().await.unwrap();
        let windows = snapshot["windows"]
            .as_array()
            .expect("windows should be an array");

        assert!(windows.is_empty());
    }

    #[test]
    fn subtitle_uses_name_for_normal_workspace() {
        assert_eq!(format_subtitle(1, "1", "firefox"), "1 \u{00B7} firefox");
    }

    #[test]
    fn subtitle_friendly_for_special_workspace() {
        assert_eq!(
            format_subtitle(-98, "special:scratchpad", "alacritty"),
            "Special: scratchpad \u{00B7} alacritty"
        );
    }

    #[test]
    fn subtitle_fallback_when_name_missing() {
        assert_eq!(format_subtitle(7, "", "code"), "Workspace 7 \u{00B7} code");
    }

    /// When the class matches a `StartupWMClass` index entry, the mapped icon
    /// name (not the raw class) is what gets resolved. Visual Studio Code's
    /// class is `Code` but its icon is `vscode`; the index must redirect the
    /// lookup to `vscode`, and the resolver must never be asked for `code`.
    #[test]
    fn resolve_class_icon_uses_mapped_name_on_index_hit() {
        let mut index = HashMap::new();
        index.insert("code".to_string(), "vscode".to_string());
        let asked = std::cell::RefCell::new(Vec::new());

        let result = resolve_class_icon_with(&index, "Code", |name| {
            asked.borrow_mut().push(name.to_string());
            if name == "vscode" {
                Some(PathBuf::from("/x/vscode.png"))
            } else {
                None
            }
        });

        assert_eq!(
            result,
            Some(quantum_domain::IconRef::Path(PathBuf::from(
                "/x/vscode.png"
            )))
        );
        let asked = asked.borrow();
        assert!(
            asked.iter().any(|name| name == "vscode"),
            "resolver should be asked for the mapped icon name, asked: {asked:?}"
        );
        assert!(
            !asked.iter().any(|name| name == "code"),
            "resolver must not be asked for the raw class once the index hit resolves"
        );
    }

    /// With no index entry, the lowercased class is tried directly as an icon
    /// name. Most classes (firefox, steam, alacritty) are also their icon name.
    #[test]
    fn resolve_class_icon_falls_back_to_class_as_name() {
        let index = HashMap::new();

        let result = resolve_class_icon_with(&index, "Firefox", |name| {
            if name == "firefox" {
                Some(PathBuf::from("/x/firefox.png"))
            } else {
                None
            }
        });

        assert_eq!(
            result,
            Some(quantum_domain::IconRef::Path(PathBuf::from(
                "/x/firefox.png"
            )))
        );
    }

    /// When nothing resolves, `None` is emitted rather than a bogus
    /// `IconRef::Name` the webview cannot load.
    #[test]
    fn resolve_class_icon_returns_none_when_nothing_resolves() {
        let index = HashMap::new();
        let result = resolve_class_icon_with(&index, "NoSuchApp", |_name| None);
        assert!(result.is_none());
    }

    /// `parse_windows` populates each window's icon via the injected class
    /// resolver, so the search path can emit a concrete icon per window.
    #[test]
    fn parse_windows_sets_icon_from_resolver() {
        let response = r#"[
            {
                "address": "0x1",
                "title": "VSCode",
                "class": "Code",
                "workspace": {"id": 1, "name": "1"}
            }
        ]"#;

        let windows = parse_windows_with(response, |class| {
            if class == "Code" {
                Some(quantum_domain::IconRef::Path(PathBuf::from(
                    "/x/vscode.png",
                )))
            } else {
                None
            }
        });

        assert_eq!(windows.len(), 1);
        assert_eq!(
            windows[0].icon,
            Some(quantum_domain::IconRef::Path(PathBuf::from(
                "/x/vscode.png"
            )))
        );
    }

    /// A window whose class resolves to no icon carries `icon: None`.
    #[test]
    fn parse_windows_leaves_icon_none_when_unresolved() {
        let response = r#"[
            {
                "address": "0x1",
                "title": "Mystery",
                "class": "UnknownClass",
                "workspace": {"id": 1, "name": "1"}
            }
        ]"#;

        let windows = parse_windows_with(response, |_class| None);

        assert_eq!(windows.len(), 1);
        assert!(windows[0].icon.is_none());
    }
}
