//! quantum-theme: theme store with embedded default + on-disk override.

pub mod error;
pub mod store;

pub use error::ThemeError;
pub use store::{ResolvedViewData, ThemeStore};
