use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Action {
    Launch {
        desktop_id: String,
    },
    Shell {
        command: Vec<String>,
        #[serde(default)]
        terminal: bool,
    },
    Focus {
        window_address: String,
    },
    Custom {
        kind: String,
        payload: serde_json::Value,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip_launch() {
        let action = Action::Launch {
            desktop_id: "firefox.desktop".to_string(),
        };
        let s = serde_json::to_string(&action).unwrap();
        let back: Action = serde_json::from_str(&s).unwrap();
        match back {
            Action::Launch { desktop_id } => assert_eq!(desktop_id, "firefox.desktop"),
            _ => panic!("Expected Launch variant"),
        }
    }

    #[test]
    fn serde_roundtrip_shell() {
        let action = Action::Shell {
            command: vec!["sh".to_string(), "-c".to_string(), "echo hi".to_string()],
            terminal: false,
        };
        let s = serde_json::to_string(&action).unwrap();
        let back: Action = serde_json::from_str(&s).unwrap();
        match back {
            Action::Shell { command, terminal } => {
                assert_eq!(command, vec!["sh", "-c", "echo hi"]);
                assert!(!terminal);
            }
            _ => panic!("Expected Shell variant"),
        }
    }

    #[test]
    fn serde_roundtrip_focus() {
        let action = Action::Focus {
            window_address: "0x12345678".to_string(),
        };
        let s = serde_json::to_string(&action).unwrap();
        let back: Action = serde_json::from_str(&s).unwrap();
        match back {
            Action::Focus { window_address } => assert_eq!(window_address, "0x12345678"),
            _ => panic!("Expected Focus variant"),
        }
    }

    #[test]
    fn serde_roundtrip_custom() {
        let payload = serde_json::json!({ "foo": "bar" });
        let action = Action::Custom {
            kind: "custom_type".to_string(),
            payload: payload.clone(),
        };
        let s = serde_json::to_string(&action).unwrap();
        let back: Action = serde_json::from_str(&s).unwrap();
        match back {
            Action::Custom { kind, payload: p } => {
                assert_eq!(kind, "custom_type");
                assert_eq!(p, payload);
            }
            _ => panic!("Expected Custom variant"),
        }
    }
}
