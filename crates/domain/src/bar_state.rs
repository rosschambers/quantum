use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemStats {
    pub cpu_percent: f32,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MprisState {
    pub player_id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub art_url: Option<String>,
    pub playback_status: PlaybackStatus,
    pub position_micros: Option<u64>,
    pub length_micros: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveWindowState {
    pub title: String,
    pub class: String,
    pub workspace_id: i64,
    pub workspace_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn system_stats_round_trips() {
        let s = SystemStats {
            cpu_percent: 12.5,
            mem_used_bytes: 1024,
            mem_total_bytes: 4096,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v,
            json!({"cpu_percent": 12.5, "mem_used_bytes": 1024, "mem_total_bytes": 4096})
        );
        let back: SystemStats = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn playback_status_serializes_snake_case() {
        let v = serde_json::to_value(PlaybackStatus::Playing).unwrap();
        assert_eq!(v, json!("playing"));
    }

    #[test]
    fn mpris_state_with_no_player_round_trips() {
        let s = MprisState {
            player_id: None,
            title: None,
            artist: None,
            album: None,
            art_url: None,
            playback_status: PlaybackStatus::Stopped,
            position_micros: None,
            length_micros: None,
        };
        let v = serde_json::to_value(&s).unwrap();
        let back: MprisState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn active_window_state_round_trips() {
        let s = ActiveWindowState {
            title: "Mozilla Firefox".into(),
            class: "firefox".into(),
            workspace_id: 1,
            workspace_name: "1".into(),
        };
        let v = serde_json::to_value(&s).unwrap();
        let back: ActiveWindowState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }
}
