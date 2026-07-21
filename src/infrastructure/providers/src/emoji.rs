use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

use quantum_domain::{
    Action, ActionOutcome, ClipboardWriter, DomainError, Match, MatchScore, MenuAction, ProviderId,
    ProviderSource, Query,
};

/// The most matches returned from a single search, bounding both the bare-colon
/// default list and any filtered result set.
const MAXIMUM_RESULTS: usize = 30;

/// A single emoji record loaded from the bundled dataset: the glyph to copy,
/// its canonical name, and search keywords.
#[derive(Debug, Clone, Deserialize)]
struct EmojiEntry {
    glyph: String,
    name: String,
    #[serde(default)]
    keywords: Vec<String>,
}

/// Provider for an emoji picker. A query is treated as an emoji lookup when it
/// starts with a colon; the text after the colon filters the curated dataset by
/// name and keyword, and each match copies the chosen glyph.
pub struct EmojiProvider {
    id: ProviderId,
    clipboard: Arc<dyn ClipboardWriter>,
    entries: Vec<EmojiEntry>,
}

/// The curated emoji dataset, embedded at compile time. It is data, not code.
const EMOJI_DATA: &str = include_str!("emoji_data.json");

impl EmojiProvider {
    /// Create a new EmojiProvider that copies chosen glyphs with `clipboard`.
    ///
    /// The bundled dataset is parsed once here. A parse failure is a programmer
    /// error in the data file rather than a runtime condition, so it is logged
    /// and the provider falls back to an empty dataset instead of crashing the
    /// daemon.
    pub fn new(clipboard: Arc<dyn ClipboardWriter>) -> Self {
        let entries = match serde_json::from_str::<Vec<EmojiEntry>>(EMOJI_DATA) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::error!(
                    error = %error,
                    "failed to parse bundled emoji dataset; emoji provider disabled"
                );
                Vec::new()
            }
        };
        Self {
            id: ProviderId::from("emoji"),
            clipboard,
            entries,
        }
    }

    /// True when `entry` matches the lowercased `needle` in either its name or
    /// any of its keywords, by substring.
    fn matches(entry: &EmojiEntry, needle: &str) -> bool {
        if entry.name.to_lowercase().contains(needle) {
            return true;
        }
        entry
            .keywords
            .iter()
            .any(|keyword| keyword.to_lowercase().contains(needle))
    }

    /// Build the [`Match`] offered for `entry`: the glyph is the title and the
    /// default copy target, with menu actions to copy either the glyph or the
    /// name.
    fn build_match(&self, entry: &EmojiEntry) -> Match {
        Match {
            id: entry.glyph.clone(),
            provider: self.id.clone(),
            title: entry.glyph.clone(),
            subtitle: Some(entry.name.clone()),
            icon: None,
            score: MatchScore::new(1.0),
            action: Action::Copy {
                text: entry.glyph.clone(),
            },
            actions: vec![
                MenuAction {
                    label: "Copy glyph".to_string(),
                    icon: None,
                    danger: false,
                    action: Action::Copy {
                        text: entry.glyph.clone(),
                    },
                },
                MenuAction {
                    label: "Copy name".to_string(),
                    icon: None,
                    danger: false,
                    action: Action::Copy {
                        text: entry.name.clone(),
                    },
                },
            ],
        }
    }
}

#[async_trait]
impl ProviderSource for EmojiProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, q: &Query) -> Result<Vec<Match>, DomainError> {
        let needle = match q.text.strip_prefix(':') {
            Some(rest) => rest.trim().to_lowercase(),
            None => return Ok(vec![]),
        };

        let selected = self
            .entries
            .iter()
            .filter(|entry| needle.is_empty() || Self::matches(entry, &needle))
            .take(MAXIMUM_RESULTS)
            .map(|entry| self.build_match(entry))
            .collect();
        Ok(selected)
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Copy { text } => {
                self.clipboard.write_text(text).await?;
                Ok(ActionOutcome {
                    message: Some("Copied".to_string()),
                })
            }
            _ => Err(DomainError::Unsupported(
                "only Copy action supported".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeClipboard {
        writes: Arc<tokio::sync::RwLock<Vec<String>>>,
    }

    impl FakeClipboard {
        fn new() -> Self {
            Self {
                writes: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl ClipboardWriter for FakeClipboard {
        async fn write_text(&self, text: &str) -> Result<(), DomainError> {
            self.writes.write().await.push(text.to_string());
            Ok(())
        }

        async fn write_bytes(&self, _mime: &str, _bytes: &[u8]) -> Result<(), DomainError> {
            Ok(())
        }
    }

    fn provider() -> EmojiProvider {
        EmojiProvider::new(Arc::new(FakeClipboard::new()))
    }

    const COFFEE_GLYPH: &str = "\u{2615}";

    #[tokio::test]
    async fn coffee_query_returns_the_coffee_glyph() {
        let matches = provider().search(&Query::new(":coffee")).await.unwrap();
        assert!(!matches.is_empty());
        let first = &matches[0];
        assert_eq!(first.title, COFFEE_GLYPH);
        match &first.action {
            Action::Copy { text } => assert_eq!(text, COFFEE_GLYPH),
            other => panic!("expected a Copy action, found {other:?}"),
        }
    }

    #[tokio::test]
    async fn bare_colon_returns_a_bounded_non_empty_list() {
        let matches = provider().search(&Query::new(":")).await.unwrap();
        assert!(!matches.is_empty());
        assert!(matches.len() <= MAXIMUM_RESULTS);
    }

    #[tokio::test]
    async fn non_colon_query_returns_no_matches() {
        let matches = provider().search(&Query::new("firefox")).await.unwrap();
        assert_eq!(matches.len(), 0);
    }

    #[tokio::test]
    async fn keyword_only_query_finds_coffee() {
        let matches = provider().search(&Query::new(":beverage")).await.unwrap();
        assert!(matches.iter().any(|entry| entry.title == COFFEE_GLYPH));
    }

    #[tokio::test]
    async fn invoke_copy_writes_glyph_to_clipboard() {
        let clipboard = Arc::new(FakeClipboard::new());
        let provider = EmojiProvider::new(clipboard.clone());
        let action = Action::Copy {
            text: COFFEE_GLYPH.to_string(),
        };

        let outcome = provider.invoke(&action).await.unwrap();

        assert!(outcome.message.is_some());
        let writes = clipboard.writes.read().await;
        assert_eq!(writes.as_slice(), [COFFEE_GLYPH.to_string()]);
    }

    #[tokio::test]
    async fn invoke_non_copy_action_fails() {
        let action = Action::Launch {
            desktop_id: "x".to_string(),
        };
        assert!(provider().invoke(&action).await.is_err());
    }
}
