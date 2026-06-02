use crate::ProviderId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum DomainError {
    #[error("provider not found: {0}")]
    ProviderNotFound(ProviderId),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("action failed: {reason}")]
    ActionFailed { reason: String },
    #[error("unsupported: {0}")]
    Unsupported(String),
}

impl DomainError {
    /// Stable JSON-RPC error code per the design doc.
    ///
    /// Domain errors occupy the range `-32000..-32099`. These codes are part
    /// of the public IPC contract — they are stable and MUST NOT be renumbered
    /// once shipped. New variants must allocate the next free code in the
    /// range; do not reuse codes that were freed by removed variants.
    pub fn rpc_code(&self) -> i32 {
        match self {
            Self::ProviderNotFound(_) => -32001,
            Self::InvalidQuery(_) => -32002,
            Self::ActionFailed { .. } => -32003,
            Self::Unsupported(_) => -32004,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_not_found_has_stable_code() {
        let e = DomainError::ProviderNotFound("apps".into());
        assert_eq!(e.rpc_code(), -32001);
    }

    #[test]
    fn invalid_query_has_stable_code() {
        let e = DomainError::InvalidQuery("invalid".to_string());
        assert_eq!(e.rpc_code(), -32002);
    }

    #[test]
    fn action_failed_has_stable_code() {
        let e = DomainError::ActionFailed {
            reason: "timeout".to_string(),
        };
        assert_eq!(e.rpc_code(), -32003);
    }

    #[test]
    fn unsupported_has_stable_code() {
        let e = DomainError::Unsupported("feature".to_string());
        assert_eq!(e.rpc_code(), -32004);
    }

    #[test]
    fn all_codes_are_in_domain_range() {
        let codes = [
            DomainError::ProviderNotFound("x".into()).rpc_code(),
            DomainError::InvalidQuery("x".to_string()).rpc_code(),
            DomainError::ActionFailed {
                reason: "x".to_string(),
            }
            .rpc_code(),
            DomainError::Unsupported("x".to_string()).rpc_code(),
        ];
        for code in codes {
            assert!(
                (-32099..=-32000).contains(&code),
                "code {} outside domain range",
                code
            );
        }
    }

    #[test]
    fn serde_roundtrip_provider_not_found() {
        let e = DomainError::ProviderNotFound("apps".into());
        let s = serde_json::to_string(&e).unwrap();
        let back: DomainError = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_invalid_query() {
        let e = DomainError::InvalidQuery("bad syntax".to_string());
        let s = serde_json::to_string(&e).unwrap();
        let back: DomainError = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_action_failed() {
        let e = DomainError::ActionFailed {
            reason: "spawn failed".to_string(),
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: DomainError = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_unsupported() {
        let e = DomainError::Unsupported("x11".to_string());
        let s = serde_json::to_string(&e).unwrap();
        let back: DomainError = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }
}
