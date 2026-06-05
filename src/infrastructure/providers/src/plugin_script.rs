//! Plugin script provider: runtime adapter exposing per-plugin
//! `actions/` and `scripts/` invocations through `ProviderSource::invoke`.
//! Polling of `scripts/` listed in `config.toml` is wired separately in
//! `quantumd::main` because the broadcast sender lives in the binary.

use async_trait::async_trait;
use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, ProviderId, ProviderSource, Query, ShellExecutor,
};
use quantum_plugins::{ActionScript, IdleScript, PolledScript};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Routes `Action::Custom { kind: "plugin_script", payload }` invocations to
/// the appropriate idle or action script for a given plugin. Idle scripts are
/// keyed by file stem (matching the lookup contract from the plugin walker),
/// action scripts by their declared `name`. Polling of scripts is handled
/// outside this provider — Phase 4 wires polled scripts in `quantumd::main`
/// where the broadcast sender lives.
pub struct PluginScriptProvider {
    id: ProviderId,
    idle: HashMap<String, IdleScript>,
    actions: HashMap<String, ActionScript>,
    executor: Arc<dyn ShellExecutor>,
}

impl PluginScriptProvider {
    /// Build a provider for a plugin identified by `name`. The `_polled`
    /// argument is accepted for signature consistency with the future
    /// Phase 4 wiring — this provider does not poll anything itself.
    pub fn new(
        name: &str,
        _polled: Vec<PolledScript>,
        idle: Vec<IdleScript>,
        actions: Vec<ActionScript>,
        executor: Arc<dyn ShellExecutor>,
    ) -> Self {
        let idle_map = idle
            .into_iter()
            .map(|script| {
                let key = script
                    .command
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("")
                    .to_string();
                (key, script)
            })
            .collect();
        let actions_map = actions
            .into_iter()
            .map(|action| (action.name.clone(), action))
            .collect();
        Self {
            id: ProviderId::from(name.to_string()),
            idle: idle_map,
            actions: actions_map,
            executor,
        }
    }
}

#[derive(Deserialize)]
struct InvokePayload {
    name: String,
    kind: String,
}

const TIMEOUT_MS: u64 = 30_000;

#[async_trait]
impl ProviderSource for PluginScriptProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    // The provider exposes invoke-only. Polled scripts (the streaming
    // half) are driven by per-script tokio tasks in quantumd::main,
    // which publish directly to the broadcast event bus. The provider
    // never returns a Stream itself.
    async fn search(&self, _q: &Query) -> Result<Vec<Match>, DomainError> {
        Ok(Vec::new())
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        let payload = match action {
            Action::Custom { kind, payload } if kind == "plugin_script" => payload,
            Action::Custom { kind, .. } => {
                return Err(DomainError::Unsupported(format!(
                    "plugin scripts only accept Custom actions with kind 'plugin_script', got '{kind}'"
                )));
            }
            _ => {
                return Err(DomainError::Unsupported(
                    "plugin scripts only accept Custom actions".into(),
                ));
            }
        };

        let parsed: InvokePayload = serde_json::from_value(payload.clone())
            .map_err(|err| DomainError::Unsupported(format!("bad payload: {err}")))?;

        let command_path = match parsed.kind.as_str() {
            "idle" => self.idle.get(&parsed.name).map(|script| &script.command),
            "action" => self
                .actions
                .get(&parsed.name)
                .map(|action_script| &action_script.command),
            other => {
                return Err(DomainError::Unsupported(format!(
                    "unknown plugin script kind '{other}'"
                )));
            }
        }
        .ok_or_else(|| {
            DomainError::Unsupported(format!("unknown {} '{}'", parsed.kind, parsed.name))
        })?;

        let command = vec![command_path.to_string_lossy().to_string()];
        let output = self.executor.run_with_timeout(&command, TIMEOUT_MS).await?;
        Ok(ActionOutcome {
            message: Some(output.stdout),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::{Action, DomainError, Query, ShellExecutor, ShellOutput};
    use quantum_plugins::{ActionScript, IdleScript};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::Mutex;

    struct FakeExecutor {
        responses: Mutex<HashMap<String, Result<ShellOutput, DomainError>>>,
    }

    impl FakeExecutor {
        fn new(map: HashMap<String, Result<ShellOutput, DomainError>>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(map),
            })
        }
    }

    #[async_trait]
    impl ShellExecutor for FakeExecutor {
        async fn run_with_timeout(
            &self,
            command: &[String],
            _ms: u64,
        ) -> Result<ShellOutput, DomainError> {
            let key = command.join(" ");
            let mut guard = self.responses.lock().expect("lock");
            guard.remove(&key).unwrap_or_else(|| {
                Err(DomainError::Unsupported(format!(
                    "no canned response for command '{key}'"
                )))
            })
        }
        async fn spawn_detached(&self, _command: &[String]) -> Result<(), DomainError> {
            Err(DomainError::Unsupported("not used in tests".into()))
        }
    }

    fn ok(stdout: &str) -> ShellOutput {
        ShellOutput {
            stdout: stdout.to_string(),
            stderr: String::new(),
            status: 0,
        }
    }

    #[tokio::test]
    async fn invoke_idle_script_returns_stdout() {
        let cmd = "/tmp/my-plugin/scripts/foo";
        let mut map = HashMap::new();
        map.insert(cmd.to_string(), Ok(ok("idle output\n")));
        let provider = PluginScriptProvider::new(
            "my-plugin",
            vec![],
            vec![IdleScript {
                command: PathBuf::from(cmd),
                channel: "my-plugin.foo".into(),
            }],
            vec![],
            FakeExecutor::new(map),
        );

        let action = Action::Custom {
            kind: "plugin_script".into(),
            payload: serde_json::json!({ "name": "foo", "kind": "idle" }),
        };
        let outcome = provider.invoke(&action).await.expect("ok");
        assert_eq!(outcome.message.as_deref(), Some("idle output\n"));
    }

    #[tokio::test]
    async fn invoke_action_routes_by_name() {
        let cmd = "/tmp/my-plugin/actions/open";
        let mut map = HashMap::new();
        map.insert(cmd.to_string(), Ok(ok("opened\n")));
        let provider = PluginScriptProvider::new(
            "my-plugin",
            vec![],
            vec![],
            vec![ActionScript {
                name: "open".into(),
                command: PathBuf::from(cmd),
            }],
            FakeExecutor::new(map),
        );

        let action = Action::Custom {
            kind: "plugin_script".into(),
            payload: serde_json::json!({ "name": "open", "kind": "action" }),
        };
        let outcome = provider.invoke(&action).await.expect("ok");
        assert_eq!(outcome.message.as_deref(), Some("opened\n"));
    }

    #[tokio::test]
    async fn invoke_unknown_action_returns_error() {
        let provider = PluginScriptProvider::new(
            "p",
            vec![],
            vec![],
            vec![],
            FakeExecutor::new(HashMap::new()),
        );
        let action = Action::Custom {
            kind: "plugin_script".into(),
            payload: serde_json::json!({ "name": "missing", "kind": "action" }),
        };
        let err = provider.invoke(&action).await.expect_err("not found");
        assert!(
            format!("{err}").contains("missing"),
            "error mentions missing name: {err}"
        );
    }

    #[tokio::test]
    async fn search_returns_empty() {
        let provider = PluginScriptProvider::new(
            "p",
            vec![],
            vec![],
            vec![],
            FakeExecutor::new(HashMap::new()),
        );
        let q = Query::new("x");
        let result = provider.search(&q).await.expect("ok");
        assert!(result.is_empty());
    }
}
