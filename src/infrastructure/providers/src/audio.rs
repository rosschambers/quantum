// Note: Using option B (pactl polling) instead of direct libpulse-binding integration.
// The libpulse-binding + threaded mainloop approach requires careful synchronization
// between the pulse thread event loop and tokio runtime, plus integration with
// condvars and timer callbacks. Option B's simplicity and reliability (shell commands
// with explicit error handling) is more pragmatic for our time budget and maintenance
// surface. Polling once per second is enough for a tray indicator and easy on
// battery; combined with change-gating in `polling_stream` it produces zero IPC
// traffic when the volume and mute state are steady.

use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, Notify};

use quantum_domain::{
    Action, ActionOutcome, AudioCard, AudioCardProfile, AudioDevice, AudioSink, AudioState,
    AudioStream, DomainError, Match, ProviderId, ProviderSource, Query,
};

use crate::error::ProvidersError;

/// PulseAudio/PipeWire provider using `pactl` shell commands.
pub struct PulseAudioProvider {
    id: ProviderId,
    available: bool,
    session: Arc<AudioSession>,
}

/// Shared session state for the audio provider (wifi ScanSession pattern).
///
/// The sound window drives stream visibility explicitly: `open_session`
/// flips `active` on so state fetches include sink-inputs/source-outputs and
/// the subscribe loop honors their events; `close_session` flips it off.
/// `notify` lets any write command wake the streaming task immediately so
/// the next emitted `AudioState` reflects the change without waiting for a
/// pactl event. `last` caches the most recently emitted state for
/// change-gating and for `snapshot()`.
pub(crate) struct AudioSession {
    pub(crate) active: AtomicBool,
    pub(crate) notify: Notify,
    pub(crate) last: Mutex<Option<AudioState>>,
}

impl Default for AudioSession {
    fn default() -> Self {
        Self {
            active: AtomicBool::new(false),
            notify: Notify::new(),
            last: Mutex::new(None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AudioDeviceKind {
    Sink,
    Source,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AudioStreamKind {
    Playback,
    Record,
}

#[derive(Debug, Clone)]
pub(crate) enum AudioCommand {
    SetVolume(u8),
    ToggleMute,
    AdjustVolume(i32),
    ToggleMicMute,
    OpenSession,
    CloseSession,
    SetDefaultSink {
        name: String,
    },
    SetDefaultSource {
        name: String,
    },
    SetDeviceVolume {
        kind: AudioDeviceKind,
        name: String,
        percent: u8,
    },
    SetDeviceMute {
        kind: AudioDeviceKind,
        name: String,
        muted: bool,
    },
    SetStreamVolume {
        kind: AudioStreamKind,
        index: u32,
        percent: u8,
    },
    SetStreamMute {
        kind: AudioStreamKind,
        index: u32,
        muted: bool,
    },
    MoveStream {
        kind: AudioStreamKind,
        index: u32,
        device_name: String,
    },
    SetCardProfile {
        card_index: u32,
        profile: String,
    },
}

impl PulseAudioProvider {
    /// Attempt to connect to PulseAudio via `pactl`.
    ///
    /// Unavailable (with a logged warning) when the binary is missing OR its
    /// `--format=json` output is unsupported/unparseable — the JSON state
    /// path is now load-bearing, so a pactl too old for it cannot serve
    /// real state.
    pub async fn connect(_runtime: tokio::runtime::Handle) -> Result<Self, ProvidersError> {
        let available = which::which("pactl").is_ok() && probe_pactl_json_support().await;
        Ok(Self {
            id: ProviderId::from("audio"),
            available,
            session: Arc::new(AudioSession::default()),
        })
    }

    /// Execute a parsed audio command. Session commands flip shared flags;
    /// table-mapped commands run their exact pactl argv; the legacy four keep
    /// their default-sink-resolving path. Every state-changing command
    /// notifies the streaming task so the window sees the effect quickly.
    async fn execute(&self, command: AudioCommand) -> Result<ActionOutcome, DomainError> {
        match &command {
            AudioCommand::OpenSession => {
                self.session.active.store(true, Ordering::Relaxed);
                self.session.notify.notify_one();
                return Ok(ActionOutcome { message: None });
            }
            AudioCommand::CloseSession => {
                self.session.active.store(false, Ordering::Relaxed);
                // Do NOT clear `session.last`: it is the cache `snapshot()`
                // serves to `provider.query`; clearing it would leave a
                // re-opened window querying an empty snapshot.
                self.session.notify.notify_one();
                return Ok(ActionOutcome { message: None });
            }
            _ => {}
        }
        if let Some(arguments) = pactl_arguments(&command) {
            let argument_slices: Vec<&str> = arguments.iter().map(String::as_str).collect();
            run_pactl(&argument_slices).await.map_err(map_pactl_error)?;
        } else {
            execute_audio_command(&command)
                .await
                .map_err(|error| DomainError::Unsupported(error.to_string()))?;
        }
        self.session.notify.notify_one();
        Ok(ActionOutcome { message: None })
    }

    /// Event-driven stream backed by a long-lived `pactl subscribe`
    /// subprocess, session-aware and wakeable by write commands.
    ///
    /// Behaviour:
    /// - Emit the current state once on startup so late subscribers see real
    ///   data, caching it in `session.last` for `snapshot()`.
    /// - Select on subscribe lines AND `session.notify`. A line refreshes
    ///   when `should_refresh_for_pactl_line(line, session_active)` says so;
    ///   a notify always refreshes (an action or session flip just happened).
    /// - Change-gate emissions against `session.last`.
    /// - If the child exits (PulseAudio restart), sleep 1s and respawn.
    fn event_driven_stream(&self) -> BoxStream<'static, serde_json::Value> {
        let session = self.session.clone();
        Box::pin(async_stream::stream! {
            if let Some(initial) = fetch_audio_state(session.active.load(Ordering::Relaxed)).await {
                {
                    let mut last = session.last.lock().await;
                    *last = Some(initial.clone());
                }
                yield serde_json::to_value(&initial).unwrap_or(serde_json::Value::Null);
            }

            loop {
                let mut child = match Command::new("pactl")
                    .arg("subscribe")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .stdin(Stdio::null())
                    .spawn()
                {
                    Ok(child) => child,
                    Err(error) => {
                        tracing::warn!("pactl subscribe spawn failed: {error}; retry in 1s");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                };

                let stdout = match child.stdout.take() {
                    Some(stdout) => stdout,
                    None => {
                        tracing::warn!("pactl subscribe has no stdout pipe; retry in 1s");
                        let _ = child.kill().await;
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                };

                let mut lines = BufReader::new(stdout).lines();
                loop {
                    let should_fetch = tokio::select! {
                        line = lines.next_line() => match line {
                            Ok(Some(line)) => should_refresh_for_pactl_line(
                                &line,
                                session.active.load(Ordering::Relaxed),
                            ),
                            Ok(None) => break,
                            Err(error) => {
                                tracing::warn!("pactl subscribe read error: {error}; respawning");
                                break;
                            }
                        },
                        _ = session.notify.notified() => true,
                    };
                    if !should_fetch {
                        continue;
                    }
                    let session_active = session.active.load(Ordering::Relaxed);
                    if let Some(new_state) = fetch_audio_state(session_active).await {
                        let mut last = session.last.lock().await;
                        if last.as_ref() != Some(&new_state) {
                            *last = Some(new_state.clone());
                            drop(last);
                            yield serde_json::to_value(&new_state)
                                .unwrap_or(serde_json::Value::Null);
                        }
                    }
                }

                let _ = child.wait().await;
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        })
    }
}

#[async_trait]
impl ProviderSource for PulseAudioProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, _: &Query) -> Result<Vec<Match>, DomainError> {
        Ok(vec![])
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        if !self.available {
            return Err(DomainError::Unsupported(
                "audio provider unavailable".to_string(),
            ));
        }

        match action {
            Action::Custom { kind, payload } => {
                if kind != "audio" {
                    return Err(DomainError::Unsupported(format!(
                        "unknown custom action kind: {}",
                        kind
                    )));
                }
                let command = parse_audio_action(payload)?;
                self.execute(command).await
            }
            _ => Err(DomainError::Unsupported(
                "audio provider only supports custom actions".to_string(),
            )),
        }
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        // Audio uses `pactl subscribe` rather than a DBus property
        // subscription, so the DBus-shaped `service_lifecycle_stream`
        // helper does not apply. If pactl is missing entirely we fall
        // back to the legacy default-then-pending stream; recovery on
        // late install would require restarting the daemon.
        if !self.available {
            #[allow(deprecated)]
            return Some(quantum_dbus::common::unavailable_stream::<AudioState>());
        }

        Some(self.event_driven_stream())
    }

    /// Serve the last state the streaming task cached so `provider.query`
    /// resolves instantly (the stream is pre-subscribed at daemon startup,
    /// so the cache is warm by the time the window opens). None before the
    /// first emission; the caller falls back to the stream.
    async fn snapshot(&self) -> Option<serde_json::Value> {
        let last = self.session.last.lock().await;
        last.as_ref()
            .map(|state| serde_json::to_value(state).unwrap_or(serde_json::Value::Null))
    }
}

/// Read a required string field, erroring with Unsupported when absent or
/// not a string.
fn required_string(payload: &serde_json::Value, key: &str) -> Result<String, DomainError> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| {
            DomainError::Unsupported(format!("missing or non-string '{key}' in audio action"))
        })
}

/// Read a required boolean field.
fn required_bool(payload: &serde_json::Value, key: &str) -> Result<bool, DomainError> {
    payload
        .get(key)
        .and_then(|value| value.as_bool())
        .ok_or_else(|| {
            DomainError::Unsupported(format!("missing or non-bool '{key}' in audio action"))
        })
}

/// Read a required unsigned integer field that must fit in u32 (pactl
/// object indexes).
fn required_u32(payload: &serde_json::Value, key: &str) -> Result<u32, DomainError> {
    payload
        .get(key)
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            DomainError::Unsupported(format!("missing or non-u32 '{key}' in audio action"))
        })
}

/// Read the required percent field, clamped to pactl's practical 0..=150
/// range BEFORE the u8 cast so oversized values clamp instead of truncating.
fn required_percent(payload: &serde_json::Value) -> Result<u8, DomainError> {
    let percent = payload
        .get("percent")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| {
            DomainError::Unsupported("missing or non-u64 'percent' in audio action".to_string())
        })?;
    Ok(percent.min(150) as u8)
}

/// Parse the device kind discriminator: "sink" or "source".
fn parse_device_kind(payload: &serde_json::Value) -> Result<AudioDeviceKind, DomainError> {
    match required_string(payload, "kind")?.as_str() {
        "sink" => Ok(AudioDeviceKind::Sink),
        "source" => Ok(AudioDeviceKind::Source),
        other => Err(DomainError::Unsupported(format!(
            "unknown device kind: {other}"
        ))),
    }
}

/// Parse the stream kind discriminator: "playback" or "record".
fn parse_stream_kind(payload: &serde_json::Value) -> Result<AudioStreamKind, DomainError> {
    match required_string(payload, "kind")?.as_str() {
        "playback" => Ok(AudioStreamKind::Playback),
        "record" => Ok(AudioStreamKind::Record),
        other => Err(DomainError::Unsupported(format!(
            "unknown stream kind: {other}"
        ))),
    }
}

/// Parse an audio action from a payload.
pub(crate) fn parse_audio_action(payload: &serde_json::Value) -> Result<AudioCommand, DomainError> {
    let command = payload
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| DomainError::Unsupported("missing 'command' field".to_string()))?;

    match command {
        "set_volume" => {
            let percent = payload
                .get("percent")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| {
                    DomainError::Unsupported("'set_volume' requires 'percent' (u64)".to_string())
                })?;
            Ok(AudioCommand::SetVolume((percent as u8).min(150)))
        }
        "toggle_mute" => Ok(AudioCommand::ToggleMute),
        "toggle_mic_mute" => Ok(AudioCommand::ToggleMicMute),
        "adjust_volume" => {
            let delta = payload
                .get("delta")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| {
                    DomainError::Unsupported("'adjust_volume' requires 'delta' (i64)".to_string())
                })?;
            Ok(AudioCommand::AdjustVolume(delta as i32))
        }
        "open_session" => Ok(AudioCommand::OpenSession),
        "close_session" => Ok(AudioCommand::CloseSession),
        "set_default_sink" => Ok(AudioCommand::SetDefaultSink {
            name: required_string(payload, "name")?,
        }),
        "set_default_source" => Ok(AudioCommand::SetDefaultSource {
            name: required_string(payload, "name")?,
        }),
        "set_device_volume" => Ok(AudioCommand::SetDeviceVolume {
            kind: parse_device_kind(payload)?,
            name: required_string(payload, "name")?,
            percent: required_percent(payload)?,
        }),
        "set_device_mute" => Ok(AudioCommand::SetDeviceMute {
            kind: parse_device_kind(payload)?,
            name: required_string(payload, "name")?,
            muted: required_bool(payload, "muted")?,
        }),
        "set_stream_volume" => Ok(AudioCommand::SetStreamVolume {
            kind: parse_stream_kind(payload)?,
            index: required_u32(payload, "index")?,
            percent: required_percent(payload)?,
        }),
        "set_stream_mute" => Ok(AudioCommand::SetStreamMute {
            kind: parse_stream_kind(payload)?,
            index: required_u32(payload, "index")?,
            muted: required_bool(payload, "muted")?,
        }),
        "move_stream" => Ok(AudioCommand::MoveStream {
            kind: parse_stream_kind(payload)?,
            index: required_u32(payload, "index")?,
            device_name: required_string(payload, "device_name")?,
        }),
        "set_card_profile" => Ok(AudioCommand::SetCardProfile {
            card_index: required_u32(payload, "card_index")?,
            profile: required_string(payload, "profile")?,
        }),
        _ => Err(DomainError::Unsupported(format!(
            "unknown audio command: {}",
            command
        ))),
    }
}

/// Map a parsed command to its exact pactl argv (the design's action table).
/// Returns None for session commands (state flips handled by the provider)
/// and for the four legacy commands, which keep their default-sink-resolving
/// execution path in `execute_audio_command`.
pub(crate) fn pactl_arguments(command: &AudioCommand) -> Option<Vec<String>> {
    let mute_flag = |muted: bool| if muted { "1" } else { "0" };
    match command {
        AudioCommand::SetDefaultSink { name } => {
            Some(vec!["set-default-sink".to_string(), name.clone()])
        }
        AudioCommand::SetDefaultSource { name } => {
            Some(vec!["set-default-source".to_string(), name.clone()])
        }
        AudioCommand::SetDeviceVolume {
            kind,
            name,
            percent,
        } => Some(vec![
            match kind {
                AudioDeviceKind::Sink => "set-sink-volume",
                AudioDeviceKind::Source => "set-source-volume",
            }
            .to_string(),
            name.clone(),
            format!("{}%", percent),
        ]),
        AudioCommand::SetDeviceMute { kind, name, muted } => Some(vec![
            match kind {
                AudioDeviceKind::Sink => "set-sink-mute",
                AudioDeviceKind::Source => "set-source-mute",
            }
            .to_string(),
            name.clone(),
            mute_flag(*muted).to_string(),
        ]),
        AudioCommand::SetStreamVolume {
            kind,
            index,
            percent,
        } => Some(vec![
            match kind {
                AudioStreamKind::Playback => "set-sink-input-volume",
                AudioStreamKind::Record => "set-source-output-volume",
            }
            .to_string(),
            index.to_string(),
            format!("{}%", percent),
        ]),
        AudioCommand::SetStreamMute { kind, index, muted } => Some(vec![
            match kind {
                AudioStreamKind::Playback => "set-sink-input-mute",
                AudioStreamKind::Record => "set-source-output-mute",
            }
            .to_string(),
            index.to_string(),
            mute_flag(*muted).to_string(),
        ]),
        AudioCommand::MoveStream {
            kind,
            index,
            device_name,
        } => Some(vec![
            match kind {
                AudioStreamKind::Playback => "move-sink-input",
                AudioStreamKind::Record => "move-source-output",
            }
            .to_string(),
            index.to_string(),
            device_name.clone(),
        ]),
        AudioCommand::SetCardProfile {
            card_index,
            profile,
        } => Some(vec![
            "set-card-profile".to_string(),
            card_index.to_string(),
            profile.clone(),
        ]),
        AudioCommand::SetVolume(_)
        | AudioCommand::ToggleMute
        | AudioCommand::AdjustVolume(_)
        | AudioCommand::ToggleMicMute
        | AudioCommand::OpenSession
        | AudioCommand::CloseSession => None,
    }
}

/// One channel's volume in pactl's JSON output. `value_percent` is a string
/// like "55%".
#[derive(Debug, serde::Deserialize)]
pub(crate) struct PactlChannelVolume {
    pub value_percent: String,
}

/// One entry of a device's `ports` array.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct PactlPort {
    pub name: String,
    pub description: String,
}

/// One sink or source from `pactl --format=json list sinks|sources`.
/// Unknown fields (state, driver, latency, and so on) are ignored.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct PactlDevice {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub mute: bool,
    #[serde(default)]
    pub volume: BTreeMap<String, PactlChannelVolume>,
    #[serde(default)]
    pub active_port: Option<String>,
    #[serde(default)]
    pub ports: Vec<PactlPort>,
    #[serde(default)]
    pub properties: BTreeMap<String, serde_json::Value>,
}

/// One sink-input or source-output from
/// `pactl --format=json list sink-inputs|source-outputs`. Sink-inputs carry
/// `sink`, source-outputs carry `source`; the other is absent.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct PactlStream {
    pub index: u32,
    #[serde(default)]
    pub sink: Option<u32>,
    #[serde(default)]
    pub source: Option<u32>,
    pub mute: bool,
    #[serde(default)]
    pub volume: BTreeMap<String, PactlChannelVolume>,
    #[serde(default)]
    pub properties: BTreeMap<String, serde_json::Value>,
}

/// One profile value in a card's `profiles` map (keyed by profile name).
#[derive(Debug, serde::Deserialize)]
pub(crate) struct PactlCardProfile {
    pub description: String,
    pub available: bool,
    #[serde(default)]
    pub sinks: u32,
    #[serde(default)]
    pub sources: u32,
}

/// One card from `pactl --format=json list cards`. Cards have no top-level
/// description; the human-readable name is `properties["device.description"]`.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct PactlCard {
    pub index: u32,
    pub name: String,
    pub active_profile: String,
    #[serde(default)]
    pub profiles: BTreeMap<String, PactlCardProfile>,
    #[serde(default)]
    pub properties: BTreeMap<String, serde_json::Value>,
}

/// Parse one `pactl --format=json list <kind>` buffer into DTOs.
pub(crate) fn parse_pactl_json_list<T: serde::de::DeserializeOwned>(
    raw: &str,
) -> Result<Vec<T>, serde_json::Error> {
    serde_json::from_str(raw)
}

/// Extract the first channel's percent from a volume map, matching the text
/// parser's first-percent-token behavior. serde_json's default map preserves
/// alphabetical key order, so for stereo layouts the first entry is
/// front-left — the same channel the long-form text output lists first.
pub(crate) fn percent_from_volume_map(volume: &BTreeMap<String, PactlChannelVolume>) -> Option<u8> {
    let channel = volume.values().next()?;
    channel
        .value_percent
        .strip_suffix('%')?
        .trim()
        .parse::<u8>()
        .ok()
}

/// Read a string-valued property, when present.
fn property_string(properties: &BTreeMap<String, serde_json::Value>, key: &str) -> Option<String> {
    properties
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

/// A source is a monitor (loopback of a sink) when its device class says so
/// or its name carries the conventional `.monitor` suffix. Monitors are
/// filtered out of the sound window's input-device list.
pub(crate) fn is_monitor_source(device: &PactlDevice) -> bool {
    device.name.ends_with(".monitor")
        || device
            .properties
            .get("device.class")
            .and_then(|value| value.as_str())
            == Some("monitor")
}

/// Map a pactl device DTO to the domain type, marking it default when its
/// name matches the default device name and resolving the active port name
/// to that port's description (falling back to the raw name).
pub(crate) fn map_device(device: &PactlDevice, default_name: &str) -> AudioDevice {
    let port = device.active_port.as_ref().map(|active_port_name| {
        device
            .ports
            .iter()
            .find(|port| &port.name == active_port_name)
            .map(|port| port.description.clone())
            .unwrap_or_else(|| active_port_name.clone())
    });
    AudioDevice {
        index: device.index,
        name: device.name.clone(),
        description: device.description.clone(),
        volume_percent: percent_from_volume_map(&device.volume).unwrap_or(0),
        muted: device.mute,
        is_default: device.name == default_name,
        port,
    }
}

/// Map a pactl stream DTO to the domain type. `device_index` comes from
/// `sink` for playback streams and `source` for recording streams.
pub(crate) fn map_stream(stream: &PactlStream) -> AudioStream {
    AudioStream {
        index: stream.index,
        application_name: property_string(&stream.properties, "application.name")
            .unwrap_or_default(),
        media_name: property_string(&stream.properties, "media.name").unwrap_or_default(),
        icon: property_string(&stream.properties, "application.icon_name"),
        volume_percent: percent_from_volume_map(&stream.volume).unwrap_or(0),
        muted: stream.mute,
        device_index: stream.sink.or(stream.source).unwrap_or(0),
    }
}

/// Map a pactl card DTO to the domain type, flattening the profiles map into
/// a vector (BTreeMap order: alphabetical by profile name).
pub(crate) fn map_card(card: &PactlCard) -> AudioCard {
    AudioCard {
        index: card.index,
        name: card.name.clone(),
        description: property_string(&card.properties, "device.description")
            .unwrap_or_else(|| card.name.clone()),
        active_profile: card.active_profile.clone(),
        profiles: card
            .profiles
            .iter()
            .map(|(profile_name, profile)| AudioCardProfile {
                name: profile_name.clone(),
                description: profile.description.clone(),
                available: profile.available,
                sink_count: profile.sinks,
                source_count: profile.sources,
            })
            .collect(),
    }
}

/// Execute an audio command via pactl.
async fn execute_audio_command(command: &AudioCommand) -> Result<(), ProvidersError> {
    // The microphone toggle targets the default source via the `@DEFAULT_SOURCE@`
    // alias, so it is handled before (and independently of) the default sink
    // lookup that the sink commands need.
    if let AudioCommand::ToggleMicMute = command {
        Command::new("pactl")
            .args(["set-source-mute", "@DEFAULT_SOURCE@", "toggle"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await
            .map_err(|e| ProvidersError::Spawn(e.to_string()))?;
        return Ok(());
    }

    let sink = get_default_sink().await.ok_or_else(|| {
        ProvidersError::ServiceUnavailable("no default sink available".to_string())
    })?;

    match command {
        AudioCommand::SetVolume(percent) => {
            Command::new("pactl")
                .args(["set-sink-volume", &sink, &format!("{}%", percent)])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .await
                .map_err(|e| ProvidersError::Spawn(e.to_string()))?;
            Ok(())
        }
        AudioCommand::ToggleMute => {
            Command::new("pactl")
                .args(["set-sink-mute", &sink, "toggle"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .await
                .map_err(|e| ProvidersError::Spawn(e.to_string()))?;
            Ok(())
        }
        AudioCommand::AdjustVolume(delta) => {
            let dir = if *delta >= 0 { "+" } else { "" };
            let abs_delta = delta.abs();
            Command::new("pactl")
                .args(["set-sink-volume", &sink, &format!("{}{}%", dir, abs_delta)])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .await
                .map_err(|e| ProvidersError::Spawn(e.to_string()))?;
            Ok(())
        }
        // Handled by the early return above; never reached here.
        AudioCommand::ToggleMicMute => Ok(()),
        // Table-mapped and session commands are routed through
        // `pactl_arguments` / session flags by `execute`; never reached here.
        _ => Ok(()),
    }
}

/// Get the default PulseAudio sink name.
async fn get_default_sink() -> Option<String> {
    let output = Command::new("pactl")
        .args(["get-default-sink"])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    }
}

/// Get the default PulseAudio source (microphone) name.
async fn get_default_source() -> Option<String> {
    let output = Command::new("pactl")
        .args(["get-default-source"])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    }
}

/// Decide whether a single line from `pactl subscribe` should trigger a
/// re-fetch of the audio state.
///
/// `pactl subscribe` emits lines in the shape `Event '<kind>' on <facility> #<id>`.
/// Device (`sink`, `source`), `server` (default device changed), and `card`
/// (profile switched) events always refresh. Stream events (`sink-input`,
/// `source-output`) fire constantly during playback or recording; honoring
/// them all the time would put us back into polling-territory CPU usage, so
/// they refresh ONLY while a sound-window session is open.
pub(crate) fn should_refresh_for_pactl_line(line: &str, session_active: bool) -> bool {
    let Some(rest) = line.split(" on ").nth(1) else {
        return false;
    };
    let facility = rest.split_whitespace().next().unwrap_or("");
    match facility {
        "sink" | "source" | "server" | "card" => true,
        "sink-input" | "source-output" => session_active,
        _ => false,
    }
}

/// Probe whether `pactl --format=json` works on this host. pactl versions
/// before 16 lack `--format`; per the design that means the provider reports
/// unavailable with a logged warning rather than limping on text parsing.
async fn probe_pactl_json_support() -> bool {
    let output = Command::new("pactl")
        .args(["--format=json", "list", "sinks"])
        .stdin(Stdio::null())
        .output()
        .await;
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            match parse_pactl_json_list::<PactlDevice>(&stdout) {
                Ok(_) => true,
                Err(error) => {
                    tracing::warn!(
                        "pactl --format=json output unparseable: {error}; audio provider unavailable"
                    );
                    false
                }
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                "pactl --format=json unsupported: {}; audio provider unavailable",
                stderr.trim()
            );
            false
        }
        Err(error) => {
            tracing::warn!("pactl probe failed to spawn: {error}; audio provider unavailable");
            false
        }
    }
}

/// Run a pactl mutation, checking the exit status. Non-zero exit carries the
/// captured stderr; a spawn failure is its own variant so `map_pactl_error`
/// can distinguish "pactl broken" from "command rejected".
async fn run_pactl(arguments: &[&str]) -> Result<(), ProvidersError> {
    let output = Command::new("pactl")
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| ProvidersError::Spawn(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(ProvidersError::ServiceUnavailable(stderr))
    }
}

/// Map a pactl failure to a typed domain error: spawn failures mean the
/// binary is unusable (Unsupported); a rejected command is ActionFailed with
/// pactl's own stderr as the reason.
fn map_pactl_error(error: ProvidersError) -> DomainError {
    match error {
        ProvidersError::Spawn(message) => DomainError::Unsupported(message),
        other => DomainError::ActionFailed {
            reason: other.to_string(),
        },
    }
}

/// Run one `pactl --format=json list <kind>` and parse it. Returns None on
/// any failure so the caller keeps the last-known state (design: pactl
/// subprocess failures log and keep last-known state).
async fn run_pactl_json_list<T: serde::de::DeserializeOwned>(kind: &str) -> Option<Vec<T>> {
    let output = Command::new("pactl")
        .args(["--format=json", "list", kind])
        .stdin(Stdio::null())
        .output()
        .await;
    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                "pactl --format=json list {kind} failed: {}; keeping last known state",
                stderr.trim()
            );
            return None;
        }
        Err(error) => {
            tracing::warn!(
                "pactl --format=json list {kind} failed to spawn: {error}; keeping last known state"
            );
            return None;
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    match parse_pactl_json_list::<T>(&stdout) {
        Ok(items) => Some(items),
        Err(error) => {
            tracing::warn!("failed to parse pactl {kind} JSON: {error}; keeping last known state");
            None
        }
    }
}

/// Assemble the full AudioState from already-parsed pactl DTOs. Pure (no
/// I/O) so the default-marking, monitor filtering, and default_sink /
/// default_source derivation are unit-testable against fixtures.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assemble_audio_state(
    sink_devices: Vec<PactlDevice>,
    source_devices: Vec<PactlDevice>,
    playback: Vec<PactlStream>,
    recording: Vec<PactlStream>,
    cards: Vec<PactlCard>,
    default_sink_name: &str,
    default_source_name: &str,
) -> AudioState {
    let sinks: Vec<AudioDevice> = sink_devices
        .iter()
        .map(|device| map_device(device, default_sink_name))
        .collect();
    let sources: Vec<AudioDevice> = source_devices
        .iter()
        .filter(|device| !is_monitor_source(device))
        .map(|device| map_device(device, default_source_name))
        .collect();
    let sink_summary = |device: &AudioDevice| AudioSink {
        name: device.name.clone(),
        description: device.description.clone(),
        volume_percent: device.volume_percent,
        muted: device.muted,
    };
    let default_sink = sinks
        .iter()
        .find(|device| device.is_default)
        .map(sink_summary);
    let default_source = sources
        .iter()
        .find(|device| device.is_default)
        .map(sink_summary);
    AudioState {
        available: true,
        default_sink,
        default_source,
        sinks,
        sources,
        playback_streams: playback.iter().map(map_stream).collect(),
        recording_streams: recording.iter().map(map_stream).collect(),
        cards: cards.iter().map(map_card).collect(),
    }
}

/// Fetch the full AudioState from pactl JSON. Streams are fetched ONLY while
/// a session is open; outside a session the provider spawns exactly the
/// device/card/default lookups. Returns None when any fetch fails, so the
/// stream keeps the last emitted state instead of publishing a hole.
async fn fetch_audio_state(session_active: bool) -> Option<AudioState> {
    let sinks: Vec<PactlDevice> = run_pactl_json_list("sinks").await?;
    let sources: Vec<PactlDevice> = run_pactl_json_list("sources").await?;
    let cards: Vec<PactlCard> = run_pactl_json_list("cards").await?;
    let (playback, recording): (Vec<PactlStream>, Vec<PactlStream>) = if session_active {
        (
            run_pactl_json_list("sink-inputs").await?,
            run_pactl_json_list("source-outputs").await?,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let default_sink_name = get_default_sink().await.unwrap_or_default();
    let default_source_name = get_default_source().await.unwrap_or_default();
    Some(assemble_audio_state(
        sinks,
        sources,
        playback,
        recording,
        cards,
        &default_sink_name,
        &default_source_name,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_set_volume_50() {
        let payload = json!({"command": "set_volume", "percent": 50});
        match parse_audio_action(&payload) {
            Ok(AudioCommand::SetVolume(50)) => {}
            other => panic!("expected SetVolume(50), got {:?}", other),
        }
    }

    #[test]
    fn parses_toggle_mute() {
        let payload = json!({"command": "toggle_mute"});
        match parse_audio_action(&payload) {
            Ok(AudioCommand::ToggleMute) => {}
            other => panic!("expected ToggleMute, got {:?}", other),
        }
    }

    #[test]
    fn parses_adjust_volume_positive() {
        let payload = json!({"command": "adjust_volume", "delta": 5});
        match parse_audio_action(&payload) {
            Ok(AudioCommand::AdjustVolume(5)) => {}
            other => panic!("expected AdjustVolume(5), got {:?}", other),
        }
    }

    #[test]
    fn parses_adjust_volume_negative() {
        let payload = json!({"command": "adjust_volume", "delta": -5});
        match parse_audio_action(&payload) {
            Ok(AudioCommand::AdjustVolume(-5)) => {}
            other => panic!("expected AdjustVolume(-5), got {:?}", other),
        }
    }

    #[test]
    fn rejects_unknown_command() {
        let payload = json!({"command": "reticulate_splines"});
        assert!(parse_audio_action(&payload).is_err());
    }

    #[test]
    fn rejects_missing_percent() {
        let payload = json!({"command": "set_volume"});
        assert!(parse_audio_action(&payload).is_err());
    }

    #[test]
    fn rejects_non_int_delta() {
        let payload = json!({"command": "adjust_volume", "delta": "five"});
        assert!(parse_audio_action(&payload).is_err());
    }

    #[test]
    fn pactl_subscribe_sink_change_triggers_refresh() {
        // PulseAudio / PipeWire-Pulse emits one of these whenever the sink
        // state changes (volume, mute, default-sink swap), so we refresh.
        assert!(should_refresh_for_pactl_line(
            "Event 'change' on sink #0",
            false
        ));
        assert!(should_refresh_for_pactl_line(
            "Event 'change' on sink #42",
            false
        ));
        assert!(should_refresh_for_pactl_line(
            "Event 'new' on sink #1",
            false
        ));
        assert!(should_refresh_for_pactl_line(
            "Event 'remove' on sink #1",
            false
        ));
    }

    #[test]
    fn pactl_subscribe_server_change_triggers_refresh() {
        // The default sink can change without any sink-level event — the
        // user picks a new default in `pactl set-default-sink` and the
        // server emits a `server` event. We must refresh on that too,
        // otherwise the bar keeps showing the old sink's volume.
        assert!(should_refresh_for_pactl_line(
            "Event 'change' on server #0",
            false
        ));
    }

    #[test]
    fn parses_toggle_mic_mute() {
        let payload = json!({"command": "toggle_mic_mute"});
        match parse_audio_action(&payload) {
            Ok(AudioCommand::ToggleMicMute) => {}
            other => panic!("expected ToggleMicMute, got {:?}", other),
        }
    }

    #[test]
    fn pactl_subscribe_source_change_triggers_refresh() {
        // A microphone mute/volume change emits a `source` event; we refresh so
        // the bar reflects the new mic mute state live.
        assert!(should_refresh_for_pactl_line(
            "Event 'change' on source #4",
            false
        ));
        assert!(should_refresh_for_pactl_line(
            "Event 'new' on source #1",
            false
        ));
    }

    #[test]
    fn pactl_subscribe_unrelated_events_do_not_trigger_refresh() {
        // Sink-input and source-output events fire constantly during playback /
        // recording; ignoring them is the entire point of moving off
        // polling. If we refreshed on every sink-input change we'd be
        // back to 5+Hz wakeups during playback. Note `source-output` (the
        // recording stream) is distinct from `source` (the device) and must
        // NOT refresh.
        assert!(!should_refresh_for_pactl_line(
            "Event 'change' on sink-input #123",
            false
        ));
        assert!(!should_refresh_for_pactl_line(
            "Event 'change' on source-output #7",
            false
        ));
        assert!(!should_refresh_for_pactl_line(
            "Event 'change' on client #99",
            false
        ));
        // Stray blank / garbage lines must not refresh either.
        assert!(!should_refresh_for_pactl_line("", false));
        assert!(!should_refresh_for_pactl_line("garbage", false));
    }

    #[test]
    fn pactl_subscribe_card_events_always_trigger_refresh() {
        // Card profile switches must be reflected even when the sound window
        // is closed, so `card` refreshes with and without a session.
        assert!(should_refresh_for_pactl_line(
            "Event 'change' on card #48",
            false
        ));
        assert!(should_refresh_for_pactl_line(
            "Event 'change' on card #48",
            true
        ));
    }

    #[test]
    fn pactl_subscribe_stream_events_refresh_only_during_a_session() {
        // With the sound window open we need live stream rows; sink-input and
        // source-output events are honored then, and ONLY then. This is the
        // session-gating guarantee: no open session, no sink-input subprocess
        // churn.
        assert!(should_refresh_for_pactl_line(
            "Event 'change' on sink-input #123",
            true
        ));
        assert!(should_refresh_for_pactl_line(
            "Event 'new' on source-output #7",
            true
        ));
        // Session or not, client and garbage lines never refresh.
        assert!(!should_refresh_for_pactl_line(
            "Event 'change' on client #99",
            true
        ));
        assert!(!should_refresh_for_pactl_line("garbage", true));
        assert!(!should_refresh_for_pactl_line("", true));
    }

    #[tokio::test]
    #[ignore = "requires real Pulse/PipeWire-pulse"]
    async fn yields_initial_state_within_2s() {
        use futures::StreamExt;
        use std::time::Duration;

        let p = PulseAudioProvider::connect(tokio::runtime::Handle::current())
            .await
            .expect("connect");
        let mut stream = p.subscribe().expect("subscribe");
        let v = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("first state within 2s")
            .expect("Some");
        let _state: AudioState = serde_json::from_value(v).expect("AudioState");
    }

    // Real `pactl --format=json list sinks` output (pactl 17.0, PipeWire),
    // trimmed: properties shortened, second sink's mute flipped to true so
    // mute mapping is exercised.
    const SINKS_JSON_FIXTURE: &str = r#"[
      {
        "index": 59,
        "state": "RUNNING",
        "name": "alsa_output.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Speaker__sink",
        "description": "Arrow Lake cAVS Speaker",
        "driver": "PipeWire",
        "sample_specification": "s32le 2ch 48000Hz",
        "mute": false,
        "volume": {
          "front-left": { "value": 36036, "value_percent": "55%", "db": "-15.58 dB" },
          "front-right": { "value": 36036, "value_percent": "55%", "db": "-15.58 dB" }
        },
        "balance": 0.0,
        "base_volume": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" },
        "active_port": "[Out] Speaker",
        "ports": [
          { "name": "[Out] Speaker", "description": "Speaker", "type": "Speaker", "priority": 100, "availability": "availability unknown" }
        ],
        "properties": { "device.icon_name": "audio-card", "media.class": "Audio/Sink" },
        "formats": ["pcm"]
      },
      {
        "index": 56,
        "state": "SUSPENDED",
        "name": "alsa_output.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__HDMI3__sink",
        "description": "Arrow Lake cAVS HDMI / DisplayPort 3 Output",
        "driver": "PipeWire",
        "mute": true,
        "volume": {
          "front-left": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" },
          "front-right": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" }
        },
        "active_port": "[Out] HDMI3",
        "ports": [
          { "name": "[Out] HDMI3", "description": "HDMI / DisplayPort 3 Output", "type": "HDMI", "priority": 700, "availability": "not available" }
        ],
        "properties": { "device.icon_name": "video-display", "media.class": "Audio/Sink" }
      }
    ]"#;

    // Real `pactl --format=json list sources`: one monitor source (must be
    // filtered) and one real microphone.
    const SOURCES_JSON_FIXTURE: &str = r#"[
      {
        "index": 59,
        "name": "alsa_output.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Speaker__sink.monitor",
        "description": "Monitor of Arrow Lake cAVS Speaker",
        "mute": false,
        "volume": {
          "front-left": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" },
          "front-right": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" }
        },
        "active_port": "[Out] Speaker",
        "properties": { "device.class": "monitor" }
      },
      {
        "index": 61,
        "name": "alsa_input.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Mic1__source",
        "description": "Arrow Lake cAVS Digital Microphone",
        "mute": false,
        "volume": {
          "front-left": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" },
          "front-right": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" }
        },
        "active_port": "[In] Mic1",
        "ports": [
          { "name": "[In] Mic1", "description": "Digital Microphone", "type": "Mic", "priority": 200, "availability": "availability unknown" }
        ],
        "properties": { "device.class": "sound" }
      }
    ]"#;

    // Real `pactl --format=json list sink-inputs` while paplay was running.
    const SINK_INPUTS_JSON_FIXTURE: &str = r#"[
      {
        "index": 900,
        "driver": "protocol-native.c",
        "owner_module": 4294967295,
        "client": 901,
        "sink": 59,
        "mute": false,
        "corked": false,
        "volume": {
          "front-left": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" },
          "front-right": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" }
        },
        "properties": {
          "application.name": "paplay",
          "media.name": "/dev/zero",
          "application.process.binary": "paplay"
        }
      }
    ]"#;

    // Real `pactl --format=json list source-outputs` while parecord was running.
    const SOURCE_OUTPUTS_JSON_FIXTURE: &str = r#"[
      {
        "index": 932,
        "driver": "protocol-native.c",
        "client": 933,
        "source": 61,
        "mute": false,
        "corked": false,
        "volume": {
          "front-left": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" },
          "front-right": { "value": 65536, "value_percent": "100%", "db": "0.00 dB" }
        },
        "properties": {
          "application.name": "parecord",
          "media.name": "/dev/null"
        }
      }
    ]"#;

    // Real `pactl --format=json list cards`, trimmed to three profiles.
    // Note: cards carry NO top-level "description"; the human-readable name
    // is properties["device.description"].
    const CARDS_JSON_FIXTURE: &str = r#"[
      {
        "index": 48,
        "name": "alsa_card.pci-0000_00_1f.3-platform-skl_hda_dsp_generic",
        "driver": "alsa",
        "owner_module": 4294967295,
        "active_profile": "HiFi (HDMI1, HDMI2, HDMI3, Mic1, Mic2, Speaker)",
        "profiles": {
          "off": { "description": "Off", "sinks": 0, "sources": 0, "priority": 0, "available": true },
          "HiFi (HDMI1, HDMI2, HDMI3, Headphones, Mic1, Mic2)": { "description": "Play HiFi quality Music (HDMI1, HDMI2, HDMI3, Headphones, Mic1, Mic2)", "sinks": 4, "sources": 2, "priority": 10300, "available": false },
          "HiFi (HDMI1, HDMI2, HDMI3, Mic1, Mic2, Speaker)": { "description": "Play HiFi quality Music (HDMI1, HDMI2, HDMI3, Mic1, Mic2, Speaker)", "sinks": 4, "sources": 2, "priority": 10200, "available": true }
        },
        "properties": { "device.description": "Arrow Lake cAVS", "device.nick": "sof-hda-dsp" }
      }
    ]"#;

    #[test]
    fn parses_sinks_json_and_maps_devices() {
        let devices: Vec<PactlDevice> =
            parse_pactl_json_list(SINKS_JSON_FIXTURE).expect("sinks fixture parses");
        assert_eq!(devices.len(), 2);
        let default_name =
            "alsa_output.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Speaker__sink";
        let speaker = map_device(&devices[0], default_name);
        assert_eq!(speaker.index, 59);
        assert_eq!(speaker.description, "Arrow Lake cAVS Speaker");
        assert_eq!(speaker.volume_percent, 55);
        assert!(!speaker.muted);
        assert!(speaker.is_default);
        // Active port name "[Out] Speaker" resolves to its description.
        assert_eq!(speaker.port.as_deref(), Some("Speaker"));
        let hdmi = map_device(&devices[1], default_name);
        assert!(hdmi.muted);
        assert!(!hdmi.is_default);
        assert_eq!(hdmi.volume_percent, 100);
    }

    #[test]
    fn monitor_sources_are_detected() {
        let devices: Vec<PactlDevice> =
            parse_pactl_json_list(SOURCES_JSON_FIXTURE).expect("sources fixture parses");
        assert_eq!(devices.len(), 2);
        assert!(is_monitor_source(&devices[0]));
        assert!(!is_monitor_source(&devices[1]));
    }

    #[test]
    fn parses_sink_inputs_json_and_maps_streams() {
        let streams: Vec<PactlStream> =
            parse_pactl_json_list(SINK_INPUTS_JSON_FIXTURE).expect("sink-inputs fixture parses");
        assert_eq!(streams.len(), 1);
        let stream = map_stream(&streams[0]);
        assert_eq!(stream.index, 900);
        assert_eq!(stream.application_name, "paplay");
        assert_eq!(stream.media_name, "/dev/zero");
        assert_eq!(stream.icon, None);
        assert_eq!(stream.volume_percent, 100);
        assert!(!stream.muted);
        assert_eq!(stream.device_index, 59);
    }

    #[test]
    fn parses_source_outputs_json_and_maps_streams() {
        let streams: Vec<PactlStream> = parse_pactl_json_list(SOURCE_OUTPUTS_JSON_FIXTURE)
            .expect("source-outputs fixture parses");
        let stream = map_stream(&streams[0]);
        assert_eq!(stream.index, 932);
        assert_eq!(stream.application_name, "parecord");
        assert_eq!(stream.device_index, 61);
    }

    #[test]
    fn parses_cards_json_and_maps_profiles() {
        let cards: Vec<PactlCard> =
            parse_pactl_json_list(CARDS_JSON_FIXTURE).expect("cards fixture parses");
        assert_eq!(cards.len(), 1);
        let card = map_card(&cards[0]);
        assert_eq!(card.index, 48);
        // Description comes from properties["device.description"], not a
        // top-level field (cards have none).
        assert_eq!(card.description, "Arrow Lake cAVS");
        assert_eq!(
            card.active_profile,
            "HiFi (HDMI1, HDMI2, HDMI3, Mic1, Mic2, Speaker)"
        );
        assert_eq!(card.profiles.len(), 3);
        let unavailable = card
            .profiles
            .iter()
            .find(|profile| profile.name.contains("Headphones"))
            .expect("headphones profile present");
        assert!(!unavailable.available);
        assert!(unavailable.description.starts_with("Play HiFi"));
        // Per-profile sink/source counts flow through from pactl JSON.
        let off = card
            .profiles
            .iter()
            .find(|profile| profile.name == "off")
            .expect("off profile present");
        assert_eq!(off.sink_count, 0);
        assert_eq!(off.source_count, 0);
        let active = card
            .profiles
            .iter()
            .find(|profile| profile.name == card.active_profile)
            .expect("active profile present");
        assert_eq!(active.sink_count, 4);
        assert_eq!(active.source_count, 2);
    }

    #[test]
    fn percent_from_volume_map_reads_first_channel() {
        let devices: Vec<PactlDevice> =
            parse_pactl_json_list(SINKS_JSON_FIXTURE).expect("fixture parses");
        assert_eq!(percent_from_volume_map(&devices[0].volume), Some(55));
        let empty: std::collections::BTreeMap<String, PactlChannelVolume> =
            std::collections::BTreeMap::new();
        assert_eq!(percent_from_volume_map(&empty), None);
    }

    #[test]
    fn malformed_json_is_a_parse_error_not_a_panic() {
        let result: Result<Vec<PactlDevice>, _> = parse_pactl_json_list("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn parses_session_commands() {
        assert!(matches!(
            parse_audio_action(&json!({"command": "open_session"})),
            Ok(AudioCommand::OpenSession)
        ));
        assert!(matches!(
            parse_audio_action(&json!({"command": "close_session"})),
            Ok(AudioCommand::CloseSession)
        ));
    }

    #[test]
    fn parses_set_default_sink_and_source() {
        match parse_audio_action(&json!({"command": "set_default_sink", "name": "spk"})) {
            Ok(AudioCommand::SetDefaultSink { name }) => assert_eq!(name, "spk"),
            other => panic!("expected SetDefaultSink, got {:?}", other),
        }
        match parse_audio_action(&json!({"command": "set_default_source", "name": "mic"})) {
            Ok(AudioCommand::SetDefaultSource { name }) => assert_eq!(name, "mic"),
            other => panic!("expected SetDefaultSource, got {:?}", other),
        }
        assert!(parse_audio_action(&json!({"command": "set_default_sink"})).is_err());
    }

    #[test]
    fn parses_set_device_volume_and_mute() {
        match parse_audio_action(
            &json!({"command": "set_device_volume", "kind": "sink", "name": "spk", "percent": 70}),
        ) {
            Ok(AudioCommand::SetDeviceVolume {
                kind: AudioDeviceKind::Sink,
                name,
                percent: 70,
            }) => assert_eq!(name, "spk"),
            other => panic!("expected SetDeviceVolume, got {:?}", other),
        }
        match parse_audio_action(
            &json!({"command": "set_device_mute", "kind": "source", "name": "mic", "muted": true}),
        ) {
            Ok(AudioCommand::SetDeviceMute {
                kind: AudioDeviceKind::Source,
                name,
                muted: true,
            }) => assert_eq!(name, "mic"),
            other => panic!("expected SetDeviceMute, got {:?}", other),
        }
        // Unknown kind rejected.
        assert!(parse_audio_action(
            &json!({"command": "set_device_volume", "kind": "banana", "name": "x", "percent": 10})
        )
        .is_err());
    }

    #[test]
    fn set_device_volume_clamps_percent_to_150() {
        match parse_audio_action(
            &json!({"command": "set_device_volume", "kind": "sink", "name": "spk", "percent": 400}),
        ) {
            Ok(AudioCommand::SetDeviceVolume { percent, .. }) => assert_eq!(percent, 150),
            other => panic!("expected clamped SetDeviceVolume, got {:?}", other),
        }
    }

    #[test]
    fn parses_stream_volume_mute_and_move() {
        match parse_audio_action(
            &json!({"command": "set_stream_volume", "kind": "playback", "index": 900, "percent": 80}),
        ) {
            Ok(AudioCommand::SetStreamVolume {
                kind: AudioStreamKind::Playback,
                index: 900,
                percent: 80,
            }) => {}
            other => panic!("expected SetStreamVolume, got {:?}", other),
        }
        match parse_audio_action(
            &json!({"command": "set_stream_mute", "kind": "record", "index": 932, "muted": false}),
        ) {
            Ok(AudioCommand::SetStreamMute {
                kind: AudioStreamKind::Record,
                index: 932,
                muted: false,
            }) => {}
            other => panic!("expected SetStreamMute, got {:?}", other),
        }
        match parse_audio_action(
            &json!({"command": "move_stream", "kind": "playback", "index": 900, "device_name": "hdmi"}),
        ) {
            Ok(AudioCommand::MoveStream {
                kind: AudioStreamKind::Playback,
                index: 900,
                device_name,
            }) => assert_eq!(device_name, "hdmi"),
            other => panic!("expected MoveStream, got {:?}", other),
        }
        // Missing index rejected.
        assert!(parse_audio_action(
            &json!({"command": "set_stream_volume", "kind": "playback", "percent": 80})
        )
        .is_err());
    }

    #[test]
    fn parses_set_card_profile() {
        match parse_audio_action(
            &json!({"command": "set_card_profile", "card_index": 48, "profile": "HiFi"}),
        ) {
            Ok(AudioCommand::SetCardProfile {
                card_index: 48,
                profile,
            }) => assert_eq!(profile, "HiFi"),
            other => panic!("expected SetCardProfile, got {:?}", other),
        }
        assert!(
            parse_audio_action(&json!({"command": "set_card_profile", "profile": "x"})).is_err()
        );
    }

    #[test]
    fn pactl_arguments_match_the_design_table() {
        let cases: Vec<(AudioCommand, Vec<&str>)> = vec![
            (
                AudioCommand::SetDefaultSink { name: "spk".into() },
                vec!["set-default-sink", "spk"],
            ),
            (
                AudioCommand::SetDefaultSource { name: "mic".into() },
                vec!["set-default-source", "mic"],
            ),
            (
                AudioCommand::SetDeviceVolume {
                    kind: AudioDeviceKind::Sink,
                    name: "spk".into(),
                    percent: 70,
                },
                vec!["set-sink-volume", "spk", "70%"],
            ),
            (
                AudioCommand::SetDeviceVolume {
                    kind: AudioDeviceKind::Source,
                    name: "mic".into(),
                    percent: 55,
                },
                vec!["set-source-volume", "mic", "55%"],
            ),
            (
                AudioCommand::SetDeviceMute {
                    kind: AudioDeviceKind::Sink,
                    name: "spk".into(),
                    muted: true,
                },
                vec!["set-sink-mute", "spk", "1"],
            ),
            (
                AudioCommand::SetDeviceMute {
                    kind: AudioDeviceKind::Source,
                    name: "mic".into(),
                    muted: false,
                },
                vec!["set-source-mute", "mic", "0"],
            ),
            (
                AudioCommand::SetStreamVolume {
                    kind: AudioStreamKind::Playback,
                    index: 900,
                    percent: 80,
                },
                vec!["set-sink-input-volume", "900", "80%"],
            ),
            (
                AudioCommand::SetStreamVolume {
                    kind: AudioStreamKind::Record,
                    index: 932,
                    percent: 40,
                },
                vec!["set-source-output-volume", "932", "40%"],
            ),
            (
                AudioCommand::SetStreamMute {
                    kind: AudioStreamKind::Playback,
                    index: 900,
                    muted: true,
                },
                vec!["set-sink-input-mute", "900", "1"],
            ),
            (
                AudioCommand::SetStreamMute {
                    kind: AudioStreamKind::Record,
                    index: 932,
                    muted: false,
                },
                vec!["set-source-output-mute", "932", "0"],
            ),
            (
                AudioCommand::MoveStream {
                    kind: AudioStreamKind::Playback,
                    index: 900,
                    device_name: "hdmi".into(),
                },
                vec!["move-sink-input", "900", "hdmi"],
            ),
            (
                AudioCommand::MoveStream {
                    kind: AudioStreamKind::Record,
                    index: 932,
                    device_name: "mic".into(),
                },
                vec!["move-source-output", "932", "mic"],
            ),
            (
                AudioCommand::SetCardProfile {
                    card_index: 48,
                    profile: "HiFi (HDMI1)".into(),
                },
                vec!["set-card-profile", "48", "HiFi (HDMI1)"],
            ),
        ];
        for (command, expected) in cases {
            let arguments = pactl_arguments(&command)
                .unwrap_or_else(|| panic!("expected arguments for {:?}", command));
            assert_eq!(arguments, expected, "argv mismatch for {:?}", command);
        }
        // Session commands and the legacy four are executed elsewhere.
        assert!(pactl_arguments(&AudioCommand::OpenSession).is_none());
        assert!(pactl_arguments(&AudioCommand::CloseSession).is_none());
        assert!(pactl_arguments(&AudioCommand::ToggleMute).is_none());
        assert!(pactl_arguments(&AudioCommand::SetVolume(50)).is_none());
        assert!(pactl_arguments(&AudioCommand::AdjustVolume(-5)).is_none());
        assert!(pactl_arguments(&AudioCommand::ToggleMicMute).is_none());
    }

    fn provider_with_session(
        available: bool,
        session: std::sync::Arc<AudioSession>,
    ) -> PulseAudioProvider {
        PulseAudioProvider {
            id: ProviderId::from("audio"),
            available,
            session,
        }
    }

    #[test]
    fn assemble_audio_state_builds_the_full_snapshot() {
        let sinks: Vec<PactlDevice> = parse_pactl_json_list(SINKS_JSON_FIXTURE).expect("sinks");
        let sources: Vec<PactlDevice> =
            parse_pactl_json_list(SOURCES_JSON_FIXTURE).expect("sources");
        let playback: Vec<PactlStream> =
            parse_pactl_json_list(SINK_INPUTS_JSON_FIXTURE).expect("sink-inputs");
        let recording: Vec<PactlStream> =
            parse_pactl_json_list(SOURCE_OUTPUTS_JSON_FIXTURE).expect("source-outputs");
        let cards: Vec<PactlCard> = parse_pactl_json_list(CARDS_JSON_FIXTURE).expect("cards");

        let state = assemble_audio_state(
            sinks,
            sources,
            playback,
            recording,
            cards,
            "alsa_output.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Speaker__sink",
            "alsa_input.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Mic1__source",
        );

        assert!(state.available);
        // default_sink keeps the legacy AudioSink shape with identical percent
        // semantics: Speaker at 55%, not muted.
        let default_sink = state.default_sink.expect("default sink present");
        assert_eq!(default_sink.description, "Arrow Lake cAVS Speaker");
        assert_eq!(default_sink.volume_percent, 55);
        assert!(!default_sink.muted);
        let default_source = state.default_source.expect("default source present");
        assert_eq!(
            default_source.description,
            "Arrow Lake cAVS Digital Microphone"
        );
        // Monitor source filtered: only the microphone remains.
        assert_eq!(state.sources.len(), 1);
        assert!(state.sources[0].is_default);
        assert_eq!(state.sinks.len(), 2);
        assert!(state.sinks.iter().any(|device| device.is_default));
        assert_eq!(state.playback_streams.len(), 1);
        assert_eq!(state.playback_streams[0].device_index, 59);
        assert_eq!(state.recording_streams.len(), 1);
        assert_eq!(state.cards.len(), 1);
    }

    #[test]
    fn assemble_audio_state_without_default_names_marks_nothing_default() {
        let sinks: Vec<PactlDevice> = parse_pactl_json_list(SINKS_JSON_FIXTURE).expect("sinks");
        let state = assemble_audio_state(sinks, vec![], vec![], vec![], vec![], "", "");
        assert!(state.default_sink.is_none());
        assert!(state.default_source.is_none());
        assert!(state.sinks.iter().all(|device| !device.is_default));
    }

    #[tokio::test]
    async fn open_and_close_session_flip_the_flag_and_keep_the_cache() {
        let session = std::sync::Arc::new(AudioSession::default());
        {
            let mut last = session.last.lock().await;
            *last = Some(AudioState::default());
        }
        let provider = provider_with_session(true, session.clone());

        provider
            .execute(AudioCommand::OpenSession)
            .await
            .expect("open_session succeeds");
        assert!(session.active.load(Ordering::Relaxed));

        provider
            .execute(AudioCommand::CloseSession)
            .await
            .expect("close_session succeeds");
        assert!(!session.active.load(Ordering::Relaxed));
        // close_session must NOT clear the snapshot cache (wifi lesson): a
        // re-open queries snapshot() first and an empty cache would show a
        // dead window until the stream re-populates.
        assert!(session.last.lock().await.is_some());
    }

    #[tokio::test]
    async fn snapshot_serves_the_cached_state() {
        let session = std::sync::Arc::new(AudioSession::default());
        {
            let mut last = session.last.lock().await;
            let state = AudioState {
                available: true,
                ..AudioState::default()
            };
            *last = Some(state);
        }
        let provider = provider_with_session(true, session);
        let snapshot = provider.snapshot().await.expect("cached snapshot");
        assert_eq!(snapshot["available"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn snapshot_is_none_before_first_emission() {
        let provider = provider_with_session(true, std::sync::Arc::new(AudioSession::default()));
        assert!(provider.snapshot().await.is_none());
    }

    #[tokio::test]
    async fn invoke_rejects_foreign_custom_kind() {
        let provider = provider_with_session(true, std::sync::Arc::new(AudioSession::default()));
        let action = Action::Custom {
            kind: "wifi".to_string(),
            payload: json!({"command": "open_session"}),
        };
        assert!(provider.invoke(&action).await.is_err());
    }

    #[tokio::test]
    async fn invoke_on_unavailable_provider_is_unsupported() {
        let provider = provider_with_session(false, std::sync::Arc::new(AudioSession::default()));
        let action = Action::Custom {
            kind: "audio".to_string(),
            payload: json!({"command": "open_session"}),
        };
        assert!(provider.invoke(&action).await.is_err());
    }
}
