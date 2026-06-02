use quantum_domain::DomainError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SystemPowerAction {
    Shutdown,
    Restart,
    Suspend,
    Hibernate,
    Lock,
}

pub(crate) fn parse_system_power_action(
    payload: &serde_json::Value,
) -> Result<SystemPowerAction, DomainError> {
    let command = payload
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| DomainError::Unsupported("missing or non-string command".into()))?;
    match command {
        "shutdown" => Ok(SystemPowerAction::Shutdown),
        "restart" => Ok(SystemPowerAction::Restart),
        "suspend" => Ok(SystemPowerAction::Suspend),
        "hibernate" => Ok(SystemPowerAction::Hibernate),
        "lock" => Ok(SystemPowerAction::Lock),
        other => Err(DomainError::Unsupported(format!(
            "unknown system_power command: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_shutdown() {
        assert_eq!(
            parse_system_power_action(&json!({"command":"shutdown"})).unwrap(),
            SystemPowerAction::Shutdown
        );
    }

    #[test]
    fn parses_restart() {
        assert_eq!(
            parse_system_power_action(&json!({"command":"restart"})).unwrap(),
            SystemPowerAction::Restart
        );
    }

    #[test]
    fn parses_suspend() {
        assert_eq!(
            parse_system_power_action(&json!({"command":"suspend"})).unwrap(),
            SystemPowerAction::Suspend
        );
    }

    #[test]
    fn parses_hibernate() {
        assert_eq!(
            parse_system_power_action(&json!({"command":"hibernate"})).unwrap(),
            SystemPowerAction::Hibernate
        );
    }

    #[test]
    fn parses_lock() {
        assert_eq!(
            parse_system_power_action(&json!({"command":"lock"})).unwrap(),
            SystemPowerAction::Lock
        );
    }

    #[test]
    fn rejects_unknown() {
        assert!(parse_system_power_action(&json!({"command":"explode"})).is_err());
    }

    #[test]
    fn rejects_missing() {
        assert!(parse_system_power_action(&json!({})).is_err());
    }

    #[test]
    fn rejects_non_string_command() {
        assert!(parse_system_power_action(&json!({"command": 42})).is_err());
    }
}
