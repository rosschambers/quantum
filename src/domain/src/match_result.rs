use crate::{Action, MatchScore, ProviderId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum IconRef {
    Name(String),
    Path(PathBuf),
    DataUri(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuAction {
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub danger: bool,
    pub action: Action,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Match {
    pub id: String,
    pub provider: ProviderId,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub icon: Option<IconRef>,
    pub score: MatchScore,
    pub action: Action,
    #[serde(default)]
    pub actions: Vec<MenuAction>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip_icon_name() {
        let icon = IconRef::Name("firefox".to_string());
        let s = serde_json::to_string(&icon).unwrap();
        let back: IconRef = serde_json::from_str(&s).unwrap();
        match back {
            IconRef::Name(n) => assert_eq!(n, "firefox"),
            _ => panic!("Expected Name variant"),
        }
    }

    #[test]
    fn serde_roundtrip_icon_path() {
        let icon = IconRef::Path(PathBuf::from("/usr/share/icons/firefox.png"));
        let s = serde_json::to_string(&icon).unwrap();
        let back: IconRef = serde_json::from_str(&s).unwrap();
        match back {
            IconRef::Path(p) => assert_eq!(p, PathBuf::from("/usr/share/icons/firefox.png")),
            _ => panic!("Expected Path variant"),
        }
    }

    #[test]
    fn serde_roundtrip_icon_data_uri() {
        let icon = IconRef::DataUri("data:image/png;base64,iVBORw0KGgoAAAANS".to_string());
        let s = serde_json::to_string(&icon).unwrap();
        let back: IconRef = serde_json::from_str(&s).unwrap();
        match back {
            IconRef::DataUri(d) => assert!(d.starts_with("data:image")),
            _ => panic!("Expected DataUri variant"),
        }
    }

    #[test]
    fn serde_roundtrip_match() {
        let m = Match {
            id: "firefox-1".to_string(),
            provider: ProviderId::from("apps"),
            title: "Firefox".to_string(),
            subtitle: Some("Web Browser".to_string()),
            icon: Some(IconRef::Name("firefox".to_string())),
            score: MatchScore::new(0.95),
            action: Action::Launch {
                desktop_id: "firefox.desktop".to_string(),
            },
            actions: vec![],
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: Match = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, "firefox-1");
        assert_eq!(back.title, "Firefox");
        assert_eq!(back.score, MatchScore::new(0.95));
    }

    #[test]
    fn match_without_actions_deserializes_empty() {
        let json = r#"{"id":"a","provider":"apps","title":"X","score":1.0,"action":{"kind":"launch","data":{"desktop_id":"x.desktop"}}}"#;
        let m: Match = serde_json::from_str(json).unwrap();
        assert!(m.actions.is_empty());
    }

    #[test]
    fn menu_action_roundtrip() {
        let ma = MenuAction {
            label: "Copy result".to_string(),
            icon: None,
            danger: false,
            action: Action::Copy {
                text: "16".to_string(),
            },
        };
        let s = serde_json::to_string(&ma).unwrap();
        let back: MenuAction = serde_json::from_str(&s).unwrap();
        assert_eq!(back.label, "Copy result");
    }
}
