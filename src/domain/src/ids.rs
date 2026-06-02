use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ProviderId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ProviderId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowId(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_from_str_and_string_equal() {
        assert_eq!(
            ProviderId::from("apps"),
            ProviderId::from("apps".to_string())
        );
    }

    #[test]
    fn provider_id_serde_roundtrip() {
        let id = ProviderId::from("apps");
        let s = serde_json::to_string(&id).unwrap();
        let back: ProviderId = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
    }
}
