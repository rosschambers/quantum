//! Power Profiles Daemon power profile provider.
//!
//! Subscribes to power profile state via `net.hadess.PowerProfiles` service,
//! reading active profile, available profiles, and performance inhibit status.

use async_trait::async_trait;
use futures::stream::BoxStream;
use zbus::Connection;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, PowerProfile, PowerProfileState, ProviderId,
    ProviderSource, Query,
};

use quantum_dbus::DbusError;

use crate::error::ProvidersError;

pub struct PowerProfilesDaemonProvider {
    id: ProviderId,
    conn: Option<Connection>,
}

impl PowerProfilesDaemonProvider {
    /// Attempt to connect to Power Profiles Daemon on the system bus.
    ///
    /// If the system bus is unavailable, returns `Ok(Self { conn: None })` with no
    /// error -- the provider degrades gracefully. When the bus is available but
    /// Power Profiles Daemon is not, `service_lifecycle_stream` polls availability
    /// and switches to a real subscription once the daemon appears.
    pub async fn connect() -> Result<Self, ProvidersError> {
        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(_e) => {
                return Ok(Self {
                    id: ProviderId::from("power_profile"),
                    conn: None,
                });
            }
        };

        Ok(Self {
            id: ProviderId::from("power_profile"),
            conn: Some(conn),
        })
    }
}

#[async_trait]
impl ProviderSource for PowerProfilesDaemonProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, _: &Query) -> Result<Vec<Match>, DomainError> {
        Ok(vec![])
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Custom { kind, payload } if kind == "power_profile" => {
                let profile_action = parse_power_profile_action(payload)?;
                match profile_action {
                    PowerProfileAction::Set(profile) => {
                        let conn = self.conn.as_ref().ok_or_else(|| {
                            DomainError::Unsupported(
                                "power profiles daemon not available".to_string(),
                            )
                        })?;

                        let proxy = zbus::Proxy::new(
                            conn,
                            "net.hadess.PowerProfiles",
                            "/net/hadess/PowerProfiles",
                            "net.hadess.PowerProfiles",
                        )
                        .await
                        .map_err(|e| DomainError::ActionFailed {
                            reason: format!("build proxy: {e}"),
                        })?;

                        proxy
                            .set_property("ActiveProfile", profile_to_string(profile))
                            .await
                            .map_err(|e| DomainError::ActionFailed {
                                reason: format!("set active profile: {e}"),
                            })?;

                        Ok(ActionOutcome {
                            message: Some(format!(
                                "set power profile to {}",
                                profile_to_string(profile)
                            )),
                        })
                    }
                }
            }
            _ => Err(DomainError::Unsupported(
                "power_profile provider only handles custom actions with kind='power_profile'"
                    .to_string(),
            )),
        }
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        let conn = match self.conn.as_ref() {
            Some(c) => c.clone(),
            None => {
                #[allow(deprecated)]
                return Some(quantum_dbus::common::unavailable_stream::<PowerProfileState>());
            }
        };

        Some(quantum_dbus::common::service_lifecycle_stream::<
            PowerProfileState,
            _,
        >(
            conn,
            "net.hadess.PowerProfiles",
            |conn: Connection| {
                let build = |conn: &Connection| {
                    let conn = conn.clone();
                    async move {
                        let proxy = zbus::Proxy::new(
                            &conn,
                            "net.hadess.PowerProfiles",
                            "/net/hadess/PowerProfiles",
                            "net.hadess.PowerProfiles",
                        )
                        .await
                        .map_err(|e| DbusError::Transport(e.to_string()))?;

                        let active_profile_str: String = proxy
                            .get_property("ActiveProfile")
                            .await
                            .unwrap_or_default();
                        let active = parse_profile_str(&active_profile_str);

                        // Profiles is `aa{sv}`: an array of dicts (string ->
                        // variant). Each dict has a "Profile" key whose value
                        // is the profile name string (power-saver / balanced /
                        // performance) plus driver metadata we ignore.
                        let profiles_data: Vec<
                            std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
                        > = proxy.get_property("Profiles").await.unwrap_or_default();
                        let profiles: Vec<PowerProfile> = profiles_data
                            .iter()
                            .filter_map(|dict| {
                                let val = dict.get("Profile")?;
                                let s: String = val.try_clone().ok()?.try_into().ok()?;
                                parse_profile_str(&s)
                            })
                            .collect();

                        let performance_inhibited_str: String = proxy
                            .get_property("PerformanceInhibited")
                            .await
                            .unwrap_or_default();
                        let performance_inhibited = if performance_inhibited_str.is_empty() {
                            None
                        } else {
                            Some(performance_inhibited_str)
                        };

                        Ok(PowerProfileState {
                            available: true,
                            active,
                            profiles,
                            performance_inhibited,
                        })
                    }
                };

                quantum_dbus::common::property_subscription_stream(
                    conn,
                    "net.hadess.PowerProfiles",
                    "/net/hadess/PowerProfiles",
                    "net.hadess.PowerProfiles",
                    build,
                )
            },
        ))
    }
}

/// Parse a power profile string to PowerProfile.
///
/// "power-saver" → Some(PowerSaver)
/// "balanced" → Some(Balanced)
/// "performance" → Some(Performance)
/// Anything else → None
pub(crate) fn parse_profile_str(s: &str) -> Option<PowerProfile> {
    match s {
        "power-saver" => Some(PowerProfile::PowerSaver),
        "balanced" => Some(PowerProfile::Balanced),
        "performance" => Some(PowerProfile::Performance),
        _ => None,
    }
}

/// Convert a PowerProfile to its string representation.
pub(crate) fn profile_to_string(p: PowerProfile) -> &'static str {
    match p {
        PowerProfile::PowerSaver => "power-saver",
        PowerProfile::Balanced => "balanced",
        PowerProfile::Performance => "performance",
    }
}

/// Parsed power profile action from JSON payload.
pub(crate) enum PowerProfileAction {
    Set(PowerProfile),
}

/// Parse a power profile action from a JSON payload.
///
/// Expects `{"command": "set", "profile": "balanced"}` (or "power-saver"/"performance").
pub(crate) fn parse_power_profile_action(
    payload: &serde_json::Value,
) -> Result<PowerProfileAction, DomainError> {
    let command = payload
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            DomainError::Unsupported(
                "missing or non-string command in power_profile action".to_string(),
            )
        })?;

    match command {
        "set" => {
            let profile_str = payload
                .get("profile")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    DomainError::Unsupported(
                        "missing or non-string profile for set command".to_string(),
                    )
                })?;
            let profile = parse_profile_str(profile_str).ok_or_else(|| {
                DomainError::Unsupported(format!("unknown power profile: {}", profile_str))
            })?;
            Ok(PowerProfileAction::Set(profile))
        }
        _ => Err(DomainError::Unsupported(format!(
            "unknown power_profile command: {}",
            command
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_profile_str_known() {
        assert_eq!(
            parse_profile_str("power-saver"),
            Some(PowerProfile::PowerSaver)
        );
        assert_eq!(parse_profile_str("balanced"), Some(PowerProfile::Balanced));
        assert_eq!(
            parse_profile_str("performance"),
            Some(PowerProfile::Performance)
        );
        assert_eq!(parse_profile_str("unknown"), None);
    }

    #[test]
    fn profile_to_string_roundtrips() {
        for profile in [
            PowerProfile::PowerSaver,
            PowerProfile::Balanced,
            PowerProfile::Performance,
        ] {
            let s = profile_to_string(profile);
            assert_eq!(parse_profile_str(s), Some(profile));
        }
    }

    #[test]
    fn parses_set_balanced() {
        let p = serde_json::json!({"command":"set","profile":"balanced"});
        match parse_power_profile_action(&p) {
            Ok(PowerProfileAction::Set(PowerProfile::Balanced)) => {}
            _ => panic!("expected Set(Balanced)"),
        }
    }

    #[test]
    fn parses_set_performance() {
        let p = serde_json::json!({"command":"set","profile":"performance"});
        match parse_power_profile_action(&p) {
            Ok(PowerProfileAction::Set(PowerProfile::Performance)) => {}
            _ => panic!("expected Set(Performance)"),
        }
    }

    #[test]
    fn parses_set_power_saver() {
        let p = serde_json::json!({"command":"set","profile":"power-saver"});
        match parse_power_profile_action(&p) {
            Ok(PowerProfileAction::Set(PowerProfile::PowerSaver)) => {}
            _ => panic!("expected Set(PowerSaver)"),
        }
    }

    #[test]
    fn rejects_unknown_command() {
        let p = serde_json::json!({"command":"toggle"});
        assert!(parse_power_profile_action(&p).is_err());
    }

    #[test]
    fn rejects_unknown_profile() {
        let p = serde_json::json!({"command":"set","profile":"hyper-performance"});
        assert!(parse_power_profile_action(&p).is_err());
    }

    #[test]
    fn rejects_missing_profile() {
        let p = serde_json::json!({"command":"set"});
        assert!(parse_power_profile_action(&p).is_err());
    }

    #[tokio::test]
    #[ignore = "requires real power-profiles-daemon"]
    async fn yields_initial_state_within_2s() {
        use futures::StreamExt;
        use std::time::Duration;

        let p = PowerProfilesDaemonProvider::connect()
            .await
            .expect("connect");
        let mut stream = p.subscribe().expect("subscribe");
        let v = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("first state within 2s")
            .expect("Some");
        let _state: PowerProfileState = serde_json::from_value(v).expect("PowerProfileState");
    }
}
