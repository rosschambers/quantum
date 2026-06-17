use quantum_domain::{DomainError, TimerError};
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

impl ApplicationError {
    /// Stable JSON-RPC error code per the design doc.
    ///
    /// `Domain` and `Dispatch` delegate to `DomainError::rpc_code` so the
    /// frontend can distinguish e.g. `ProviderNotFound` from `ActionFailed`
    /// regardless of which application-layer wrapper an error travelled in.
    /// `Unknown` is the only variant that legitimately maps to the generic
    /// JSON-RPC internal-error code (`-32603`) because by construction we
    /// do not know its domain semantics.
    pub fn rpc_code(&self) -> i32 {
        match self {
            Self::Domain(e) => e.rpc_code(),
            Self::Dispatch { source, .. } => source.rpc_code(),
            Self::Unknown(_) => -32603,
        }
    }
}

/// Map timer-subsystem errors onto the EXISTING `ApplicationError`/`DomainError`
/// variants. This deliberately adds no new variant: a dedicated timer variant
/// would force a matching change in quantumd's error mapping, which is out of
/// scope for this task.
///
/// - `TimerError::NotFound` is the timer subsystem's "not found" condition.
///   `DomainError` has no generic not-found variant (`ProviderNotFound` is
///   provider-specific and wraps a `ProviderId`, not an arbitrary string), so
///   it routes through `DomainError::Unsupported`, which carries an arbitrary
///   message and keeps a stable domain-range JSON-RPC code.
/// - Every other `TimerError` (invalid time, empty weekday set, persistence,
///   invalid duration) is an operation failure, so it maps to
///   `DomainError::ActionFailed`, again preserving the original `Display`
///   message and a domain-range code.
///
/// Both arms use the `TimerError`'s `Display` output as the message so the
/// frontend still sees the human-readable cause.
impl From<TimerError> for ApplicationError {
    fn from(error: TimerError) -> ApplicationError {
        let message = error.to_string();
        match error {
            TimerError::NotFound(_) => {
                ApplicationError::Domain(DomainError::Unsupported(message))
            }
            _ => ApplicationError::Domain(DomainError::ActionFailed { reason: message }),
        }
    }
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

    #[test]
    fn domain_variant_delegates_rpc_code() {
        let err = ApplicationError::Domain(DomainError::ProviderNotFound("apps".into()));
        assert_eq!(err.rpc_code(), -32001);
    }

    #[test]
    fn dispatch_variant_delegates_rpc_code_to_source() {
        let err = ApplicationError::Dispatch {
            method: "search".to_string(),
            source: DomainError::InvalidQuery("oops".to_string()),
        };
        assert_eq!(err.rpc_code(), -32002);
    }

    #[test]
    fn unknown_variant_is_generic_internal_error() {
        let err = ApplicationError::Unknown("something exploded".to_string());
        assert_eq!(err.rpc_code(), -32603);
    }

    #[test]
    fn delegated_codes_stay_in_domain_range() {
        let err = ApplicationError::Dispatch {
            method: "search".to_string(),
            source: DomainError::ActionFailed {
                reason: "x".to_string(),
            },
        };
        let code = err.rpc_code();
        assert!(
            (-32099..=-32000).contains(&code),
            "code {} outside domain range",
            code
        );
    }
}
