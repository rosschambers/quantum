//! Logind brightness provider via sysfs polling.
//!
//! Enumerates `/sys/class/backlight/*` and `/sys/class/leds/*::kbd_backlight` at startup,
//! polls each brightness file at 1Hz, and emits `BrightnessState` updates. Supports
//! SetBrightness via logind session interface.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use zbus::Connection;

use quantum_domain::{
    Action, ActionOutcome, BrightnessDisplay, BrightnessState, DomainError, Match,
    ProviderCapabilities, ProviderId, ProviderSource, Query,
};

use crate::error::InfrastructureError;

pub struct LogindBrightnessProvider {
    id: ProviderId,
    conn: Option<Connection>,
    session_path: Option<zbus::zvariant::OwnedObjectPath>,
    specs: Vec<BrightnessSpec>,
    tx: broadcast::Sender<serde_json::Value>,
}

#[derive(Clone)]
pub(crate) struct BrightnessSpec {
    subsystem: String,
    name: String,
    max: u32,
    brightness_path: PathBuf,
}

impl LogindBrightnessProvider {
    /// Connect and enumerate brightness devices from sysfs.
    ///
    /// Polls `/sys/class/backlight` and `/sys/class/leds/*::kbd_backlight` to find
    /// max_brightness. Attempts logind connection for write support. Spawns a 1Hz
    /// polling task on the provided runtime.
    pub async fn connect(runtime: tokio::runtime::Handle) -> Result<Self, InfrastructureError> {
        let id = ProviderId::from("brightness");

        // Enumerate specs from standard sysfs locations.
        let specs = read_specs_from(Path::new("/sys/class"));

        // If no devices found, return unavailable.
        if specs.is_empty() {
            let (tx, _rx) = broadcast::channel(16);
            return Ok(Self {
                id,
                conn: None,
                session_path: None,
                specs,
                tx,
            });
        }

        // Try to connect to logind for write support.
        let (conn, session_path) = match Connection::system().await {
            Ok(c) => {
                let sp = match get_session_path(&c).await {
                    Ok(path) => Some(path),
                    Err(_) => None,
                };
                (Some(c), sp)
            }
            Err(_) => (None, None),
        };

        // Create broadcast channel and spawn polling task.
        let (tx, _rx) = broadcast::channel(16);
        let tx_task = tx.clone();
        let specs_task = specs.clone();

        runtime.spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            let mut last_emitted: Option<serde_json::Value> = None;

            loop {
                interval.tick().await;

                let mut displays = Vec::new();
                for spec in &specs_task {
                    match tokio::fs::read_to_string(&spec.brightness_path).await {
                        Ok(content) => {
                            if let Ok(current) = content.trim().parse::<u32>() {
                                displays.push(BrightnessDisplay {
                                    subsystem: spec.subsystem.clone(),
                                    name: spec.name.clone(),
                                    current,
                                    max: spec.max,
                                });
                            }
                        }
                        Err(_) => continue,
                    }
                }

                let state = BrightnessState {
                    available: true,
                    displays,
                };

                let value = serde_json::to_value(&state).unwrap_or(serde_json::Value::Null);

                // Dedupe: only emit if different from last.
                if last_emitted.as_ref() != Some(&value) {
                    let _ = tx_task.send(value.clone());
                    last_emitted = Some(value);
                }
            }
        });

        Ok(Self {
            id,
            conn,
            session_path,
            specs,
            tx,
        })
    }
}

#[async_trait]
impl ProviderSource for LogindBrightnessProvider {
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

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Custom { kind, payload } if kind == "brightness" => {
                let brightness_action = parse_brightness_action(payload)?;
                match brightness_action {
                    BrightnessAction::Set {
                        subsystem,
                        name,
                        value,
                    } => self.set_brightness(&subsystem, &name, value).await,
                    BrightnessAction::Adjust {
                        subsystem,
                        name,
                        delta_percent,
                    } => {
                        self.adjust_brightness(&subsystem, &name, delta_percent)
                            .await
                    }
                }
            }
            _ => Err(DomainError::Unsupported(
                "brightness provider only handles custom actions with kind='brightness'"
                    .to_string(),
            )),
        }
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        if self.specs.is_empty() {
            return Some(crate::providers::dbus_common::unavailable_stream::<
                BrightnessState,
            >());
        }

        let rx = self.tx.subscribe();
        Some(
            BroadcastStream::new(rx)
                .filter_map(|res| async move { res.ok() })
                .boxed(),
        )
    }
}

impl LogindBrightnessProvider {
    async fn set_brightness(
        &self,
        subsystem: &str,
        name: &str,
        value: u32,
    ) -> Result<ActionOutcome, DomainError> {
        let conn = self.conn.as_ref().ok_or_else(|| {
            DomainError::Unsupported("brightness writes require logind session".to_string())
        })?;
        let session_path = self.session_path.as_ref().ok_or_else(|| {
            DomainError::Unsupported("brightness writes require logind session".to_string())
        })?;

        let proxy = zbus::Proxy::new(
            conn,
            "org.freedesktop.login1",
            session_path,
            "org.freedesktop.login1.Session",
        )
        .await
        .map_err(|e| DomainError::ActionFailed {
            reason: format!("build session proxy: {e}"),
        })?;

        proxy
            .call_method("SetBrightness", &(subsystem, name, value))
            .await
            .map_err(|e| DomainError::ActionFailed {
                reason: format!("set brightness: {e}"),
            })?;

        Ok(ActionOutcome {
            message: Some(format!("set {}/{} to {}", subsystem, name, value)),
        })
    }

    async fn adjust_brightness(
        &self,
        subsystem: &str,
        name: &str,
        delta_percent: i32,
    ) -> Result<ActionOutcome, DomainError> {
        // Find the spec to get max and path.
        let spec = self
            .specs
            .iter()
            .find(|s| s.subsystem == subsystem && s.name == name)
            .ok_or_else(|| DomainError::ActionFailed {
                reason: format!("brightness device not found: {}/{}", subsystem, name),
            })?;

        // Read current brightness synchronously.
        let current_str = std::fs::read_to_string(&spec.brightness_path).map_err(|e| {
            DomainError::ActionFailed {
                reason: format!("read brightness: {e}"),
            }
        })?;

        let current = current_str
            .trim()
            .parse::<u32>()
            .map_err(|_| DomainError::ActionFailed {
                reason: "parse brightness value".to_string(),
            })?;

        let new_value = compute_adjusted(current, spec.max, delta_percent);

        self.set_brightness(subsystem, name, new_value).await
    }
}

/// Read brightness device specs from a sysfs root.
///
/// Enumerates `/backlight/*` and `/leds/*::kbd_backlight` subdirectories,
/// reading `max_brightness` from each and building a spec with the brightness path.
pub(crate) fn read_specs_from(root: &Path) -> Vec<BrightnessSpec> {
    let mut specs = Vec::new();

    // Backlight devices.
    if let Ok(entries) = std::fs::read_dir(root.join("backlight")) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();

            let max_brightness_path = path.join("max_brightness");
            if let Ok(content) = std::fs::read_to_string(&max_brightness_path) {
                if let Ok(max) = content.trim().parse::<u32>() {
                    specs.push(BrightnessSpec {
                        subsystem: "backlight".to_string(),
                        name: name_str,
                        max,
                        brightness_path: path.join("brightness"),
                    });
                }
            }
        }
    }

    // LED keyboard backlight devices (name contains "kbd_backlight").
    if let Ok(entries) = std::fs::read_dir(root.join("leds")) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();

            // Only pick up kbd_backlight entries.
            if name_str.contains("kbd_backlight") {
                let max_brightness_path = path.join("max_brightness");
                if let Ok(content) = std::fs::read_to_string(&max_brightness_path) {
                    if let Ok(max) = content.trim().parse::<u32>() {
                        specs.push(BrightnessSpec {
                            subsystem: "leds".to_string(),
                            name: name_str,
                            max,
                            brightness_path: path.join("brightness"),
                        });
                    }
                }
            }
        }
    }

    specs
}

/// Parse a brightness action from a JSON payload.
pub(crate) fn parse_brightness_action(
    payload: &serde_json::Value,
) -> Result<BrightnessAction, DomainError> {
    let command = payload
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| DomainError::ActionFailed {
            reason: "missing command field".to_string(),
        })?;

    match command {
        "set" => {
            let subsystem = payload
                .get("subsystem")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DomainError::ActionFailed {
                    reason: "set requires subsystem".to_string(),
                })?
                .to_string();

            let name = payload
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DomainError::ActionFailed {
                    reason: "set requires name".to_string(),
                })?
                .to_string();

            let value = payload
                .get("value")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| DomainError::ActionFailed {
                    reason: "set requires value (u32)".to_string(),
                })? as u32;

            Ok(BrightnessAction::Set {
                subsystem,
                name,
                value,
            })
        }
        "adjust" => {
            let subsystem = payload
                .get("subsystem")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DomainError::ActionFailed {
                    reason: "adjust requires subsystem".to_string(),
                })?
                .to_string();

            let name = payload
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DomainError::ActionFailed {
                    reason: "adjust requires name".to_string(),
                })?
                .to_string();

            let delta_percent = payload
                .get("delta_percent")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| DomainError::ActionFailed {
                    reason: "adjust requires delta_percent (i32)".to_string(),
                })? as i32;

            Ok(BrightnessAction::Adjust {
                subsystem,
                name,
                delta_percent,
            })
        }
        _ => Err(DomainError::ActionFailed {
            reason: format!("unknown brightness command: {}", command),
        }),
    }
}

/// Compute adjusted brightness with clamping.
///
/// `step = (delta_percent * max / 100)`, then clamp to [0, max].
pub(crate) fn compute_adjusted(current: u32, max: u32, delta_percent: i32) -> u32 {
    let step = delta_percent as i64 * max as i64 / 100;
    (current as i64 + step).max(0).min(max as i64) as u32
}

pub(crate) enum BrightnessAction {
    Set {
        subsystem: String,
        name: String,
        value: u32,
    },
    Adjust {
        subsystem: String,
        name: String,
        delta_percent: i32,
    },
}

/// Get the active session path via logind.
async fn get_session_path(
    conn: &Connection,
) -> Result<zbus::zvariant::OwnedObjectPath, InfrastructureError> {
    let proxy = zbus::Proxy::new(
        conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await
    .map_err(|e| InfrastructureError::DbusTransport(e.to_string()))?;

    let session_path: zbus::zvariant::OwnedObjectPath = proxy
        .call_method("GetSession", &("auto",))
        .await
        .map_err(|e| InfrastructureError::DbusTransport(e.to_string()))?
        .body()
        .deserialize()
        .map_err(|e| {
            InfrastructureError::DbusTransport(format!("deserialize session path: {e}"))
        })?;

    Ok(session_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_specs_from_empty_root() {
        let tmpdir = tempfile::TempDir::new().expect("tempdir");
        let specs = read_specs_from(tmpdir.path());
        assert!(specs.is_empty());
    }

    #[test]
    fn read_specs_from_backlight_only() {
        let tmpdir = tempfile::TempDir::new().expect("tempdir");
        let backlight_dir = tmpdir.path().join("backlight");
        std::fs::create_dir(&backlight_dir).expect("mkdir");

        let device_dir = backlight_dir.join("intel_backlight");
        std::fs::create_dir(&device_dir).expect("mkdir");

        std::fs::write(device_dir.join("max_brightness"), "96000\n").expect("write");

        let specs = read_specs_from(tmpdir.path());
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec.subsystem, "backlight");
        assert_eq!(spec.name, "intel_backlight");
        assert_eq!(spec.max, 96000);
        assert!(spec.brightness_path.ends_with("brightness"));
    }

    #[test]
    fn read_specs_from_keyboard_backlight() {
        let tmpdir = tempfile::TempDir::new().expect("tempdir");
        let leds_dir = tmpdir.path().join("leds");
        std::fs::create_dir(&leds_dir).expect("mkdir");

        let device_dir = leds_dir.join("asus::kbd_backlight");
        std::fs::create_dir(&device_dir).expect("mkdir");

        std::fs::write(device_dir.join("max_brightness"), "3\n").expect("write");

        let specs = read_specs_from(tmpdir.path());
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec.subsystem, "leds");
        assert_eq!(spec.name, "asus::kbd_backlight");
        assert_eq!(spec.max, 3);
    }

    #[test]
    fn read_specs_from_ignores_non_kbd_leds() {
        let tmpdir = tempfile::TempDir::new().expect("tempdir");
        let leds_dir = tmpdir.path().join("leds");
        std::fs::create_dir(&leds_dir).expect("mkdir");

        let device_dir = leds_dir.join("input1::capslock");
        std::fs::create_dir(&device_dir).expect("mkdir");

        std::fs::write(device_dir.join("max_brightness"), "1\n").expect("write");

        let specs = read_specs_from(tmpdir.path());
        assert!(specs.is_empty());
    }

    #[test]
    fn read_specs_from_ignores_unparseable_max() {
        let tmpdir = tempfile::TempDir::new().expect("tempdir");
        let backlight_dir = tmpdir.path().join("backlight");
        std::fs::create_dir(&backlight_dir).expect("mkdir");

        let device_dir = backlight_dir.join("test");
        std::fs::create_dir(&device_dir).expect("mkdir");

        std::fs::write(device_dir.join("max_brightness"), "garbage\n").expect("write");

        let specs = read_specs_from(tmpdir.path());
        assert!(specs.is_empty());
    }

    #[test]
    fn parses_set_action() {
        let payload = serde_json::json!({
            "command": "set",
            "subsystem": "backlight",
            "name": "intel_backlight",
            "value": 50000
        });

        let action = parse_brightness_action(&payload).expect("parse");
        match action {
            BrightnessAction::Set {
                subsystem,
                name,
                value,
            } => {
                assert_eq!(subsystem, "backlight");
                assert_eq!(name, "intel_backlight");
                assert_eq!(value, 50000);
            }
            _ => panic!("expected Set action"),
        }
    }

    #[test]
    fn parses_adjust_positive() {
        let payload = serde_json::json!({
            "command": "adjust",
            "subsystem": "backlight",
            "name": "intel_backlight",
            "delta_percent": 5
        });

        let action = parse_brightness_action(&payload).expect("parse");
        match action {
            BrightnessAction::Adjust {
                subsystem,
                name,
                delta_percent,
            } => {
                assert_eq!(subsystem, "backlight");
                assert_eq!(name, "intel_backlight");
                assert_eq!(delta_percent, 5);
            }
            _ => panic!("expected Adjust action"),
        }
    }

    #[test]
    fn parses_adjust_negative() {
        let payload = serde_json::json!({
            "command": "adjust",
            "subsystem": "backlight",
            "name": "intel_backlight",
            "delta_percent": -5
        });

        let action = parse_brightness_action(&payload).expect("parse");
        match action {
            BrightnessAction::Adjust { delta_percent, .. } => {
                assert_eq!(delta_percent, -5);
            }
            _ => panic!("expected Adjust action"),
        }
    }

    #[test]
    fn rejects_unknown_command() {
        let payload = serde_json::json!({
            "command": "invalid"
        });

        assert!(parse_brightness_action(&payload).is_err());
    }

    #[test]
    fn rejects_set_missing_subsystem() {
        let payload = serde_json::json!({
            "command": "set",
            "name": "intel_backlight",
            "value": 50000
        });

        assert!(parse_brightness_action(&payload).is_err());
    }

    #[test]
    fn rejects_adjust_non_int_delta() {
        let payload = serde_json::json!({
            "command": "adjust",
            "subsystem": "backlight",
            "name": "intel_backlight",
            "delta_percent": "not_an_int"
        });

        assert!(parse_brightness_action(&payload).is_err());
    }

    #[test]
    fn compute_adjusted_clamps_to_zero() {
        // current=100, max=10000, delta_percent=-50 → step=-5000 → 100-5000=-4900 → clamp to 0
        let result = compute_adjusted(100, 10000, -50);
        assert_eq!(result, 0);
    }

    #[test]
    fn compute_adjusted_clamps_to_max() {
        // current=9000, max=10000, delta_percent=20 → step=2000 → 9000+2000=11000 → clamp to 10000
        let result = compute_adjusted(9000, 10000, 20);
        assert_eq!(result, 10000);
    }

    #[test]
    fn compute_adjusted_typical() {
        // current=5000, max=10000, delta_percent=5 → step=500 → 5000+500=5500
        let result = compute_adjusted(5000, 10000, 5);
        assert_eq!(result, 5500);
    }

    #[tokio::test]
    #[ignore = "requires real logind"]
    async fn yields_initial_state_within_2s() {
        use futures::StreamExt;
        use std::time::Duration;

        let p = LogindBrightnessProvider::connect(tokio::runtime::Handle::current())
            .await
            .expect("connect");
        if p.specs.is_empty() {
            // Skip test if no devices found.
            return;
        }
        let mut s = p.subscribe().expect("subscribe");
        let v = tokio::time::timeout(Duration::from_secs(2), s.next())
            .await
            .expect("first state within 2s")
            .expect("Some");
        let _state: BrightnessState = serde_json::from_value(v).expect("BrightnessState");
    }
}
