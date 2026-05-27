//! quantum-application

pub mod error;
pub mod use_cases;

pub use error::{ApplicationError, Result};
pub use use_cases::{SearchResponse, SearchUseCase};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
