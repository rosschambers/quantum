//! quantum-application

pub mod dispatcher;
pub mod error;
pub mod use_cases;

pub use dispatcher::Dispatcher;
pub use error::{ApplicationError, Result};
pub use use_cases::{
    CreateTimerSpec, EditChanges, FilesService, LaunchActionUseCase, ListProvidersUseCase,
    OpenViewUseCase, Places, PreviewKind, PreviewPayload, QueryProviderUseCase,
    ReloadPluginsUseCase, ReloadThemeUseCase, ScheduleActionUseCase, ScheduleId,
    ScheduledJobSummary, SearchResponse, SearchUseCase, SetThemeUseCase, SubscribeProviderUseCase,
    TimerService, TimerStart,
};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
