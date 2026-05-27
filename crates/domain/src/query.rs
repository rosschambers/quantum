use serde::{Deserialize, Serialize};
use crate::ProviderId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    pub text: String,
    #[serde(default)]
    pub providers: Vec<ProviderId>,
    #[serde(default)]
    pub limit: Option<u32>,
}

impl Query {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            providers: vec![],
            limit: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_constructs_with_empty_providers_and_no_limit() {
        let q = Query::new("foo");
        assert_eq!(q.text, "foo");
        assert_eq!(q.providers.len(), 0);
        assert_eq!(q.limit, None);
    }

    #[test]
    fn serde_roundtrip() {
        let q = Query {
            text: "test".to_string(),
            providers: vec![ProviderId::from("apps")],
            limit: Some(10),
        };
        let s = serde_json::to_string(&q).unwrap();
        let back: Query = serde_json::from_str(&s).unwrap();
        assert_eq!(q.text, back.text);
        assert_eq!(q.providers, back.providers);
        assert_eq!(q.limit, back.limit);
    }
}
