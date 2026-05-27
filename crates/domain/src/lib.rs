//! quantum-domain

pub mod action;
pub mod error;
pub mod ids;
pub mod match_result;
pub mod ports;
pub mod query;
pub mod score;

pub use action::Action;
pub use error::DomainError;
pub use ids::{ProviderId, WindowId};
pub use match_result::{IconRef, Match};
pub use ports::{
    ActionOutcome, ConfigStore, EventBus, HyprlandClient, ProviderCapabilities, ProviderRegistry,
    ProviderSource, ShellExecutor, ThemeStore, WindowHost,
};
pub use query::Query;
pub use score::MatchScore;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
