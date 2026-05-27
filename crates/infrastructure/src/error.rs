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
            InfrastructureError::Spawn(msg) => {
                DomainError::Unsupported(format!("spawn: {}", msg))
            }
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
}
