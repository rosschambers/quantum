//! System power provider.
//!
//! Action-only provider that wraps `org.freedesktop.login1.Manager` for power
//! transitions (shutdown, restart, suspend, hibernate) and a configurable
//! lock command. Capabilities are probed at construction and exposed via
//! `subscribe()` which yields the capability snapshot once and then stays pending.

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, ProviderCapabilities, ProviderId, ProviderSource,
    Query, SystemPowerState,
};
use std::process::Stdio;

use crate::error::InfrastructureError;

mod action;
mod lock;

use action::{parse_system_power_action, SystemPowerAction};
use lock::resolve_lock_command;

/// Map a logind `Can*` reply string ("yes" | "no" | "challenge" | "na" |
/// "yes:tainted" | ...) to a bool. `"yes"`, `"challenge"`, and anything
/// starting with `"yes"` count as available.
pub(crate) fn parse_yes(s: &str) -> bool {
    s == "yes" || s == "challenge" || s.starts_with("yes:")
}

pub struct SystemPowerProvider {
    id: ProviderId,
    state: SystemPowerState,
    conn: Option<zbus::Connection>,
    lock_argv: Option<Vec<String>>,
}

impl SystemPowerProvider {
    pub async fn connect(lock_command_config: Option<String>) -> Result<Self, InfrastructureError> {
        let id = ProviderId::from("system_power");

        let lock_argv = resolve_lock_command(lock_command_config.as_deref(), |name| {
            which::which(name).ok()
        });

        let conn = zbus::Connection::system().await.ok();
        let mut state = SystemPowerState {
            can_lock: lock_argv.is_some(),
            ..Default::default()
        };

        if let Some(c) = &conn {
            // Build a manager proxy. If any Can* call fails (bus down, polkit denies)
            // treat that capability as unavailable. Never panic.
            if let Ok(manager) = zbus::Proxy::new(
                c,
                "org.freedesktop.login1",
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
            )
            .await
            {
                state.can_shutdown = manager
                    .call::<_, _, String>("CanPowerOff", &())
                    .await
                    .ok()
                    .as_deref()
                    .map(parse_yes)
                    .unwrap_or(false);
                state.can_restart = manager
                    .call::<_, _, String>("CanReboot", &())
                    .await
                    .ok()
                    .as_deref()
                    .map(parse_yes)
                    .unwrap_or(false);
                state.can_suspend = manager
                    .call::<_, _, String>("CanSuspend", &())
                    .await
                    .ok()
                    .as_deref()
                    .map(parse_yes)
                    .unwrap_or(false);
                state.can_hibernate = manager
                    .call::<_, _, String>("CanHibernate", &())
                    .await
                    .ok()
                    .as_deref()
                    .map(parse_yes)
                    .unwrap_or(false);
            }
        }

        Ok(Self {
            id,
            state,
            conn,
            lock_argv,
        })
    }

    async fn call_manager(
        &self,
        method: &str,
        capable: bool,
        cap_name: &str,
    ) -> Result<ActionOutcome, DomainError> {
        if !capable {
            return Err(DomainError::Unsupported(format!(
                "system_power: {cap_name} is false"
            )));
        }
        let conn = self
            .conn
            .as_ref()
            .ok_or_else(|| DomainError::Unsupported("system_power: no system bus".into()))?;
        let manager = zbus::Proxy::new(
            conn,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await
        .map_err(|e| DomainError::Unsupported(format!("system_power: open manager: {e}")))?;
        // `false` disables logind's interactive auth prompts — succeed via polkit
        // rules or fail fast, never hang.
        manager
            .call::<_, _, ()>(method, &(false,))
            .await
            .map_err(|e| DomainError::Unsupported(format!("system_power: {method} failed: {e}")))?;
        Ok(ActionOutcome { message: None })
    }

    async fn spawn_lock(&self) -> Result<ActionOutcome, DomainError> {
        let argv = self.lock_argv.as_ref().ok_or_else(|| {
            DomainError::Unsupported("system_power: lock command unavailable".into())
        })?;
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| DomainError::Unsupported("system_power: empty lock argv".into()))?;
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        cmd.spawn()
            .map_err(|e| DomainError::Unsupported(format!("system_power: spawn lock: {e}")))?;
        Ok(ActionOutcome { message: None })
    }
}

#[async_trait]
impl ProviderSource for SystemPowerProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            searchable: false,
            streamable: true,
        }
    }

    async fn search(&self, _: &Query) -> Result<Vec<Match>, DomainError> {
        Ok(vec![])
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        // Action-only provider: subscribe yields the capability snapshot once
        // and then stays pending. Capabilities don't change at runtime —
        // polkit rules don't reload on a running daemon.
        let v = serde_json::to_value(&self.state).unwrap_or(serde_json::Value::Null);
        let initial = futures::stream::once(async move { v });
        let pending: futures::stream::Pending<serde_json::Value> = futures::stream::pending();
        Some(Box::pin(initial.chain(pending)))
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        let payload = match action {
            Action::Custom { kind, payload } if kind == "system_power" => payload,
            _ => {
                return Err(DomainError::Unsupported(
                    "system_power: unsupported action shape".into(),
                ))
            }
        };
        let cmd = parse_system_power_action(payload)?;
        match cmd {
            SystemPowerAction::Shutdown => {
                self.call_manager("PowerOff", self.state.can_shutdown, "can_shutdown")
                    .await
            }
            SystemPowerAction::Restart => {
                self.call_manager("Reboot", self.state.can_restart, "can_restart")
                    .await
            }
            SystemPowerAction::Suspend => {
                self.call_manager("Suspend", self.state.can_suspend, "can_suspend")
                    .await
            }
            SystemPowerAction::Hibernate => {
                self.call_manager("Hibernate", self.state.can_hibernate, "can_hibernate")
                    .await
            }
            SystemPowerAction::Lock => self.spawn_lock().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_yes_known_strings() {
        assert!(parse_yes("yes"));
        assert!(parse_yes("challenge"));
        assert!(parse_yes("yes:tainted"));
        assert!(!parse_yes("no"));
        assert!(!parse_yes("na"));
        assert!(!parse_yes(""));
    }

    #[tokio::test]
    async fn invoke_unsupported_when_can_shutdown_false() {
        let p = SystemPowerProvider {
            id: ProviderId::from("system_power"),
            state: SystemPowerState::default(), // all false
            conn: None,
            lock_argv: None,
        };
        let action = Action::Custom {
            kind: "system_power".into(),
            payload: serde_json::json!({"command":"shutdown"}),
        };
        let r = p.invoke(&action).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn invoke_rejects_wrong_kind() {
        let p = SystemPowerProvider {
            id: ProviderId::from("system_power"),
            state: SystemPowerState {
                can_shutdown: true,
                ..Default::default()
            },
            conn: None,
            lock_argv: None,
        };
        // Wrong kind on the outer Custom envelope.
        let action = Action::Custom {
            kind: "other_provider".into(),
            payload: serde_json::json!({"command":"shutdown"}),
        };
        let r = p.invoke(&action).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn subscribe_yields_state_once() {
        use futures::StreamExt;
        let p = SystemPowerProvider {
            id: ProviderId::from("system_power"),
            state: SystemPowerState {
                can_lock: true,
                ..Default::default()
            },
            conn: None,
            lock_argv: None,
        };
        let mut stream = p.subscribe().expect("subscribe");
        let v = tokio::time::timeout(std::time::Duration::from_millis(50), stream.next())
            .await
            .expect("first item")
            .expect("Some");
        assert_eq!(v["can_lock"], true);
        // Second poll must time out.
        let next = tokio::time::timeout(std::time::Duration::from_millis(50), stream.next()).await;
        assert!(next.is_err());
    }

    #[tokio::test]
    #[ignore = "requires real logind on the system bus"]
    async fn connect_against_real_logind_finds_at_least_one_capability() {
        let p = SystemPowerProvider::connect(None).await.expect("connect");
        assert!(p.state.can_shutdown || p.state.can_restart || p.state.can_suspend);
    }
}
