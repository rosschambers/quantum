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
use std::process::Stdio;
use tokio::process::Command;

use quantum_domain::{
    Action, ActionOutcome, AudioSink, AudioState, DomainError, Match, ProviderCapabilities,
    ProviderId, ProviderSource, Query,
};

use crate::error::InfrastructureError;

/// PulseAudio/PipeWire provider using `pactl` shell commands.
pub struct PulseAudioProvider {
    id: ProviderId,
    available: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum AudioCommand {
    SetVolume(u8),
    ToggleMute,
    AdjustVolume(i32),
}

impl PulseAudioProvider {
    /// Attempt to connect to PulseAudio via `pactl`.
    ///
    /// If `pactl` is not found in PATH, returns `Ok(Self { available: false })`.
    pub async fn connect(_runtime: tokio::runtime::Handle) -> Result<Self, InfrastructureError> {
        let available = which::which("pactl").is_ok();
        Ok(Self {
            id: ProviderId::from("audio"),
            available,
        })
    }
}

#[async_trait]
impl ProviderSource for PulseAudioProvider {
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
                execute_audio_command(&command)
                    .await
                    .map_err(|e| DomainError::Unsupported(e.to_string()))?;
                Ok(ActionOutcome { message: None })
            }
            _ => Err(DomainError::Unsupported(
                "audio provider only supports custom actions".to_string(),
            )),
        }
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        if !self.available {
            return Some(crate::providers::dbus_common::unavailable_stream::<
                AudioState,
            >());
        }

        Some(polling_stream())
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
        "adjust_volume" => {
            let delta = payload
                .get("delta")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| {
                    DomainError::Unsupported("'adjust_volume' requires 'delta' (i64)".to_string())
                })?;
            Ok(AudioCommand::AdjustVolume(delta as i32))
        }
        _ => Err(DomainError::Unsupported(format!(
            "unknown audio command: {}",
            command
        ))),
    }
}

/// Convert average channel volume (normalized [0, 1.5+]) to percent [0, 150].
#[allow(dead_code)]
pub(crate) fn channel_volumes_to_percent(channel_avg_normalized: f32) -> u8 {
    ((channel_avg_normalized * 100.0).clamp(0.0, 150.0)) as u8
}

/// Parse pactl volume output to extract percent value.
/// Example: "Volume: front-left: 38863 / 65% / -7.34 dB"
/// Returns 65 from that output.
pub(crate) fn parse_pactl_volume_percent(stdout: &str) -> Option<u8> {
    for line in stdout.lines() {
        if line.contains("Volume:") {
            // Look for pattern "NN%" in the line
            for word in line.split_whitespace() {
                if let Some(num_str) = word.strip_suffix('%') {
                    if let Ok(n) = num_str.parse::<u8>() {
                        return Some(n);
                    }
                }
            }
        }
    }
    None
}

/// Parse pactl mute output. Expected values: "yes" or "no".
pub(crate) fn parse_pactl_mute(stdout: &str) -> Option<bool> {
    for line in stdout.lines() {
        if line.contains("Mute:") {
            if line.contains("yes") {
                return Some(true);
            } else if line.contains("no") {
                return Some(false);
            }
        }
    }
    None
}

/// Execute an audio command via pactl.
async fn execute_audio_command(command: &AudioCommand) -> Result<(), InfrastructureError> {
    let sink = get_default_sink().await.ok_or_else(|| {
        InfrastructureError::ServiceUnavailable("no default sink available".to_string())
    })?;

    match command {
        AudioCommand::SetVolume(percent) => {
            Command::new("pactl")
                .args(["set-sink-volume", &sink, &format!("{}%", percent)])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .await
                .map_err(|e| InfrastructureError::Spawn(e.to_string()))?;
            Ok(())
        }
        AudioCommand::ToggleMute => {
            Command::new("pactl")
                .args(["set-sink-mute", &sink, "toggle"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .await
                .map_err(|e| InfrastructureError::Spawn(e.to_string()))?;
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
                .map_err(|e| InfrastructureError::Spawn(e.to_string()))?;
            Ok(())
        }
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

/// Fetch current sink info and build an AudioSink DTO.
async fn get_sink_info(sink_name: &str) -> Option<AudioSink> {
    let output = Command::new("pactl")
        .args(["list", "sinks", "short"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 && parts[1] == sink_name {
            // Found the sink; now query its volume and mute status
            let volume_output = Command::new("pactl")
                .args(["get-sink-volume", sink_name])
                .output()
                .await
                .ok()?;
            let volume_str = String::from_utf8(volume_output.stdout).ok()?;
            let volume_percent = parse_pactl_volume_percent(&volume_str)?;

            let mute_output = Command::new("pactl")
                .args(["get-sink-mute", sink_name])
                .output()
                .await
                .ok()?;
            let mute_str = String::from_utf8(mute_output.stdout).ok()?;
            let muted = parse_pactl_mute(&mute_str).unwrap_or(false);

            let description = parts.get(3).map(|s| s.to_string()).unwrap_or_default();

            return Some(AudioSink {
                name: sink_name.to_string(),
                description,
                volume_percent,
                muted,
            });
        }
    }

    None
}

/// Polling stream that queries sink status once per second. `MissedTickBehavior::Skip`
/// prevents a transient stall (for example a slow `pactl` invocation) from making
/// the loop fire several catch-up ticks back-to-back.
fn polling_stream() -> BoxStream<'static, serde_json::Value> {
    Box::pin(async_stream::stream! {
        let mut last_state: Option<serde_json::Value> = None;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;

            if let Some(default_sink) = get_default_sink().await {
                if let Some(sink) = get_sink_info(&default_sink).await {
                    let state = AudioState {
                        available: true,
                        default_sink: Some(sink),
                    };
                    let json_val = serde_json::to_value(&state).unwrap_or(serde_json::Value::Null);
                    if last_state.as_ref() != Some(&json_val) {
                        last_state = Some(json_val.clone());
                        yield json_val;
                    }
                } else {
                    // Sink query failed
                    let state = AudioState::default();
                    let json_val = serde_json::to_value(&state).unwrap_or(serde_json::Value::Null);
                    if last_state.as_ref() != Some(&json_val) {
                        last_state = Some(json_val.clone());
                        yield json_val;
                    }
                }
            } else {
                // No default sink
                let state = AudioState::default();
                let json_val = serde_json::to_value(&state).unwrap_or(serde_json::Value::Null);
                if last_state.as_ref() != Some(&json_val) {
                    last_state = Some(json_val.clone());
                    yield json_val;
                }
            }
        }
    })
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
    fn channel_volumes_to_percent_normal() {
        let percent = channel_volumes_to_percent(0.5);
        assert_eq!(percent, 50);
    }

    #[test]
    fn channel_volumes_to_percent_clamps_above_max() {
        let percent = channel_volumes_to_percent(2.0);
        assert_eq!(percent, 150);
    }

    #[test]
    fn channel_volumes_to_percent_clamps_negative() {
        let percent = channel_volumes_to_percent(-0.1);
        assert_eq!(percent, 0);
    }

    #[test]
    fn parse_pactl_volume_extracts_percent() {
        let output = "Volume: front-left: 38863 / 65% / -7.34 dB\n\
                      Volume: front-right: 38863 / 65% / -7.34 dB";
        let percent = parse_pactl_volume_percent(output);
        assert_eq!(percent, Some(65));
    }

    #[test]
    fn parse_pactl_volume_multi_channel() {
        let output = "Volume: front-left: 30000 / 51% / -11.00 dB\n\
                      Volume: front-right: 35000 / 59% / -9.00 dB";
        let percent = parse_pactl_volume_percent(output);
        assert_eq!(percent, Some(51)); // Takes the first percentage found
    }

    #[test]
    fn parse_pactl_mute_yes() {
        let output = "Mute: yes";
        let muted = parse_pactl_mute(output);
        assert_eq!(muted, Some(true));
    }

    #[test]
    fn parse_pactl_mute_no() {
        let output = "Mute: no";
        let muted = parse_pactl_mute(output);
        assert_eq!(muted, Some(false));
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
}
