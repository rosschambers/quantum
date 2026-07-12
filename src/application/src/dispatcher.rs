use crate::{
    ApplicationError, CreateTimerSpec, EditChanges, FilesService, LaunchActionUseCase,
    ListProvidersUseCase, OpenViewUseCase, QueryProviderUseCase, ReloadPluginsUseCase,
    ReloadThemeUseCase, Result, ScheduleActionUseCase, SearchUseCase, SetThemeUseCase,
    SubscribeProviderUseCase, TimerService,
};
use quantum_domain::{DomainError, WindowMode};
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct Dispatcher {
    search: Arc<SearchUseCase>,
    launch_action: Arc<LaunchActionUseCase>,
    list_providers: Arc<ListProvidersUseCase>,
    reload_theme: Arc<ReloadThemeUseCase>,
    set_theme: Arc<SetThemeUseCase>,
    open_view: Arc<OpenViewUseCase>,
    subscribe_provider: Arc<SubscribeProviderUseCase>,
    query_provider: Arc<QueryProviderUseCase>,
    schedule_action: Arc<ScheduleActionUseCase>,
    reload_plugins: Arc<ReloadPluginsUseCase>,
    timer_service: Arc<TimerService>,
    files_service: Arc<FilesService>,
}

/// Params for the three `view.*` handlers (`view.toggle`, `view.show`,
/// `view.hide`). All three accept the same single `name` field; only the
/// `WindowMode` passed to `OpenViewUseCase::execute` differs.
#[derive(serde::Deserialize)]
struct ViewParams {
    name: String,
}

/// Params for the many `files.*` handlers that take a single filesystem path
/// (`files.list`, `files.unpin`, `files.open`, `files.preview`, `files.watch`,
/// `files.unwatch`, `files.sizes`, `files.cancel_sizes`).
#[derive(serde::Deserialize)]
struct PathParam {
    path: String,
}

/// Serialize a handler response into a JSON value, mapping a serialization
/// failure onto the application error type rather than panicking.
fn to_json<T: serde::Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value)
        .map_err(|e| ApplicationError::Unknown(format!("serialization error: {e}")))
}

/// Deserialize a required JSON-RPC `params` slice directly into the
/// handler's typed request struct. Skips the `serde_json::Value` round
/// trip the old `from_value(Value)` path performed.
fn parse_params<T: DeserializeOwned>(params: Option<&RawValue>, context: &str) -> Result<T> {
    let raw =
        params.ok_or_else(|| ApplicationError::Unknown(format!("missing params for {context}")))?;
    serde_json::from_str::<T>(raw.get())
        .map_err(|e| ApplicationError::Unknown(format!("invalid {context} params: {e}")))
}

impl Dispatcher {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        search: Arc<SearchUseCase>,
        launch_action: Arc<LaunchActionUseCase>,
        list_providers: Arc<ListProvidersUseCase>,
        reload_theme: Arc<ReloadThemeUseCase>,
        set_theme: Arc<SetThemeUseCase>,
        open_view: Arc<OpenViewUseCase>,
        subscribe_provider: Arc<SubscribeProviderUseCase>,
        query_provider: Arc<QueryProviderUseCase>,
        schedule_action: Arc<ScheduleActionUseCase>,
        reload_plugins: Arc<ReloadPluginsUseCase>,
        timer_service: Arc<TimerService>,
        files_service: Arc<FilesService>,
    ) -> Self {
        Self {
            search,
            launch_action,
            list_providers,
            reload_theme,
            set_theme,
            open_view,
            subscribe_provider,
            query_provider,
            schedule_action,
            reload_plugins,
            timer_service,
            files_service,
        }
    }

    pub async fn dispatch(&self, method: &str, params: Option<&RawValue>) -> Result<Value> {
        match method {
            "search" => self.handle_search(params).await,
            "action.invoke" => self.handle_action_invoke(params).await,
            "action.schedule" => self.handle_action_schedule(params).await,
            "action.cancel" => self.handle_action_cancel(params).await,
            "action.scheduled" => self.handle_action_scheduled(params).await,
            "provider.list" => self.handle_provider_list(params).await,
            "provider.subscribe" => self.handle_provider_subscribe(params).await,
            "provider.query" => self.handle_provider_query(params).await,
            "view.toggle" => self.handle_view(params, WindowMode::Toggle).await,
            "view.show" => self.handle_view(params, WindowMode::Show).await,
            "view.hide" => self.handle_view(params, WindowMode::Hide).await,
            "view.set_height" => self.handle_view_set_height(params).await,
            "view.set_input_region" => self.handle_view_set_input_region(params).await,
            "theme.reload" => self.handle_theme_reload(params).await,
            "theme.set" => self.handle_theme_set(params).await,
            "plugin.reload" => self.handle_plugin_reload(params).await,
            "timer.create" => self.handle_timer_create(params).await,
            "timer.list" => self.handle_timer_list(params).await,
            "timer.edit" => self.handle_timer_edit(params).await,
            "timer.cancel" => self.handle_timer_cancel(params).await,
            "timer.dismiss" => self.handle_timer_dismiss(params).await,
            "timer.dismiss_all" => self.handle_timer_dismiss_all(params).await,
            "files.list" => self.handle_files_list(params).await,
            "files.places" => self.handle_files_places(params).await,
            "files.pin" => self.handle_files_pin(params).await,
            "files.unpin" => self.handle_files_unpin(params).await,
            "files.get_preferences" => self.handle_files_get_preferences(params).await,
            "files.set_preferences" => self.handle_files_set_preferences(params).await,
            "files.operation" => self.handle_files_operation(params).await,
            "files.open" => self.handle_files_open(params).await,
            "files.open_with" => self.handle_files_open_with(params).await,
            "files.applications" => self.handle_files_applications(params).await,
            "files.open_terminal" => self.handle_files_open_terminal(params).await,
            "files.preview" => self.handle_files_preview(params).await,
            "files.search" => self.handle_files_search(params).await,
            "files.watch" => self.handle_files_watch(params).await,
            "files.unwatch" => self.handle_files_unwatch(params).await,
            "files.sizes" => self.handle_files_sizes(params).await,
            "files.cancel_sizes" => self.handle_files_cancel_sizes(params).await,
            "system.status" => self.handle_system_status(params).await,
            _ => Err(ApplicationError::Domain(DomainError::Unsupported(
                method.to_string(),
            ))),
        }
    }

    async fn handle_search(&self, params: Option<&RawValue>) -> Result<Value> {
        let query: quantum_domain::Query = parse_params(params, "search")?;

        let response = self.search.execute(query).await?;
        serde_json::to_value(response)
            .map_err(|e| ApplicationError::Unknown(format!("serialization error: {}", e)))
    }

    async fn handle_action_invoke(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct ActionInvokeParams {
            provider: String,
            action: quantum_domain::Action,
        }

        let params: ActionInvokeParams = parse_params(params, "action invoke")?;

        self.launch_action
            .execute(params.provider.into(), params.action)
            .await?;

        Ok(json!({}))
    }

    async fn handle_provider_list(&self, _params: Option<&RawValue>) -> Result<Value> {
        let providers = self.list_providers.execute().await;
        let provider_strs: Vec<String> = providers.iter().map(|p| p.to_string()).collect();
        Ok(json!(provider_strs))
    }

    async fn handle_view(&self, params: Option<&RawValue>, mode: WindowMode) -> Result<Value> {
        let params: ViewParams = parse_params(params, "view")?;
        self.open_view.execute(params.name, mode).await?;
        Ok(json!({}))
    }

    async fn handle_view_set_height(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct SetHeightParams {
            name: String,
            height: u32,
        }
        let params: SetHeightParams = parse_params(params, "view.set_height")?;
        self.open_view
            .set_height(params.name, params.height)
            .await?;
        Ok(json!({}))
    }

    async fn handle_view_set_input_region(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct SetInputRegionParams {
            name: String,
            region: Option<quantum_domain::WindowInputRegion>,
        }
        let params: SetInputRegionParams = parse_params(params, "view.set_input_region")?;
        self.open_view
            .set_input_region(params.name, params.region)
            .await?;
        Ok(json!({}))
    }

    async fn handle_theme_reload(&self, _params: Option<&RawValue>) -> Result<Value> {
        self.reload_theme.execute().await?;
        Ok(json!({}))
    }

    async fn handle_theme_set(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct ThemeSetParams {
            theme: String,
        }
        let params: ThemeSetParams = parse_params(params, "theme.set")?;
        self.set_theme.execute(&params.theme).await?;
        Ok(json!({}))
    }

    async fn handle_plugin_reload(&self, _params: Option<&RawValue>) -> Result<Value> {
        let loaded = self.reload_plugins.execute().await?;
        Ok(json!({ "loaded": loaded }))
    }

    async fn handle_system_status(&self, _params: Option<&RawValue>) -> Result<Value> {
        let providers = self.list_providers.execute().await;
        // No theme-listing capability is wired into the dispatcher (the
        // `ThemeStore` port exposes no enumeration), so reporting a real theme
        // count is impossible here. Rather than emit a fabricated value, omit
        // the field entirely.
        Ok(json!({
            "version": quantum_domain::version(),
            "providers_count": providers.len(),
        }))
    }

    async fn handle_provider_subscribe(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct SubscribeParams {
            provider: String,
        }

        let params: SubscribeParams = parse_params(params, "subscribe")?;
        self.subscribe_provider
            .execute(params.provider.into())
            .await?;
        Ok(json!({}))
    }

    async fn handle_provider_query(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct QueryParams {
            id: String,
        }
        let params: QueryParams = parse_params(params, "query")?;
        self.query_provider.execute(params.id.into()).await
    }

    async fn handle_action_schedule(&self, params: Option<&RawValue>) -> Result<Value> {
        use crate::use_cases::schedule_action::InvokeParams;
        #[derive(serde::Deserialize)]
        struct ScheduleParams {
            delay_secs: u64,
            label: String,
            action: InvokeParams,
        }
        let p: ScheduleParams = parse_params(params, "action.schedule")?;
        let id = self
            .schedule_action
            .schedule(p.delay_secs, p.label, p.action)
            .await?;
        Ok(json!({ "id": id }))
    }

    async fn handle_action_cancel(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct CancelParams {
            id: String,
        }
        let p: CancelParams = parse_params(params, "action.cancel")?;
        self.schedule_action.cancel(p.id).await?;
        Ok(json!({}))
    }

    async fn handle_action_scheduled(&self, _params: Option<&RawValue>) -> Result<Value> {
        let jobs = self.schedule_action.list().await;
        Ok(json!({ "jobs": jobs }))
    }

    async fn handle_timer_create(&self, params: Option<&RawValue>) -> Result<Value> {
        let spec: CreateTimerSpec = parse_params(params, "timer.create")?;
        let id = self.timer_service.create(spec).await?;
        Ok(json!({ "id": id.as_str() }))
    }

    async fn handle_timer_list(&self, _params: Option<&RawValue>) -> Result<Value> {
        let data = self.timer_service.list().await;
        serde_json::to_value(data).map_err(|e| ApplicationError::Unknown(e.to_string()))
    }

    async fn handle_timer_edit(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct EditParams {
            id: String,
            changes: EditChanges,
        }
        let p: EditParams = parse_params(params, "timer.edit")?;
        self.timer_service.edit(p.id.into(), p.changes).await?;
        Ok(json!({}))
    }

    async fn handle_timer_cancel(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct IdParam {
            id: String,
        }
        let p: IdParam = parse_params(params, "timer.cancel")?;
        self.timer_service.cancel(p.id.into()).await?;
        Ok(json!({}))
    }

    async fn handle_timer_dismiss(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct IdParam {
            id: String,
        }
        let p: IdParam = parse_params(params, "timer.dismiss")?;
        self.timer_service.dismiss(p.id.into()).await?;
        Ok(json!({}))
    }

    async fn handle_timer_dismiss_all(&self, _params: Option<&RawValue>) -> Result<Value> {
        let count = self.timer_service.dismiss_all().await?;
        Ok(json!({ "dismissed": count }))
    }

    async fn handle_files_list(&self, params: Option<&RawValue>) -> Result<Value> {
        let p: PathParam = parse_params(params, "files.list")?;
        let entries = self.files_service.list(&p.path).await?;
        to_json(entries)
    }

    async fn handle_files_places(&self, _params: Option<&RawValue>) -> Result<Value> {
        let places = self.files_service.places().await?;
        to_json(places)
    }

    async fn handle_files_pin(&self, params: Option<&RawValue>) -> Result<Value> {
        let pin: quantum_domain::Pin = parse_params(params, "files.pin")?;
        let pins = self.files_service.pin(pin).await?;
        to_json(pins)
    }

    async fn handle_files_unpin(&self, params: Option<&RawValue>) -> Result<Value> {
        let p: PathParam = parse_params(params, "files.unpin")?;
        let pins = self.files_service.unpin(&p.path).await?;
        to_json(pins)
    }

    async fn handle_files_get_preferences(&self, _params: Option<&RawValue>) -> Result<Value> {
        let preferences = self.files_service.get_preferences().await;
        to_json(preferences)
    }

    async fn handle_files_set_preferences(&self, params: Option<&RawValue>) -> Result<Value> {
        let preferences: quantum_domain::FilePreferences =
            parse_params(params, "files.set_preferences")?;
        self.files_service.set_preferences(preferences).await?;
        Ok(Value::Null)
    }

    async fn handle_files_operation(&self, params: Option<&RawValue>) -> Result<Value> {
        let operation: quantum_domain::FileOperation = parse_params(params, "files.operation")?;
        self.files_service.operation(operation).await?;
        Ok(json!({}))
    }

    async fn handle_files_open(&self, params: Option<&RawValue>) -> Result<Value> {
        let p: PathParam = parse_params(params, "files.open")?;
        self.files_service.open(&p.path).await?;
        Ok(json!({}))
    }

    async fn handle_files_open_with(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct OpenWithParams {
            path: String,
            desktop_id: String,
        }
        let p: OpenWithParams = parse_params(params, "files.open_with")?;
        self.files_service.open_with(&p.path, &p.desktop_id).await?;
        Ok(json!({}))
    }

    async fn handle_files_applications(&self, _params: Option<&RawValue>) -> Result<Value> {
        let applications = self.files_service.applications().await;
        to_json(applications)
    }

    async fn handle_files_open_terminal(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct OpenTerminalParams {
            directory: String,
        }
        let p: OpenTerminalParams = parse_params(params, "files.open_terminal")?;
        self.files_service.open_terminal(&p.directory).await?;
        Ok(json!({}))
    }

    async fn handle_files_preview(&self, params: Option<&RawValue>) -> Result<Value> {
        let p: PathParam = parse_params(params, "files.preview")?;
        let preview = self.files_service.preview(&p.path).await?;
        to_json(preview)
    }

    async fn handle_files_search(&self, params: Option<&RawValue>) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct SearchParams {
            root: String,
            query: String,
            limit: usize,
        }
        let p: SearchParams = parse_params(params, "files.search")?;
        let entries = self
            .files_service
            .search(&p.root, &p.query, p.limit)
            .await?;
        to_json(entries)
    }

    async fn handle_files_watch(&self, params: Option<&RawValue>) -> Result<Value> {
        let p: PathParam = parse_params(params, "files.watch")?;
        self.files_service.watch(&p.path)?;
        Ok(json!({}))
    }

    async fn handle_files_unwatch(&self, params: Option<&RawValue>) -> Result<Value> {
        let p: PathParam = parse_params(params, "files.unwatch")?;
        self.files_service.unwatch(&p.path);
        Ok(json!({}))
    }

    async fn handle_files_sizes(&self, params: Option<&RawValue>) -> Result<Value> {
        let p: PathParam = parse_params(params, "files.sizes")?;
        self.files_service.sizes(&p.path);
        Ok(json!({}))
    }

    async fn handle_files_cancel_sizes(&self, params: Option<&RawValue>) -> Result<Value> {
        let p: PathParam = parse_params(params, "files.cancel_sizes")?;
        self.files_service.cancel_sizes(&p.path);
        Ok(json!({}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchResponse;
    use async_trait::async_trait;
    use futures::stream::{self, BoxStream, StreamExt};
    use quantum_domain::{
        Action, ActionOutcome, ApplicationCatalog, ApplicationInfo, CivilNow, Clock, ContentKind,
        DirectoryWatcher, DomainError, DriveInfo, EventBus, FileEntry, FileEntryKind, FileOpener,
        FileOperation, FilePreferences, FileSystemPort, FilesError, Match, MatchScore,
        PermissionClass, Pin, PinsPort, PreferencesPort, ProviderId, ProviderRegistry,
        ProviderSource, Query, RecursiveSizer, SizeUpdate, ThemeStore, Timer, TimerBroadcast,
        TimerError, TimerNotifier, TimerStore, TimerStoreData, Weekday, WindowHost,
    };
    use std::collections::HashMap;

    /// Convenience: encode a `serde_json::Value` as a `Box<RawValue>` so
    /// tests can keep using `json!` literals while exercising the
    /// production deserialization path (from raw JSON bytes, not from a
    /// `Value` tree).
    fn raw(value: Value) -> Box<RawValue> {
        serde_json::value::to_raw_value(&value).expect("value to raw")
    }

    #[derive(Clone)]
    struct FakeProvider {
        id: ProviderId,
    }

    #[async_trait]
    impl ProviderSource for FakeProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }

        async fn search(&self, _q: &Query) -> std::result::Result<Vec<Match>, DomainError> {
            Ok(vec![Match {
                id: "test-match".to_string(),
                provider: self.id.clone(),
                title: "Test Match".to_string(),
                subtitle: None,
                icon: None,
                score: MatchScore::new(0.9),
                action: Action::Launch {
                    desktop_id: "test".to_string(),
                },
            }])
        }

        async fn invoke(&self, _a: &Action) -> std::result::Result<ActionOutcome, DomainError> {
            Ok(ActionOutcome { message: None })
        }
    }

    struct FakeRegistry {
        providers: HashMap<ProviderId, Arc<dyn ProviderSource>>,
    }

    #[async_trait]
    impl ProviderRegistry for FakeRegistry {
        async fn list(&self) -> Vec<ProviderId> {
            self.providers.keys().cloned().collect()
        }

        async fn get(&self, id: &ProviderId) -> Option<Arc<dyn ProviderSource>> {
            self.providers.get(id).cloned()
        }
    }

    struct FakeThemeStore;

    #[async_trait]
    impl ThemeStore for FakeThemeStore {
        async fn load_theme(&self, _name: &str) -> std::result::Result<(), DomainError> {
            Ok(())
        }

        async fn reload(&self) -> std::result::Result<(), DomainError> {
            Ok(())
        }

        fn get_file(&self, _theme_name: &str, _path: &str) -> Option<Vec<u8>> {
            None
        }

        fn get_asset(&self, _path: &str) -> Option<Vec<u8>> {
            None
        }

        fn get_plugin_file(&self, _plugin_name: &str, _path: &str) -> Option<Vec<u8>> {
            None
        }

        fn resolved_tokens(&self) -> std::collections::HashMap<String, String> {
            std::collections::HashMap::new()
        }
    }

    struct FakeEventBus;

    #[async_trait]
    impl EventBus for FakeEventBus {
        async fn publish(
            &self,
            _event: &str,
            _payload: &str,
        ) -> std::result::Result<(), DomainError> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FakeWindowHost;

    #[async_trait]
    impl WindowHost for FakeWindowHost {
        async fn open(
            &self,
            _view: &str,
            _mode: WindowMode,
        ) -> std::result::Result<(), DomainError> {
            Ok(())
        }

        async fn set_view_height(
            &self,
            _view: &str,
            _height: u32,
        ) -> std::result::Result<(), DomainError> {
            Ok(())
        }

        async fn set_view_input_region(
            &self,
            _view: &str,
            _region: Option<quantum_domain::WindowInputRegion>,
        ) -> std::result::Result<(), DomainError> {
            Ok(())
        }
    }

    struct FakePluginCatalog;

    #[async_trait]
    impl quantum_domain::PluginCatalog for FakePluginCatalog {
        async fn discover(&self) -> std::result::Result<usize, DomainError> {
            Ok(0)
        }
    }

    /// Minimal clock fixed at a Monday 09:00 for timer dispatch tests.
    struct FakeClock;

    impl Clock for FakeClock {
        fn now_unix(&self) -> u64 {
            1_000_000
        }
        fn local_civil(&self) -> CivilNow {
            CivilNow {
                weekday: Weekday::Monday,
                secs_into_day: 9 * 3600,
            }
        }
    }

    /// In-memory timer store that simply round-trips the last saved data.
    struct FakeTimerStore;

    #[async_trait]
    impl TimerStore for FakeTimerStore {
        async fn load(&self) -> std::result::Result<TimerStoreData, TimerError> {
            Ok(TimerStoreData::default())
        }
        async fn save(&self, _data: &TimerStoreData) -> std::result::Result<(), TimerError> {
            Ok(())
        }
    }

    struct FakeTimerNotifier;

    #[async_trait]
    impl TimerNotifier for FakeTimerNotifier {
        async fn notify_complete(&self, _timer: &Timer) {}
    }

    struct FakeTimerBroadcast;

    impl TimerBroadcast for FakeTimerBroadcast {
        fn publish(&self, _data: &TimerStoreData) {}
    }

    /// The single directory entry the files fakes report, so the `files.list`
    /// routing test has a known value to assert against.
    fn files_sample_entry() -> FileEntry {
        FileEntry {
            name: "notes.txt".to_string(),
            path: "/home/user/notes.txt".to_string(),
            kind: FileEntryKind::File,
            size: 12,
            recursive_size: None,
            modified_epoch_seconds: 0,
            owner: "user".to_string(),
            permissions: "rw-r--r--".to_string(),
            permission_class: PermissionClass::Normal,
            symlink_target: None,
            content_kind: ContentKind::Document,
        }
    }

    /// Filesystem fake for the dispatcher tests: `list_directory` and `search`
    /// return the single sample entry; the rest are inert.
    struct FilesFakeFileSystem;

    #[async_trait]
    impl FileSystemPort for FilesFakeFileSystem {
        async fn list_directory(
            &self,
            _path: &str,
        ) -> std::result::Result<Vec<FileEntry>, FilesError> {
            Ok(vec![files_sample_entry()])
        }
        async fn stat(&self, _path: &str) -> std::result::Result<FileEntry, FilesError> {
            Ok(files_sample_entry())
        }
        async fn mounts(&self) -> std::result::Result<Vec<DriveInfo>, FilesError> {
            Ok(Vec::new())
        }
        async fn read_text_preview(
            &self,
            _path: &str,
            _max_bytes: usize,
        ) -> std::result::Result<String, FilesError> {
            Ok(String::new())
        }
        async fn read_image_preview(
            &self,
            _path: &str,
            _max_dimension: u32,
        ) -> std::result::Result<String, FilesError> {
            Ok(String::new())
        }
        async fn perform(&self, _operation: FileOperation) -> std::result::Result<(), FilesError> {
            Ok(())
        }
        async fn search(
            &self,
            _root: &str,
            _query: &str,
            _limit: usize,
        ) -> std::result::Result<Vec<FileEntry>, FilesError> {
            Ok(vec![files_sample_entry()])
        }
    }

    struct FilesFakeWatcher;

    impl DirectoryWatcher for FilesFakeWatcher {
        fn watch(
            &self,
            _path: &str,
        ) -> std::result::Result<BoxStream<'static, String>, FilesError> {
            Ok(stream::empty().boxed())
        }
        fn unwatch(&self, _path: &str) {}
    }

    struct FilesFakeOpener;

    #[async_trait]
    impl FileOpener for FilesFakeOpener {
        async fn open(&self, _path: &str) -> std::result::Result<(), FilesError> {
            Ok(())
        }
        async fn open_with(
            &self,
            _path: &str,
            _desktop_id: &str,
        ) -> std::result::Result<(), FilesError> {
            Ok(())
        }
        async fn open_terminal(&self, _directory: &str) -> std::result::Result<(), FilesError> {
            Ok(())
        }
    }

    struct FilesFakeSizer;

    impl RecursiveSizer for FilesFakeSizer {
        fn compute(&self, _path: &str) -> BoxStream<'static, SizeUpdate> {
            stream::empty().boxed()
        }
        fn cancel(&self, _path: &str) {}
    }

    struct FilesFakePins;

    #[async_trait]
    impl PinsPort for FilesFakePins {
        async fn load(&self) -> Vec<Pin> {
            Vec::new()
        }
        async fn add(&self, pin: Pin) -> std::result::Result<Vec<Pin>, FilesError> {
            Ok(vec![pin])
        }
        async fn remove(&self, _path: &str) -> std::result::Result<Vec<Pin>, FilesError> {
            Ok(Vec::new())
        }
    }

    struct FilesFakeApplications;

    #[async_trait]
    impl ApplicationCatalog for FilesFakeApplications {
        async fn list_applications(&self) -> Vec<ApplicationInfo> {
            Vec::new()
        }
    }

    struct FilesFakePreferences;

    #[async_trait]
    impl PreferencesPort for FilesFakePreferences {
        async fn load(&self) -> FilePreferences {
            FilePreferences::default()
        }
        async fn save(&self, _preferences: FilePreferences) -> std::result::Result<(), FilesError> {
            Ok(())
        }
    }

    /// Assemble a `FilesService` over the files fakes for dispatcher routing
    /// tests. The `FakeEventBus` above is reused for the event bus.
    fn build_files_service() -> Arc<FilesService> {
        Arc::new(FilesService::new(
            Arc::new(FilesFakeFileSystem),
            Arc::new(FilesFakeWatcher),
            Arc::new(FilesFakeOpener),
            Arc::new(FilesFakeSizer),
            Arc::new(FilesFakePins),
            Arc::new(FilesFakePreferences),
            Arc::new(FilesFakeApplications),
            Arc::new(FakeEventBus),
        ))
    }

    fn build_dispatcher() -> Arc<Dispatcher> {
        let mut providers = HashMap::new();
        providers.insert(
            "apps".into(),
            Arc::new(FakeProvider { id: "apps".into() }) as Arc<dyn ProviderSource>,
        );

        let registry = Arc::new(FakeRegistry { providers });
        let search = Arc::new(SearchUseCase::new(registry.clone()));
        let launch_action = Arc::new(LaunchActionUseCase::new(registry.clone()));
        let list_providers = Arc::new(ListProvidersUseCase::new(registry.clone()));
        let reload_theme = Arc::new(ReloadThemeUseCase::new(
            Arc::new(FakeThemeStore),
            Arc::new(FakeEventBus),
        ));
        let set_theme = Arc::new(SetThemeUseCase::new(
            Arc::new(FakeThemeStore),
            Arc::new(FakeEventBus),
        ));
        let open_view = Arc::new(OpenViewUseCase::new(Arc::new(FakeWindowHost)));
        let subscribe_provider = Arc::new(SubscribeProviderUseCase::new(
            registry.clone(),
            Arc::new(FakeEventBus),
        ));
        let query_provider = Arc::new(QueryProviderUseCase::new(registry));

        let schedule_action = Arc::new(super::ScheduleActionUseCase::new(launch_action.clone()));
        let reload_plugins = Arc::new(super::ReloadPluginsUseCase::new(Arc::new(
            FakePluginCatalog,
        )));
        let timer_service = Arc::new(TimerService::new(
            Arc::new(FakeClock),
            Arc::new(FakeTimerStore),
            Arc::new(FakeTimerNotifier),
            Arc::new(FakeTimerBroadcast),
        ));
        Arc::new(Dispatcher::new(
            search,
            launch_action,
            list_providers,
            reload_theme,
            set_theme,
            open_view,
            subscribe_provider,
            query_provider,
            schedule_action,
            reload_plugins,
            timer_service,
            build_files_service(),
        ))
    }

    #[tokio::test]
    async fn dispatches_search() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({ "text": "test", "providers": ["apps"] }));
        let resp = dispatcher.dispatch("search", Some(&params)).await.unwrap();

        let parsed: SearchResponse = serde_json::from_value(resp).unwrap();
        assert_eq!(parsed.matches.len(), 1);
        assert_eq!(parsed.matches[0].title, "Test Match");
    }

    #[tokio::test]
    async fn dispatches_action_invoke() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({
            "provider": "apps",
            "action": {
                "kind": "launch",
                "data": { "desktop_id": "firefox" }
            }
        }));
        let resp = dispatcher.dispatch("action.invoke", Some(&params)).await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_provider_list() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({}));
        let resp = dispatcher
            .dispatch("provider.list", Some(&params))
            .await
            .unwrap();

        let providers: Vec<String> = serde_json::from_value(resp).unwrap();
        assert_eq!(providers.len(), 1);
        assert!(providers.contains(&"apps".to_string()));
    }

    #[tokio::test]
    async fn dispatches_provider_subscribe() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({ "provider": "apps" }));
        let resp = dispatcher
            .dispatch("provider.subscribe", Some(&params))
            .await;

        // FakeProvider does not support subscriptions, so this should fail with Unsupported.
        assert!(matches!(
            resp,
            Err(ApplicationError::Domain(DomainError::Unsupported(_)))
        ));
    }

    #[tokio::test]
    async fn dispatches_view_toggle() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({ "name": "launcher" }));
        let resp = dispatcher.dispatch("view.toggle", Some(&params)).await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_view_show() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({ "name": "launcher" }));
        let resp = dispatcher.dispatch("view.show", Some(&params)).await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_view_hide() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({ "name": "launcher" }));
        let resp = dispatcher.dispatch("view.hide", Some(&params)).await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_view_set_input_region() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({
            "name": "plugin/bar/bar",
            "region": { "x": 0, "y": 0, "width": 300, "height": 32 }
        }));
        let resp = dispatcher
            .dispatch("view.set_input_region", Some(&params))
            .await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_view_set_input_region_with_null_region() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({ "name": "plugin/bar/bar", "region": null }));
        let resp = dispatcher
            .dispatch("view.set_input_region", Some(&params))
            .await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_theme_reload() {
        let dispatcher = build_dispatcher();
        // theme.reload legitimately carries no params.
        let resp = dispatcher.dispatch("theme.reload", None).await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_system_status() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher.dispatch("system.status", None).await.unwrap();

        let status: serde_json::Value = resp;
        assert!(status.get("version").is_some());
        assert_eq!(status["providers_count"], 1);
        // themes_count is intentionally absent: the dispatcher has no
        // theme-listing capability, so no honest count can be reported.
        assert!(status.get("themes_count").is_none());
    }

    #[tokio::test]
    async fn returns_unsupported_for_unknown_method() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({}));
        let resp = dispatcher.dispatch("unknown.method", Some(&params)).await;

        assert!(matches!(
            resp,
            Err(ApplicationError::Domain(DomainError::Unsupported(_)))
        ));
    }

    #[tokio::test]
    async fn action_scheduled_lists_empty_initially() {
        let dispatcher = build_dispatcher();
        let listed = dispatcher
            .dispatch("action.scheduled", None)
            .await
            .expect("list");
        let jobs = listed["jobs"].as_array().expect("jobs");
        assert_eq!(jobs.len(), 0);
    }

    #[tokio::test]
    async fn action_schedule_then_scheduled_then_cancel_round_trip() {
        let dispatcher = build_dispatcher();
        // Schedule a job 60s out (won't fire during the test).
        let schedule_params = raw(json!({
            "delay_secs": 60,
            "label": "Suspend",
            "action": {
                "provider": "apps",
                "action": { "kind": "launch", "data": { "desktop_id": "noop" } }
            }
        }));
        let scheduled = dispatcher
            .dispatch("action.schedule", Some(&schedule_params))
            .await
            .expect("schedule");
        let id = scheduled["id"].as_str().expect("id").to_string();
        assert_eq!(id.len(), 8);

        // List shows the job.
        let listed = dispatcher
            .dispatch("action.scheduled", None)
            .await
            .expect("list");
        let jobs = listed["jobs"].as_array().expect("jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["id"], id);
        assert_eq!(jobs[0]["label"], "Suspend");

        // Cancel removes it.
        let cancel_params = raw(json!({ "id": id }));
        dispatcher
            .dispatch("action.cancel", Some(&cancel_params))
            .await
            .expect("cancel");
        let listed2 = dispatcher
            .dispatch("action.scheduled", None)
            .await
            .expect("list 2");
        assert_eq!(listed2["jobs"].as_array().expect("jobs").len(), 0);
    }

    #[tokio::test]
    async fn timer_create_then_list_returns_one_timer() {
        let dispatcher = build_dispatcher();
        let create_params = raw(json!({
            "label": "Tea",
            "start": { "kind": "duration", "secs": 300 }
        }));
        let created = dispatcher
            .dispatch("timer.create", Some(&create_params))
            .await
            .expect("create");
        assert!(
            created["id"].as_str().is_some(),
            "timer.create must return an id"
        );

        let listed = dispatcher.dispatch("timer.list", None).await.expect("list");
        let timers = listed["timers"].as_array().expect("timers array");
        assert_eq!(timers.len(), 1);
        assert_eq!(timers[0]["label"], "Tea");
    }

    #[tokio::test]
    async fn timer_dismiss_all_removes_every_timer() {
        let dispatcher = build_dispatcher();
        for label in ["Tea", "Coffee"] {
            let create_params = raw(json!({
                "label": label,
                "start": { "kind": "duration", "secs": 300 }
            }));
            dispatcher
                .dispatch("timer.create", Some(&create_params))
                .await
                .expect("create");
        }

        let dismissed = dispatcher
            .dispatch("timer.dismiss_all", None)
            .await
            .expect("dismiss_all");
        assert_eq!(dismissed["dismissed"], 2);

        let listed = dispatcher.dispatch("timer.list", None).await.expect("list");
        let timers = listed["timers"].as_array().expect("timers array");
        assert_eq!(timers.len(), 0);
    }

    #[tokio::test]
    async fn dispatches_files_list() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({ "path": "/home/user" }));
        let resp = dispatcher
            .dispatch("files.list", Some(&params))
            .await
            .expect("files.list");

        let entries: Vec<FileEntry> = serde_json::from_value(resp).expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "notes.txt");
    }

    #[tokio::test]
    async fn files_list_missing_params_errors() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher.dispatch("files.list", None).await;
        assert!(resp.is_err());
    }

    #[tokio::test]
    async fn dispatches_files_get_preferences() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher
            .dispatch("files.get_preferences", None)
            .await
            .expect("files.get_preferences");

        let preferences: FilePreferences = serde_json::from_value(resp).expect("preferences");
        // The fake preferences port returns the defaults (dotfiles shown).
        assert!(preferences.show_hidden);
    }

    #[tokio::test]
    async fn dispatches_files_set_preferences() {
        let dispatcher = build_dispatcher();
        let params = raw(json!({ "show_hidden": false }));
        let resp = dispatcher
            .dispatch("files.set_preferences", Some(&params))
            .await
            .expect("files.set_preferences");

        assert_eq!(resp, Value::Null);
    }
}
