use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use zbus::names::BusName;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, Proxy};

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, MprisState, PlaybackStatus, ProviderCapabilities,
    ProviderId, ProviderSource, Query,
};

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

pub struct MprisProvider {
    id: ProviderId,
    active_player: Arc<tokio::sync::Mutex<Option<String>>>,
    tx: broadcast::Sender<serde_json::Value>,
}

impl MprisProvider {
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        let id = ProviderId::from("mpris");
        let active_player = Arc::new(tokio::sync::Mutex::new(None));
        let (tx, _rx) = broadcast::channel::<serde_json::Value>(16);

        let active_player_for_task = active_player.clone();
        let tx_for_task = tx.clone();

        runtime.spawn(async move {
            let mut backoff_secs = 1u64;
            loop {
                match mpris_task(active_player_for_task.clone(), tx_for_task.clone()).await {
                    Ok(_) => {
                        backoff_secs = 1;
                    }
                    Err(err) => {
                        tracing::warn!("mpris task error: {err}");
                        backoff_secs = (backoff_secs * 2).min(30);
                    }
                }
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            }
        });

        Self {
            id,
            active_player,
            tx,
        }
    }

    pub async fn invoke_command(&self, command: &str) -> Result<(), DomainError> {
        let method = mpris_method_for_command(command)
            .ok_or_else(|| DomainError::Unsupported(format!("unknown mpris command: {command}")))?;

        let player_name =
            self.active_player
                .lock()
                .await
                .clone()
                .ok_or_else(|| DomainError::ActionFailed {
                    reason: "no active mpris player".into(),
                })?;

        let conn = Connection::session()
            .await
            .map_err(|e| DomainError::ActionFailed {
                reason: format!("dbus connect: {e}"),
            })?;

        let bus_name =
            BusName::try_from(player_name.clone()).map_err(|e| DomainError::ActionFailed {
                reason: format!("invalid bus name {player_name}: {e}"),
            })?;

        let proxy = Proxy::new(&conn, bus_name, MPRIS_OBJECT_PATH, MPRIS_PLAYER_INTERFACE)
            .await
            .map_err(|e| DomainError::ActionFailed {
                reason: format!("build mpris proxy: {e}"),
            })?;

        proxy
            .call_method(method, &())
            .await
            .map_err(|e| DomainError::ActionFailed {
                reason: format!("mpris {method} failed: {e}"),
            })?;

        Ok(())
    }
}

#[async_trait]
impl ProviderSource for MprisProvider {
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
            Action::Custom { kind, payload } if kind == "mpris" => {
                if let Some(command) = payload.get("command").and_then(|v| v.as_str()) {
                    self.invoke_command(command).await?;
                    Ok(ActionOutcome {
                        message: Some(format!("executed mpris command: {command}")),
                    })
                } else {
                    Err(DomainError::ActionFailed {
                        reason: "missing command field in mpris action".into(),
                    })
                }
            }
            _ => Err(DomainError::Unsupported(
                "mpris provider only handles custom actions with kind='mpris'".into(),
            )),
        }
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        let rx = self.tx.subscribe();
        Some(
            BroadcastStream::new(rx)
                .filter_map(|res| async move { res.ok() })
                .boxed(),
        )
    }
}

pub(crate) fn mpris_method_for_command(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "play-pause" => "PlayPause",
        "play" => "Play",
        "pause" => "Pause",
        "next" => "Next",
        "previous" => "Previous",
        "stop" => "Stop",
        _ => return None,
    })
}

fn parse_playback_status(s: &str) -> PlaybackStatus {
    match s {
        "Playing" => PlaybackStatus::Playing,
        "Paused" => PlaybackStatus::Paused,
        _ => PlaybackStatus::Stopped,
    }
}

fn empty_mpris_state() -> MprisState {
    MprisState {
        player_id: None,
        title: None,
        artist: None,
        album: None,
        art_url: None,
        playback_status: PlaybackStatus::Stopped,
        position_micros: None,
        length_micros: None,
    }
}

/// Extract a `String` from an `OwnedValue` representing a `s`-typed variant.
fn metadata_string(value: &OwnedValue) -> Option<String> {
    let s: &str = value.downcast_ref().ok()?;
    Some(s.to_string())
}

/// Extract the first string from an `as` (array of string) variant. MPRIS
/// stores `xesam:artist` as an array.
fn metadata_first_string_in_array(value: &OwnedValue) -> Option<String> {
    let arr: &zbus::zvariant::Array<'_> = value.downcast_ref().ok()?;
    for item in arr.iter() {
        if let Ok(s) = <&str>::try_from(item) {
            return Some(s.to_string());
        }
    }
    None
}

/// Extract an `i64` (or other integer) from a numeric variant.
fn metadata_i64(value: &OwnedValue) -> Option<i64> {
    if let Ok(v) = i64::try_from(value) {
        return Some(v);
    }
    if let Ok(v) = u64::try_from(value) {
        return i64::try_from(v).ok();
    }
    if let Ok(v) = i32::try_from(value) {
        return Some(v as i64);
    }
    if let Ok(v) = u32::try_from(value) {
        return Some(v as i64);
    }
    None
}

async fn fetch_player_state(conn: &Connection, bus_name: &str) -> zbus::Result<MprisState> {
    let owned_bus = BusName::try_from(bus_name.to_string())
        .map_err(|e| zbus::Error::Address(format!("invalid bus name {bus_name}: {e}")))?;

    let player_proxy =
        Proxy::new(conn, owned_bus, MPRIS_OBJECT_PATH, MPRIS_PLAYER_INTERFACE).await?;

    let playback_status_str: String = player_proxy
        .get_property("PlaybackStatus")
        .await
        .unwrap_or_else(|_| "Stopped".to_string());
    let playback_status = parse_playback_status(&playback_status_str);

    let metadata: HashMap<String, OwnedValue> = player_proxy
        .get_property("Metadata")
        .await
        .unwrap_or_default();

    let title = metadata.get("xesam:title").and_then(metadata_string);
    let album = metadata.get("xesam:album").and_then(metadata_string);
    let art_url = metadata.get("mpris:artUrl").and_then(metadata_string);
    let artist = metadata
        .get("xesam:artist")
        .and_then(metadata_first_string_in_array);
    let length_micros = metadata
        .get("mpris:length")
        .and_then(metadata_i64)
        .and_then(|v| u64::try_from(v).ok());

    let position_micros = if matches!(playback_status, PlaybackStatus::Playing) {
        player_proxy
            .get_property::<i64>("Position")
            .await
            .ok()
            .and_then(|v| u64::try_from(v).ok())
    } else {
        None
    };

    Ok(MprisState {
        player_id: Some(bus_name.to_string()),
        title,
        artist,
        album,
        art_url,
        playback_status,
        position_micros,
        length_micros,
    })
}

/// Selects the active player from the known set using the documented rule:
/// 1. Any `Playing` player (alphabetical tiebreak).
/// 2. Else any `Paused` player (alphabetical tiebreak).
/// 3. Else the alphabetically-first player overall.
/// 4. Else `None`.
pub(crate) fn pick_active_player(players: &HashMap<String, MprisState>) -> Option<String> {
    if players.is_empty() {
        return None;
    }

    let mut playing: Vec<&String> = players
        .iter()
        .filter(|(_, s)| matches!(s.playback_status, PlaybackStatus::Playing))
        .map(|(k, _)| k)
        .collect();
    playing.sort();
    if let Some(name) = playing.first() {
        return Some((*name).clone());
    }

    let mut paused: Vec<&String> = players
        .iter()
        .filter(|(_, s)| matches!(s.playback_status, PlaybackStatus::Paused))
        .map(|(k, _)| k)
        .collect();
    paused.sort();
    if let Some(name) = paused.first() {
        return Some((*name).clone());
    }

    let mut all: Vec<&String> = players.keys().collect();
    all.sort();
    all.first().map(|s| (*s).clone())
}

async fn refresh_all_players(
    conn: &Connection,
    players: &mut HashMap<String, MprisState>,
) -> zbus::Result<()> {
    let names: Vec<String> = players.keys().cloned().collect();
    for name in names {
        match fetch_player_state(conn, &name).await {
            Ok(state) => {
                players.insert(name, state);
            }
            Err(err) => {
                tracing::warn!("mpris: failed to refresh {name}: {err}");
                players.remove(&name);
            }
        }
    }
    Ok(())
}

async fn publish_state(
    conn: &Connection,
    players: &HashMap<String, MprisState>,
    active_player: &Arc<tokio::sync::Mutex<Option<String>>>,
    tx: &broadcast::Sender<serde_json::Value>,
    last_published: &mut Option<serde_json::Value>,
) {
    let active = pick_active_player(players);

    {
        let mut guard = active_player.lock().await;
        *guard = active.clone();
    }

    let state = match active.as_deref() {
        Some(name) => match players.get(name) {
            Some(s) => s.clone(),
            None => {
                // Active picked but state missing; refetch on demand.
                match fetch_player_state(conn, name).await {
                    Ok(s) => s,
                    Err(_) => empty_mpris_state(),
                }
            }
        },
        None => empty_mpris_state(),
    };

    match serde_json::to_value(&state) {
        Ok(value) => {
            send_state_if_changed(tx, last_published, value);
        }
        Err(err) => {
            tracing::warn!("mpris: failed to serialize state: {err}");
        }
    }
}

/// Forward `candidate` on `tx` only when it differs from `last`. Keeps the
/// broadcast channel quiet while the player state is steady, which matters
/// most when no players are running — otherwise the 1Hz tick would publish
/// the same "no player" envelope every second, waking every subscriber.
pub(crate) fn send_state_if_changed(
    tx: &broadcast::Sender<serde_json::Value>,
    last: &mut Option<serde_json::Value>,
    candidate: serde_json::Value,
) {
    if last.as_ref() == Some(&candidate) {
        return;
    }
    let _ = tx.send(candidate.clone());
    *last = Some(candidate);
}

async fn mpris_task(
    active_player: Arc<tokio::sync::Mutex<Option<String>>>,
    tx: broadcast::Sender<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conn = Connection::session().await?;
    let dbus_proxy = zbus::fdo::DBusProxy::new(&conn).await?;

    // Initial discovery: list all mpris services on the bus.
    let names = dbus_proxy.list_names().await?;
    let mut players: HashMap<String, MprisState> = HashMap::new();
    for owned in names {
        let name = owned.as_str();
        if name.starts_with(MPRIS_PREFIX) {
            match fetch_player_state(&conn, name).await {
                Ok(state) => {
                    players.insert(name.to_string(), state);
                }
                Err(err) => {
                    tracing::warn!("mpris: failed to fetch initial state for {name}: {err}");
                }
            }
        }
    }

    // `last_published` lives across the whole task: every publish path
    // routes through it, so transient state that round-trips back to the
    // same value (very common when no players are running) only emits a
    // single broadcast.
    let mut last_published: Option<serde_json::Value> = None;
    publish_state(&conn, &players, &active_player, &tx, &mut last_published).await;

    let mut name_owner_changed = dbus_proxy.receive_name_owner_changed().await?;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Drop the immediate first tick that `interval` emits.
    tick.tick().await;

    loop {
        tokio::select! {
            biased;

            maybe_signal = name_owner_changed.next() => {
                let Some(signal) = maybe_signal else {
                    // Stream closed; bail so the outer loop reconnects.
                    return Ok(());
                };
                let args = match signal.args() {
                    Ok(a) => a,
                    Err(err) => {
                        tracing::warn!("mpris: bad NameOwnerChanged args: {err}");
                        continue;
                    }
                };

                let name = args.name().to_string();
                if !name.starts_with(MPRIS_PREFIX) {
                    continue;
                }

                let new_owner_is_empty = args
                    .new_owner()
                    .as_ref()
                    .map(|n| n.as_str().is_empty())
                    .unwrap_or(true);

                if new_owner_is_empty {
                    players.remove(&name);
                } else {
                    match fetch_player_state(&conn, &name).await {
                        Ok(state) => {
                            players.insert(name.clone(), state);
                        }
                        Err(err) => {
                            tracing::warn!("mpris: failed to fetch state for new player {name}: {err}");
                        }
                    }
                }

                publish_state(&conn, &players, &active_player, &tx, &mut last_published).await;
            }

            _ = tick.tick() => {
                // No known players means there is nothing to refresh and the
                // empty state was already published on the previous transition
                // into emptiness. Skipping the entire branch keeps the daemon
                // idle until NameOwnerChanged fires for a new player. The
                // tick keeps running so we resume immediately on that signal.
                if players.is_empty() {
                    continue;
                }
                // Refresh all known players. This both updates position for
                // playing tracks and picks up property changes without an
                // explicit PropertiesChanged subscription.
                if let Err(err) = refresh_all_players(&conn, &mut players).await {
                    tracing::warn!("mpris: refresh error: {err}");
                }
                publish_state(&conn, &players, &active_player, &tx, &mut last_published).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_status(status: PlaybackStatus) -> MprisState {
        MprisState {
            player_id: None,
            title: None,
            artist: None,
            album: None,
            art_url: None,
            playback_status: status,
            position_micros: None,
            length_micros: None,
        }
    }

    #[test]
    fn maps_known_commands_to_methods() {
        assert_eq!(mpris_method_for_command("play-pause"), Some("PlayPause"));
        assert_eq!(mpris_method_for_command("play"), Some("Play"));
        assert_eq!(mpris_method_for_command("pause"), Some("Pause"));
        assert_eq!(mpris_method_for_command("next"), Some("Next"));
        assert_eq!(mpris_method_for_command("previous"), Some("Previous"));
        assert_eq!(mpris_method_for_command("stop"), Some("Stop"));
    }

    #[test]
    fn unknown_command_returns_none() {
        assert_eq!(mpris_method_for_command("rewind"), None);
        assert_eq!(mpris_method_for_command(""), None);
    }

    #[test]
    fn parse_playback_status_handles_known_and_unknown() {
        assert_eq!(parse_playback_status("Playing"), PlaybackStatus::Playing);
        assert_eq!(parse_playback_status("Paused"), PlaybackStatus::Paused);
        assert_eq!(parse_playback_status("Stopped"), PlaybackStatus::Stopped);
        assert_eq!(parse_playback_status("gibberish"), PlaybackStatus::Stopped);
    }

    #[test]
    fn picks_playing_over_paused() {
        let mut players = HashMap::new();
        players.insert(
            "org.mpris.MediaPlayer2.spotify".to_string(),
            state_with_status(PlaybackStatus::Paused),
        );
        players.insert(
            "org.mpris.MediaPlayer2.mpv".to_string(),
            state_with_status(PlaybackStatus::Playing),
        );

        assert_eq!(
            pick_active_player(&players),
            Some("org.mpris.MediaPlayer2.mpv".to_string())
        );
    }

    #[test]
    fn picks_paused_when_no_playing() {
        let mut players = HashMap::new();
        players.insert(
            "org.mpris.MediaPlayer2.spotify".to_string(),
            state_with_status(PlaybackStatus::Paused),
        );
        players.insert(
            "org.mpris.MediaPlayer2.mpv".to_string(),
            state_with_status(PlaybackStatus::Paused),
        );

        let picked = pick_active_player(&players);
        assert!(picked.is_some());
        let name = picked.unwrap();
        assert!(
            name == "org.mpris.MediaPlayer2.spotify" || name == "org.mpris.MediaPlayer2.mpv",
            "unexpected pick: {name}"
        );
        // Alphabetical tiebreak: mpv < spotify.
        assert_eq!(name, "org.mpris.MediaPlayer2.mpv");
    }

    #[test]
    fn picks_alphabetically_first_when_none_playing_or_paused() {
        let mut players = HashMap::new();
        players.insert(
            "org.mpris.MediaPlayer2.spotify".to_string(),
            state_with_status(PlaybackStatus::Stopped),
        );
        players.insert(
            "org.mpris.MediaPlayer2.mpv".to_string(),
            state_with_status(PlaybackStatus::Stopped),
        );
        players.insert(
            "org.mpris.MediaPlayer2.firefox".to_string(),
            state_with_status(PlaybackStatus::Stopped),
        );

        assert_eq!(
            pick_active_player(&players),
            Some("org.mpris.MediaPlayer2.firefox".to_string())
        );
    }

    #[test]
    fn returns_none_when_empty() {
        let players: HashMap<String, MprisState> = HashMap::new();
        assert_eq!(pick_active_player(&players), None);
    }

    #[tokio::test]
    async fn send_state_if_changed_suppresses_duplicate() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let mut last: Option<serde_json::Value> = None;
        let payload = serde_json::json!({"player_id": null, "playback_status": "Stopped"});

        send_state_if_changed(&tx, &mut last, payload.clone());
        send_state_if_changed(&tx, &mut last, payload.clone());

        assert_eq!(rx.try_recv().expect("first send"), payload);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn send_state_if_changed_forwards_distinct_payload() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let mut last: Option<serde_json::Value> = None;
        let a = serde_json::json!({"player_id": null});
        let b = serde_json::json!({"player_id": "spotify"});

        send_state_if_changed(&tx, &mut last, a.clone());
        send_state_if_changed(&tx, &mut last, b.clone());

        assert_eq!(rx.try_recv().expect("first"), a);
        assert_eq!(rx.try_recv().expect("second"), b);
    }

    #[tokio::test]
    async fn no_players_steady_state_yields_zero_broadcasts_after_first() {
        // Simulates three consecutive empty-state ticks: the first
        // publishes the empty state, the next two are deduped. This is
        // the steady-state battery saver: while no player is around,
        // the tick loop must NOT keep emitting the same empty payload.
        let (tx, mut rx) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let mut last: Option<serde_json::Value> = None;
        let empty = serde_json::to_value(empty_mpris_state()).expect("serialize empty");

        for _ in 0..3 {
            send_state_if_changed(&tx, &mut last, empty.clone());
        }

        // Exactly one broadcast total (the very first one).
        assert_eq!(rx.try_recv().expect("first publish"), empty);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
}
