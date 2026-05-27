use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

use quantum_domain::{
    Action, ActionOutcome, DomainError, HyprlandClient, Match, MatchScore, ProviderCapabilities,
    ProviderId, ProviderSource, Query,
};

/// Information about a Hyprland window.
#[derive(Debug, Clone)]
struct WindowInfo {
    address: String,
    title: String,
    class: String,
    workspace: String,
}

/// Provider for Hyprland windows.
pub struct HyprlandWindowsProvider {
    id: ProviderId,
    client: Arc<dyn HyprlandClient>,
    windows: RwLock<Vec<WindowInfo>>,
}

impl HyprlandWindowsProvider {
    /// Create a new HyprlandWindowsProvider.
    pub async fn new(client: Arc<dyn HyprlandClient>) -> Result<Self, DomainError> {
        let provider = Self {
            id: ProviderId::from("hyprland-windows"),
            client,
            windows: RwLock::new(Vec::new()),
        };

        // Try to load initial windows
        let _ = provider.refresh_windows().await;

        Ok(provider)
    }

    /// Refresh the list of windows from Hyprland.
    async fn refresh_windows(&self) -> Result<(), DomainError> {
        // Request clients from Hyprland
        let response = self.client.command("j/clients").await?;

        // Parse the JSON response
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response) {
            let mut windows = Vec::new();

            if let Some(clients) = json.as_array() {
                for client in clients {
                    if let (Some(address), Some(title), Some(class), Some(workspace)) = (
                        client["address"].as_str(),
                        client["title"].as_str(),
                        client["class"].as_str(),
                        client["workspace"]["id"].as_i64(),
                    ) {
                        windows.push(WindowInfo {
                            address: address.to_string(),
                            title: title.to_string(),
                            class: class.to_string(),
                            workspace: workspace.to_string(),
                        });
                    }
                }
            }

            *self.windows.write().await = windows;
        }

        Ok(())
    }
}

#[async_trait]
impl ProviderSource for HyprlandWindowsProvider {
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
        self.refresh_windows().await?;

        let windows = self.windows.read().await;
        let query_lower = q.text.to_lowercase();
        let mut matches = Vec::new();

        for window in windows.iter() {
            let title_lower = window.title.to_lowercase();
            let class_lower = window.class.to_lowercase();
            let workspace_lower = window.workspace.to_lowercase();

            // Check if any field contains the query
            let score = if title_lower.contains(&query_lower) {
                1.0
            } else if class_lower.contains(&query_lower) {
                0.7
            } else if workspace_lower.contains(&query_lower) {
                0.5
            } else {
                0.0
            };

            if score > 0.0 {
                matches.push(Match {
                    id: window.address.clone(),
                    provider: self.id.clone(),
                    title: window.title.clone(),
                    subtitle: Some(format!("Workspace {} - {}", window.workspace, window.class)),
                    icon: None,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn search_windows_by_title() {
        let response = r#"[
            {
                "address": "0x123",
                "title": "Firefox",
                "class": "firefox",
                "workspace": {"id": 1}
            },
            {
                "address": "0x456",
                "title": "VSCode",
                "class": "code",
                "workspace": {"id": 1}
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
                "workspace": {"id": 1}
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
}
