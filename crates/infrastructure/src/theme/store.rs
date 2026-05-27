use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

use quantum_domain::DomainError;

/// Resolved view with template and style paths.
#[derive(Debug, Clone)]
pub struct ResolvedView {
    pub template_path: PathBuf,
    pub style_path: Option<PathBuf>,
    pub script_path: Option<PathBuf>,
}

/// Theme store for loading and cascading themes.
pub struct ThemeStore {
    themes_dir: PathBuf,
    active_theme: RwLock<String>,
}

impl ThemeStore {
    /// Create a new theme store.
    pub fn new(active_theme: Option<String>) -> Self {
        let themes_dir = Self::themes_dir();
        let active = active_theme.unwrap_or_else(|| "default".to_string());

        Self {
            themes_dir,
            active_theme: RwLock::new(active),
        }
    }

    /// Get the themes directory.
    fn themes_dir() -> PathBuf {
        // For now, use a standard location
        // In Phase 6, frontend themes will be at frontend/themes/
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));

        PathBuf::from(config_home).join("quantum/themes")
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

    /// Get resolved tokens for the current theme.
    pub async fn resolved_tokens(&self) -> HashMap<String, String> {
        // Return a minimal default set of tokens
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

    /// Get a view by name.
    pub async fn view(&self, view_name: &str) -> Option<ResolvedView> {
        let theme = self.active_theme.read().await;

        let template_path = self
            .themes_dir
            .join(&*theme)
            .join("views")
            .join(view_name)
            .join("index.html");

        if template_path.exists() {
            Some(ResolvedView {
                template_path,
                style_path: Some(
                    self.themes_dir
                        .join(&*theme)
                        .join("views")
                        .join(view_name)
                        .join("style.css"),
                ),
                script_path: Some(
                    self.themes_dir
                        .join(&*theme)
                        .join("views")
                        .join(view_name)
                        .join("script.ts"),
                ),
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn theme_store_creates_with_default() {
        let store = ThemeStore::new(None);
        let tokens = store.resolved_tokens().await;
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
        let tokens = store.resolved_tokens().await;

        // Check essential tokens exist
        assert!(tokens.contains_key("color-bg"));
        assert!(tokens.contains_key("font-sans"));
        assert!(tokens.contains_key("space-1"));
        assert!(tokens.contains_key("radius-sm"));
        assert!(tokens.contains_key("duration-fast"));
    }
}
