//! quantum-application

pub mod dispatcher;
pub mod error;
pub mod shell_capture;
pub mod use_cases;

pub use dispatcher::Dispatcher;
pub use error::{ApplicationError, Result};
pub use shell_capture::{CaptureSink, ShellCaptureUseCase};
pub use use_cases::{
    ClipboardService, CreateTimerSpec, EditChanges, FilesService, LaunchActionUseCase,
    ListProvidersUseCase, OpenViewUseCase, Places, PreviewKind, PreviewPayload, ProcessesService,
    QueryProviderUseCase, ReloadPluginsUseCase, ReloadThemeUseCase, ScheduleActionUseCase,
    ScheduleId, ScheduledJobSummary, SearchResponse, SearchUseCase, SetThemeUseCase,
    SubscribeProviderUseCase, TimerService, TimerStart,
};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
