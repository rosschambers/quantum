//! quantum-application

pub mod dispatcher;
pub mod error;
pub mod use_cases;

pub use dispatcher::Dispatcher;
pub use error::{ApplicationError, Result};
pub use use_cases::{
    LaunchActionUseCase, ListProvidersUseCase, OpenViewUseCase, ReloadThemeUseCase, SearchResponse,
    SearchUseCase,
};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
