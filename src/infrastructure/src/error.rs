use serde::{Deserialize, Serialize};
use std::io;
use thiserror::Error;

use quantum_domain::DomainError;

/// Infrastructure layer errors, wrapping domain errors and adding I/O specific variants.
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum InfrastructureError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error("IO error: {0}")]
    Io(String),

    #[error("JSON serialization error: {0}")]
    Serde(String),

    #[error("config parse error: {0}")]
    ConfigParse(String),

    #[error("Hyprland unreachable")]
    HyprlandUnreachable,

    #[error("spawn error: {0}")]
    Spawn(String),

    #[error("DBus service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("DBus transport error: {0}")]
    DbusTransport(String),
}

impl From<io::Error> for InfrastructureError {
    fn from(e: io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<serde_json::Error> for InfrastructureError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e.to_string())
    }
}

impl From<quantum_dbus::DbusError> for InfrastructureError {
    fn from(e: quantum_dbus::DbusError) -> Self {
        use quantum_dbus::DbusError as D;
        match e {
            D::Transport(s) => InfrastructureError::DbusTransport(s),
            D::ServiceUnavailable(s) => InfrastructureError::ServiceUnavailable(s),
        }
    }
}

impl From<quantum_config::ConfigError> for InfrastructureError {
    fn from(e: quantum_config::ConfigError) -> Self {
        use quantum_config::ConfigError as C;
        match e {
            C::Io(s) => InfrastructureError::Io(s),
            C::ConfigParse(s) => InfrastructureError::ConfigParse(s),
        }
    }
}

impl From<quantum_theme::ThemeError> for InfrastructureError {
    fn from(e: quantum_theme::ThemeError) -> Self {
        use quantum_theme::ThemeError as T;
        match e {
            T::Io(s) => InfrastructureError::Io(s),
            T::Parse(s) => InfrastructureError::ConfigParse(s),
        }
    }
}

impl From<quantum_ipc::IpcError> for InfrastructureError {
    fn from(e: quantum_ipc::IpcError) -> Self {
        use quantum_ipc::IpcError as I;
        match e {
            I::Io(s) => InfrastructureError::Io(s),
            I::Serde(s) => InfrastructureError::Serde(s),
        }
    }
}

impl From<quantum_hyprland::HyprlandError> for InfrastructureError {
    fn from(e: quantum_hyprland::HyprlandError) -> Self {
        use quantum_hyprland::HyprlandError as H;
        match e {
            H::Io(s) => InfrastructureError::Io(s),
            H::Serde(s) => InfrastructureError::Serde(s),
            H::Unreachable => InfrastructureError::HyprlandUnreachable,
        }
    }
}

impl InfrastructureError {
    /// Lossy conversion to DomainError for IPC boundaries.
    pub fn to_domain(&self) -> DomainError {
        match self {
            InfrastructureError::Domain(e) => e.clone(),
            InfrastructureError::Io(msg) => DomainError::Unsupported(format!("IO error: {}", msg)),
            InfrastructureError::Serde(msg) => {
                DomainError::Unsupported(format!("serde error: {}", msg))
            }
            InfrastructureError::ConfigParse(msg) => {
                DomainError::InvalidQuery(format!("config: {}", msg))
            }
            InfrastructureError::HyprlandUnreachable => {
                DomainError::Unsupported("Hyprland unreachable".to_string())
            }
            InfrastructureError::Spawn(msg) => DomainError::Unsupported(format!("spawn: {}", msg)),
            InfrastructureError::ServiceUnavailable(msg) => {
                DomainError::Unsupported(format!("service unavailable: {}", msg))
            }
            InfrastructureError::DbusTransport(msg) => {
                DomainError::Unsupported(format!("DBus transport: {}", msg))
            }
        }
    }

    /// Stable JSON-RPC error code per the design doc.
    ///
    /// Infrastructure-specific errors occupy the range `-32100..-32199`.
    /// `Domain` delegates to `DomainError::rpc_code` so domain semantics
    /// are preserved when domain errors travel wrapped in infrastructure
    /// errors. These codes are part of the public IPC contract — they are
    /// stable and MUST NOT be renumbered once shipped. New variants must
    /// allocate the next free code in the range.
    pub fn rpc_code(&self) -> i32 {
        match self {
            Self::Domain(e) => e.rpc_code(),
            Self::Io(_) => -32100,
            Self::Serde(_) => -32101,
            Self::ConfigParse(_) => -32102,
            Self::HyprlandUnreachable => -32103,
            Self::Spawn(_) => -32104,
            Self::ServiceUnavailable(_) => -32105,
            Self::DbusTransport(_) => -32106,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_io_error() {
        let e = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let ie: InfrastructureError = e.into();
        assert!(matches!(ie, InfrastructureError::Io(_)));
    }

    #[test]
    fn from_serde_json_error() {
        let e: Result<(), serde_json::Error> = serde_json::from_str("invalid json");
        let ie: InfrastructureError = e.unwrap_err().into();
        assert!(matches!(ie, InfrastructureError::Serde(_)));
    }

    #[test]
    fn from_domain_error() {
        let de = DomainError::Unsupported("test".to_string());
        let ie: InfrastructureError = de.clone().into();
        match ie {
            InfrastructureError::Domain(d) => assert_eq!(d, de),
            _ => panic!("expected Domain variant"),
        }
    }

    #[test]
    fn to_domain_preserves_domain_error() {
        let de = DomainError::Unsupported("test".to_string());
        let ie = InfrastructureError::Domain(de.clone());
        let result = ie.to_domain();
        assert_eq!(result, de);
    }

    #[test]
    fn to_domain_maps_io_error() {
        let ie = InfrastructureError::Io("connection failed".to_string());
        let de = ie.to_domain();
        assert!(matches!(de, DomainError::Unsupported(_)));
    }

    #[test]
    fn to_domain_maps_hyprland_unreachable() {
        let ie = InfrastructureError::HyprlandUnreachable;
        let de = ie.to_domain();
        assert!(matches!(de, DomainError::Unsupported(_)));
    }

    #[test]
    fn io_has_stable_rpc_code() {
        let e = InfrastructureError::Io("disk full".to_string());
        assert_eq!(e.rpc_code(), -32100);
    }

    #[test]
    fn hyprland_unreachable_has_stable_rpc_code() {
        let e = InfrastructureError::HyprlandUnreachable;
        assert_eq!(e.rpc_code(), -32103);
    }

    #[test]
    fn domain_variant_delegates_rpc_code_to_inner() {
        let inner = DomainError::ProviderNotFound("apps".into());
        let e = InfrastructureError::Domain(inner.clone());
        assert_eq!(e.rpc_code(), inner.rpc_code());
        assert_eq!(e.rpc_code(), -32001);
    }

    #[test]
    fn all_infra_specific_codes_are_in_documented_range() {
        // Domain variant is delegated and excluded here; only infra-specific
        // variants should land in the -32100.. range.
        let codes = [
            InfrastructureError::Io("x".to_string()).rpc_code(),
            InfrastructureError::Serde("x".to_string()).rpc_code(),
            InfrastructureError::ConfigParse("x".to_string()).rpc_code(),
            InfrastructureError::HyprlandUnreachable.rpc_code(),
            InfrastructureError::Spawn("x".to_string()).rpc_code(),
            InfrastructureError::ServiceUnavailable("x".to_string()).rpc_code(),
            InfrastructureError::DbusTransport("x".to_string()).rpc_code(),
        ];
        for code in codes {
            assert!(
                (-32199..=-32100).contains(&code),
                "code {} outside infra range",
                code
            );
        }
    }
}
