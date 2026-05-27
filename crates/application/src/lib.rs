//! quantum-application

pub mod error;

pub use error::{ApplicationError, Result};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
