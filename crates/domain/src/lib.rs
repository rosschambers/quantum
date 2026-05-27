//! quantum-domain

pub mod ids;
pub mod score;

pub use ids::{ProviderId, WindowId};
pub use score::MatchScore;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
