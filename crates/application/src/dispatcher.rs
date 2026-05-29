use crate::{
    ApplicationError, LaunchActionUseCase, ListProvidersUseCase, OpenViewUseCase,
    QueryProviderUseCase, ReloadThemeUseCase, Result, SearchUseCase, SubscribeProviderUseCase,
};
use quantum_domain::{DomainError, WindowMode};
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
}

impl Dispatcher {
    pub fn new(
        search: Arc<SearchUseCase>,
        launch_action: Arc<LaunchActionUseCase>,
        list_providers: Arc<ListProvidersUseCase>,
        reload_theme: Arc<ReloadThemeUseCase>,
        open_view: Arc<OpenViewUseCase>,
        subscribe_provider: Arc<SubscribeProviderUseCase>,
        query_provider: Arc<QueryProviderUseCase>,
    ) -> Self {
        Self {
            search,
            launch_action,
            list_providers,
            reload_theme,
            open_view,
            subscribe_provider,
            query_provider,
        }
    }

    pub async fn dispatch(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "search" => self.handle_search(params).await,
            "action.invoke" => self.handle_action_invoke(params).await,
            "provider.list" => self.handle_provider_list(params).await,
            "provider.subscribe" => self.handle_provider_subscribe(params).await,
            "provider.query" => self.handle_provider_query(params).await,
            "view.toggle" => self.handle_view_toggle(params).await,
            "view.show" => self.handle_view_show(params).await,
            "view.hide" => self.handle_view_hide(params).await,
            "theme.reload" => self.handle_theme_reload(params).await,
            "system.status" => self.handle_system_status(params).await,
            _ => Err(ApplicationError::Domain(DomainError::Unsupported(
                method.to_string(),
            ))),
        }
    }

    async fn handle_search(&self, params: Value) -> Result<Value> {
        let query: quantum_domain::Query = serde_json::from_value(params)
            .map_err(|e| ApplicationError::Unknown(format!("invalid query params: {}", e)))?;

        let response = self.search.execute(query).await?;
        serde_json::to_value(response)
            .map_err(|e| ApplicationError::Unknown(format!("serialization error: {}", e)))
    }

    async fn handle_action_invoke(&self, params: Value) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct ActionInvokeParams {
            provider: String,
            action: quantum_domain::Action,
        }

        let params: ActionInvokeParams = serde_json::from_value(params).map_err(|e| {
            ApplicationError::Unknown(format!("invalid action invoke params: {}", e))
        })?;

        self.launch_action
            .execute(params.provider.into(), params.action)
            .await?;

        Ok(json!({}))
    }

    async fn handle_provider_list(&self, _params: Value) -> Result<Value> {
        let providers = self.list_providers.execute().await;
        let provider_strs: Vec<String> = providers.iter().map(|p| p.to_string()).collect();
        Ok(json!(provider_strs))
    }

    async fn handle_view_toggle(&self, params: Value) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct ViewParams {
            name: String,
        }

        let params: ViewParams = serde_json::from_value(params)
            .map_err(|e| ApplicationError::Unknown(format!("invalid view params: {}", e)))?;

        self.open_view
            .execute(params.name, WindowMode::Toggle)
            .await?;

        Ok(json!({}))
    }

    async fn handle_view_show(&self, params: Value) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct ViewParams {
            name: String,
        }

        let params: ViewParams = serde_json::from_value(params)
            .map_err(|e| ApplicationError::Unknown(format!("invalid view params: {}", e)))?;

        self.open_view
            .execute(params.name, WindowMode::Show)
            .await?;

        Ok(json!({}))
    }

    async fn handle_view_hide(&self, params: Value) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct ViewParams {
            name: String,
        }

        let params: ViewParams = serde_json::from_value(params)
            .map_err(|e| ApplicationError::Unknown(format!("invalid view params: {}", e)))?;

        self.open_view
            .execute(params.name, WindowMode::Hide)
            .await?;

        Ok(json!({}))
    }

    async fn handle_theme_reload(&self, _params: Value) -> Result<Value> {
        self.reload_theme.execute().await?;
        Ok(json!({}))
    }

    async fn handle_system_status(&self, _params: Value) -> Result<Value> {
        let providers = self.list_providers.execute().await;
        Ok(json!({
            "version": quantum_domain::version(),
            "providers_count": providers.len(),
            "themes_count": 1,
        }))
    }

    async fn handle_provider_subscribe(&self, params: Value) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct SubscribeParams {
            provider: String,
        }

        let params: SubscribeParams = serde_json::from_value(params)
            .map_err(|e| ApplicationError::Unknown(format!("invalid subscribe params: {}", e)))?;
        self.subscribe_provider
            .execute(params.provider.into())
            .await?;
        Ok(json!({}))
    }

    async fn handle_provider_query(&self, params: Value) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct QueryParams {
            id: String,
        }
        let params: QueryParams = serde_json::from_value(params)
            .map_err(|e| ApplicationError::Unknown(format!("invalid query params: {}", e)))?;
        self.query_provider.execute(params.id.into()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchResponse;
    use async_trait::async_trait;
    use quantum_domain::{
        Action, ActionOutcome, DomainError, EventBus, Match, MatchScore, ProviderCapabilities,
        ProviderId, ProviderRegistry, ProviderSource, Query, ThemeStore, WindowHost,
    };
    use std::collections::HashMap;

    struct FakeProvider {
        id: ProviderId,
    }

    #[async_trait]
    impl ProviderSource for FakeProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                searchable: true,
                streamable: false,
            }
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

        async fn subscribe(&self, _event: &str) -> std::result::Result<(), DomainError> {
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
    }

    fn build_dispatcher() -> Dispatcher {
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

        Dispatcher::new(
            search,
            launch_action,
            list_providers,
            reload_theme,
            open_view,
            subscribe_provider,
            query_provider,
        )
    }

    #[tokio::test]
    async fn dispatches_search() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher
            .dispatch("search", json!({ "text": "test", "providers": ["apps"] }))
            .await
            .unwrap();

        let parsed: SearchResponse = serde_json::from_value(resp).unwrap();
        assert_eq!(parsed.matches.len(), 1);
        assert_eq!(parsed.matches[0].title, "Test Match");
    }

    #[tokio::test]
    async fn dispatches_action_invoke() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher
            .dispatch(
                "action.invoke",
                json!({
                    "provider": "apps",
                    "action": {
                        "kind": "launch",
                        "data": { "desktop_id": "firefox" }
                    }
                }),
            )
            .await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_provider_list() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher
            .dispatch("provider.list", json!({}))
            .await
            .unwrap();

        let providers: Vec<String> = serde_json::from_value(resp).unwrap();
        assert_eq!(providers.len(), 1);
        assert!(providers.contains(&"apps".to_string()));
    }

    #[tokio::test]
    async fn dispatches_provider_subscribe() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher
            .dispatch("provider.subscribe", json!({ "provider": "apps" }))
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
        let resp = dispatcher
            .dispatch("view.toggle", json!({ "name": "launcher" }))
            .await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_view_show() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher
            .dispatch("view.show", json!({ "name": "launcher" }))
            .await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_view_hide() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher
            .dispatch("view.hide", json!({ "name": "launcher" }))
            .await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_theme_reload() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher.dispatch("theme.reload", json!({})).await;

        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn dispatches_system_status() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher
            .dispatch("system.status", json!({}))
            .await
            .unwrap();

        let status: serde_json::Value = resp;
        assert!(status.get("version").is_some());
        assert_eq!(status["providers_count"], 1);
        assert_eq!(status["themes_count"], 1);
    }

    #[tokio::test]
    async fn returns_unsupported_for_unknown_method() {
        let dispatcher = build_dispatcher();
        let resp = dispatcher.dispatch("unknown.method", json!({})).await;

        assert!(matches!(
            resp,
            Err(ApplicationError::Domain(DomainError::Unsupported(_)))
        ));
    }
}
