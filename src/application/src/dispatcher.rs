use crate::{
    ApplicationError, LaunchActionUseCase, ListProvidersUseCase, OpenViewUseCase,
    QueryProviderUseCase, ReloadPluginsUseCase, ReloadThemeUseCase, Result, ScheduleActionUseCase,
    SearchUseCase, SubscribeProviderUseCase,
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
    open_view: Arc<OpenViewUseCase>,
    subscribe_provider: Arc<SubscribeProviderUseCase>,
    query_provider: Arc<QueryProviderUseCase>,
    schedule_action: Arc<ScheduleActionUseCase>,
    reload_plugins: Arc<ReloadPluginsUseCase>,
}

/// Params for the three `view.*` handlers (`view.toggle`, `view.show`,
/// `view.hide`). All three accept the same single `name` field; only the
/// `WindowMode` passed to `OpenViewUseCase::execute` differs.
#[derive(serde::Deserialize)]
struct ViewParams {
    name: String,
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
        open_view: Arc<OpenViewUseCase>,
        subscribe_provider: Arc<SubscribeProviderUseCase>,
        query_provider: Arc<QueryProviderUseCase>,
        schedule_action: Arc<ScheduleActionUseCase>,
        reload_plugins: Arc<ReloadPluginsUseCase>,
    ) -> Self {
        Self {
            search,
            launch_action,
            list_providers,
            reload_theme,
            open_view,
            subscribe_provider,
            query_provider,
            schedule_action,
            reload_plugins,
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
            "theme.reload" => self.handle_theme_reload(params).await,
            "plugin.reload" => self.handle_plugin_reload(params).await,
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

    async fn handle_theme_reload(&self, _params: Option<&RawValue>) -> Result<Value> {
        self.reload_theme.execute().await?;
        Ok(json!({}))
    }

    async fn handle_plugin_reload(&self, _params: Option<&RawValue>) -> Result<Value> {
        let loaded = self.reload_plugins.execute().await?;
        Ok(json!({ "loaded": loaded }))
    }

    async fn handle_system_status(&self, _params: Option<&RawValue>) -> Result<Value> {
        let providers = self.list_providers.execute().await;
        Ok(json!({
            "version": quantum_domain::version(),
            "providers_count": providers.len(),
            "themes_count": 1,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchResponse;
    use async_trait::async_trait;
    use quantum_domain::{
        Action, ActionOutcome, DomainError, EventBus, Match, MatchScore, ProviderId,
        ProviderRegistry, ProviderSource, Query, ThemeStore, WindowHost,
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
    }

    struct FakePluginCatalog;

    #[async_trait]
    impl quantum_domain::PluginCatalog for FakePluginCatalog {
        async fn discover(&self) -> std::result::Result<usize, DomainError> {
            Ok(0)
        }
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
        Arc::new(Dispatcher::new(
            search,
            launch_action,
            list_providers,
            reload_theme,
            open_view,
            subscribe_provider,
            query_provider,
            schedule_action,
            reload_plugins,
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
        assert_eq!(status["themes_count"], 1);
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
}
