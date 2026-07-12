use quantum_domain::{DomainError, FilesError, ProcessesError, TimerError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ApplicationError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("dispatch failed for method `{method}`: {source}")]
    Dispatch { method: String, source: DomainError },
    /// A file-explorer subsystem error. `FilesError` is its own IPC contract
    /// (a serde-tagged union with plain-string payloads), so it is preserved
    /// intact rather than flattened into a `DomainError`; the frontend then
    /// sees the exact file failure (not found, permission denied, and so on).
    #[error(transparent)]
    Files(#[from] FilesError),
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
            Self::Files(e) => Self::files_rpc_code(e),
            Self::Unknown(_) => -32603,
        }
    }

    /// Stable JSON-RPC codes for the file-explorer error contract, kept in the
    /// domain range (`-32000..=-32099`). `NotFound` and `Unsupported` reuse the
    /// matching `DomainError` codes because they carry the same semantics; the
    /// remaining file-specific conditions allocate fresh codes. These are part
    /// of the public IPC contract and MUST NOT be renumbered once shipped.
    fn files_rpc_code(error: &FilesError) -> i32 {
        match error {
            FilesError::NotFound(_) => -32005,
            FilesError::Unsupported(_) => -32004,
            FilesError::PermissionDenied(_) => -32010,
            FilesError::AlreadyExists(_) => -32011,
            FilesError::Io(_) => -32012,
        }
    }
}

/// Map timer-subsystem errors onto the `ApplicationError`/`DomainError`
/// variants.
///
/// - `TimerError::NotFound` is the timer subsystem's "not found" condition. It
///   maps to `DomainError::NotFound`, the generic not-found variant, which
///   carries the missing identifier string and keeps a stable domain-range
///   JSON-RPC code distinct from `ProviderNotFound` (which is provider-specific
///   and wraps a `ProviderId`).
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
            TimerError::NotFound(_) => ApplicationError::Domain(DomainError::NotFound(message)),
            _ => ApplicationError::Domain(DomainError::ActionFailed { reason: message }),
        }
    }
}

/// Map process-subsystem errors onto the `ApplicationError`/`DomainError`
/// variants, following the same shape as the timer conversion above.
///
/// `ProcessesError` is not itself part of the serialized IPC contract (unlike
/// `FilesError`, it derives neither `Serialize` nor `Clone`), so rather than
/// give it a dedicated `ApplicationError` variant it is folded into
/// `DomainError`, preserving the original `Display` message:
///
/// - `ProcessesError::NotFound` is the subsystem's "not found" condition, so it
///   maps to `DomainError::NotFound`, carrying the missing pid's message and a
///   stable domain-range JSON-RPC code.
/// - Every other `ProcessesError` (permission denied, protected, sampling) is an
///   operation failure, so it maps to `DomainError::ActionFailed`, again keeping
///   the human-readable cause.
impl From<ProcessesError> for ApplicationError {
    fn from(error: ProcessesError) -> ApplicationError {
        let message = error.to_string();
        match error {
            ProcessesError::NotFound(_) => ApplicationError::Domain(DomainError::NotFound(message)),
            _ => ApplicationError::Domain(DomainError::ActionFailed { reason: message }),
        }
    }
}

pub type Result<T> = std::result::Result<T, ApplicationError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_not_found_maps_to_domain_not_found() {
        let timer_err = TimerError::NotFound("t1".to_string());
        let app_err: ApplicationError = timer_err.into();
        match app_err {
            ApplicationError::Domain(DomainError::NotFound(id)) => {
                assert!(id.contains("t1"));
            }
            other => panic!("expected DomainError::NotFound, got {other:?}"),
        }
    }

    #[test]
    fn timer_not_found_carries_not_found_rpc_code() {
        let timer_err = TimerError::NotFound("t1".to_string());
        let app_err: ApplicationError = timer_err.into();
        assert_eq!(app_err.rpc_code(), -32005);
    }

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
    fn files_error_converts_to_application_error() {
        let files_err = FilesError::PermissionDenied("/etc/shadow".to_string());
        let app_err: ApplicationError = files_err.into();
        match app_err {
            ApplicationError::Files(FilesError::PermissionDenied(path)) => {
                assert_eq!(path, "/etc/shadow");
            }
            other => panic!("expected ApplicationError::Files, got {other:?}"),
        }
    }

    #[test]
    fn files_error_serde_roundtrip() {
        let err = ApplicationError::Files(FilesError::NotFound("/missing".to_string()));
        let json = serde_json::to_string(&err).unwrap();
        let back: ApplicationError = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{}", err), format!("{}", back));
    }

    #[test]
    fn files_error_codes_stay_in_domain_range() {
        let errors = [
            FilesError::NotFound("x".to_string()),
            FilesError::PermissionDenied("x".to_string()),
            FilesError::AlreadyExists("x".to_string()),
            FilesError::Io("x".to_string()),
            FilesError::Unsupported("x".to_string()),
        ];
        for error in errors {
            let code = ApplicationError::Files(error).rpc_code();
            assert!(
                (-32099..=-32000).contains(&code),
                "code {} outside domain range",
                code
            );
        }
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
