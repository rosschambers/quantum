use quantum_domain::DomainError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ApplicationError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("dispatch failed for method `{method}`: {source}")]
    Dispatch { method: String, source: DomainError },
    #[error("{0}")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, ApplicationError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_error_converts_to_application_error() {
        let domain_err = DomainError::InvalidQuery("bad".to_string());
        let app_err: ApplicationError = domain_err.into();
        match app_err {
            ApplicationError::Domain(DomainError::InvalidQuery(_)) => {}
            _ => panic!("unexpected error variant"),
        }
    }

    #[test]
    fn application_error_serde_roundtrip() {
        let err = ApplicationError::Domain(DomainError::ProviderNotFound("apps".into()));
        let json = serde_json::to_string(&err).unwrap();
        let back: ApplicationError = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{}", err), format!("{}", back));
    }

    #[test]
    fn dispatch_error_serde_roundtrip() {
        let err = ApplicationError::Dispatch {
            method: "search".to_string(),
            source: DomainError::InvalidQuery("oops".to_string()),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ApplicationError = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, ApplicationError::Dispatch { .. }));
    }
}
