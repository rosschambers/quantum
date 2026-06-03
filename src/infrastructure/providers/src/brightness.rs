//! Logind brightness provider via sysfs polling.
//!
//! Enumerates `/sys/class/backlight/*` and `/sys/class/leds/*::kbd_backlight` at startup,
//! polls each brightness file at 1Hz, and emits `BrightnessState` updates. Supports
//! SetBrightness via logind session interface.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use tokio::sync::watch;
use tokio_stream::wrappers::WatchStream;
use zbus::Connection;

use quantum_domain::{
    Action, ActionOutcome, BrightnessDisplay, BrightnessState, DomainError, Match,
    ProviderCapabilities, ProviderId, ProviderSource, Query,
};

use crate::error::ProvidersError;

pub struct LogindBrightnessProvider {
    id: ProviderId,
    conn: Option<Connection>,
    session_path: Option<zbus::zvariant::OwnedObjectPath>,
    specs: Vec<BrightnessSpec>,
    state_rx: watch::Receiver<serde_json::Value>,
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
    pub async fn connect(runtime: tokio::runtime::Handle) -> Result<Self, ProvidersError> {
        let id = ProviderId::from("brightness");

        // Enumerate specs from standard sysfs locations.
        let specs = read_specs_from(Path::new("/sys/class"));

        // If no devices found, return unavailable.
        if specs.is_empty() {
            let (_tx, state_rx) = watch::channel(serde_json::Value::Null);
            return Ok(Self {
                id,
                conn: None,
                session_path: None,
                specs,
                state_rx,
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

        // Read an initial sample synchronously so the watch holds real state
        // immediately. Subscribers attached AFTER startup still see this
        // initial value because watch retains the latest sent payload — no
        // race between pre-subscribe and frontend connection.
        let initial_state = sample_brightness(&specs).await;
        let initial_value = serde_json::to_value(&initial_state).unwrap_or(serde_json::Value::Null);
        let (state_tx, state_rx) = watch::channel(initial_value);

        let specs_task = specs.clone();
        runtime.spawn(async move {
            // 5s is far slower than the human response time for a brightness
            // keypress, but the brightness value only changes on a keypress or
            // an explicit `SetBrightness` action — so 5s of latency on a value
            // that was already updated synchronously by the writer is a
            // non-issue, and saves four wakeups per second on idle.
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            // Skip the immediate first tick — we already sampled before spawn.
            interval.tick().await;
            loop {
                interval.tick().await;
                let state = sample_brightness(&specs_task).await;
                let value = serde_json::to_value(&state).unwrap_or(serde_json::Value::Null);
                // watch::send_if_modified only fires on change, deduping by
                // serde_json::Value equality. No subscribers? Fine, watch
                // retains the latest value for future receivers.
                state_tx.send_if_modified(|current| {
                    if *current != value {
                        *current = value.clone();
                        true
                    } else {
                        false
                    }
                });
            }
        });

        Ok(Self {
            id,
            conn,
            session_path,
            specs,
            state_rx,
        })
    }
}

async fn sample_brightness(specs: &[BrightnessSpec]) -> BrightnessState {
    let mut displays = Vec::new();
    for spec in specs {
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
    BrightnessState {
        available: true,
        displays,
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
            return Some(quantum_dbus::common::unavailable_stream::<BrightnessState>());
        }
        // WatchStream yields the current value immediately on subscribe,
        // then every time the watch sender mutates it. Late subscribers
        // catch up automatically.
        Some(WatchStream::new(self.state_rx.clone()).boxed())
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

        // Floor the requested value so we never drive a backlight to
        // zero. Look up the device's max to derive the floor; if the
        // device is unknown, fall through with the raw value (it will
        // fail downstream).
        let value = if let Some(spec) = self
            .specs
            .iter()
            .find(|s| s.subsystem == subsystem && s.name == name)
        {
            value.max(min_brightness(spec.max)).min(spec.max)
        } else {
            value
        };

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
/// `step = (delta_percent * max / 100)`, then clamp to
/// `[min_brightness(max), max]`. The lower bound is the higher of 1
/// raw unit or 1% of max so the screen never goes fully dark by
/// accident — backlights at 0 are essentially "off" and you can't
/// see anything to dial them back up.
pub(crate) fn compute_adjusted(current: u32, max: u32, delta_percent: i32) -> u32 {
    let step = delta_percent as i64 * max as i64 / 100;
    let target = (current as i64 + step).clamp(0, max as i64) as u32;
    target.max(min_brightness(max))
}

/// Minimum allowed brightness for a device whose `max_brightness` is
/// `max`. Floors at 1% of max but never below 1 raw unit so devices
/// with very small max values (e.g. keyboard backlights at max=3)
/// still get a usable lower bound.
pub(crate) fn min_brightness(max: u32) -> u32 {
    let one_percent = max / 100;
    one_percent.max(1)
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
) -> Result<zbus::zvariant::OwnedObjectPath, ProvidersError> {
    let proxy = zbus::Proxy::new(
        conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await
    .map_err(|e| ProvidersError::DbusTransport(e.to_string()))?;

    let session_path: zbus::zvariant::OwnedObjectPath = proxy
        .call_method("GetSession", &("auto",))
        .await
        .map_err(|e| ProvidersError::DbusTransport(e.to_string()))?
        .body()
        .deserialize()
        .map_err(|e| ProvidersError::DbusTransport(format!("deserialize session path: {e}")))?;

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
    fn compute_adjusted_clamps_to_minimum_floor() {
        // current=100, max=10000, delta_percent=-50 → step=-5000 →
        // 100-5000=-4900 → clamped first to 0 but then floored at
        // min_brightness(10000) = 100 (1% of max).
        let result = compute_adjusted(100, 10000, -50);
        assert_eq!(result, 100);
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

    #[test]
    fn compute_adjusted_floors_at_one_raw_unit_for_tiny_max() {
        // Keyboard backlight with max=3. 1% of 3 = 0, so min_brightness
        // returns 1. Driving it to "off" via a delta therefore lands
        // at 1, not 0.
        let result = compute_adjusted(2, 3, -100);
        assert_eq!(result, 1);
    }

    #[test]
    fn min_brightness_one_percent_of_typical_panel() {
        assert_eq!(min_brightness(10000), 100);
        assert_eq!(min_brightness(96000), 960);
    }

    #[test]
    fn min_brightness_never_zero() {
        // Tiny / unusual max values still produce a floor of at least 1.
        assert_eq!(min_brightness(3), 1);
        assert_eq!(min_brightness(50), 1);
        assert_eq!(min_brightness(1), 1);
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
