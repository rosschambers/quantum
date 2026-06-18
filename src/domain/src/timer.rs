//! Timer value types for the domain layer.
//! No imports from other workspace crates and no IO.

use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Errors produced by timer value types and operations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TimerError {
    #[error("invalid time: {0}")]
    InvalidTime(String),
    #[error("recurring timer needs at least one weekday")]
    EmptyWeekdaySet,
    #[error("timer not found: {0}")]
    NotFound(String),
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("invalid duration: {0}")]
    InvalidDuration(String),
}

/// A day of the week. Serializes as a lowercase name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    /// Map a zero-based index (`0 = Monday .. 6 = Sunday`) to a weekday.
    /// Returns `None` for indices `>= 7`.
    pub fn from_index(index: u8) -> Option<Weekday> {
        match index {
            0 => Some(Weekday::Monday),
            1 => Some(Weekday::Tuesday),
            2 => Some(Weekday::Wednesday),
            3 => Some(Weekday::Thursday),
            4 => Some(Weekday::Friday),
            5 => Some(Weekday::Saturday),
            6 => Some(Weekday::Sunday),
            _ => None,
        }
    }

    /// The zero-based index of this weekday (`Monday = 0 .. Sunday = 6`).
    pub fn index(self) -> u8 {
        match self {
            Weekday::Monday => 0,
            Weekday::Tuesday => 1,
            Weekday::Wednesday => 2,
            Weekday::Thursday => 3,
            Weekday::Friday => 4,
            Weekday::Saturday => 5,
            Weekday::Sunday => 6,
        }
    }
}

/// A set of weekdays stored as a 7-bit mask (bit `i` set means index `i` is in
/// the set). Serializes as a JSON array of lowercase weekday names in
/// weekday-index order (Monday..Sunday).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekdaySet(u8);

impl WeekdaySet {
    /// A set containing all seven days.
    pub fn all() -> WeekdaySet {
        WeekdaySet(0b0111_1111)
    }

    /// Build a set from a slice of days. Duplicates are collapsed.
    pub fn from_days(days: &[Weekday]) -> WeekdaySet {
        let mut mask = 0u8;
        for day in days {
            mask |= 1 << day.index();
        }
        WeekdaySet(mask)
    }

    /// Whether the given day is in the set.
    pub fn contains(self, day: Weekday) -> bool {
        self.0 & (1 << day.index()) != 0
    }

    /// Whether the set contains no days.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Iterate the contained days in weekday-index order (Monday..Sunday).
    fn days(self) -> impl Iterator<Item = Weekday> {
        (0u8..7).filter_map(move |index| {
            let day = Weekday::from_index(index)?;
            self.contains(day).then_some(day)
        })
    }
}

impl Serialize for WeekdaySet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.count_ones() as usize))?;
        for day in self.days() {
            seq.serialize_element(&day)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for WeekdaySet {
    fn deserialize<D>(deserializer: D) -> Result<WeekdaySet, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WeekdaySetVisitor;

        impl<'de> Visitor<'de> for WeekdaySetVisitor {
            type Value = WeekdaySet;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an array of lowercase weekday names")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<WeekdaySet, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut mask = 0u8;
                while let Some(day) = seq.next_element::<Weekday>()? {
                    mask |= 1 << day.index();
                }
                Ok(WeekdaySet(mask))
            }
        }

        deserializer.deserialize_seq(WeekdaySetVisitor)
    }
}

/// A wall-clock time of day with no date or timezone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TimeOfDay {
    pub hour: u8,
    pub minute: u8,
}

impl<'de> Deserialize<'de> for TimeOfDay {
    fn deserialize<D>(deserializer: D) -> Result<TimeOfDay, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Shadow {
            hour: u8,
            minute: u8,
        }

        let shadow = Shadow::deserialize(deserializer)?;
        TimeOfDay::new(shadow.hour, shadow.minute).map_err(de::Error::custom)
    }
}

impl TimeOfDay {
    /// Construct a valid time of day. Rejects `hour > 23` or `minute > 59`.
    pub fn new(hour: u8, minute: u8) -> Result<TimeOfDay, TimerError> {
        if hour > 23 || minute > 59 {
            return Err(TimerError::InvalidTime(format!("{hour:02}:{minute:02}")));
        }
        Ok(TimeOfDay { hour, minute })
    }

    /// Seconds elapsed since midnight for this time of day.
    pub fn secs_into_day(self) -> u32 {
        self.hour as u32 * 3600 + self.minute as u32 * 60
    }
}

/// Seconds from "now" until the next instant whose weekday is in `days` and
/// whose time-of-day equals `target`. Pure integer calendar arithmetic with no
/// dependency on a real clock or timezone.
///
/// "Now" is described by `now_weekday` and `now_secs_into_day` (seconds elapsed
/// since midnight today). Returns `None` when `days` is empty. Never returns 0:
/// a target exactly equal to now rolls forward to the next occurrence, up to a
/// full week (7 days) away when only today's weekday is included.
pub fn seconds_until_next(
    now_weekday: Weekday,
    now_secs_into_day: u32,
    days: &WeekdaySet,
    target: TimeOfDay,
) -> Option<u64> {
    if days.is_empty() {
        return None;
    }

    let target_secs = target.secs_into_day();
    for day_offset in 0u64..=7 {
        let candidate_index = (now_weekday.index() as u64 + day_offset) % 7;
        let candidate = Weekday::from_index(candidate_index as u8)?;
        if !days.contains(candidate) {
            continue;
        }
        if day_offset == 0 {
            if target_secs > now_secs_into_day {
                return Some((target_secs - now_secs_into_day) as u64);
            }
            continue;
        }
        return Some(day_offset * 86400 + target_secs as u64 - now_secs_into_day as u64);
    }

    None
}

/// The visual rendering style of a timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VisualStyle {
    Ring,
    Wedge,
    Pie,
    Dots,
    Bar,
    Spiral,
    Pulse,
    #[default]
    Mixed,
}

/// When a piece of text on a timer is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextVisibility {
    Always,
    Hover,
    Hidden,
}

/// Where text is positioned relative to a timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextPosition {
    Below,
    Above,
    Center,
}

/// The color treatment applied to timer text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextColor {
    Accent,
    White,
    Muted,
}

/// The color of the outline that hugs a timer's filling portion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FillBorderColor {
    #[default]
    Dark,
    Light,
    Accent,
}

/// How the remaining time is formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeFormat {
    Clock,
    Compact,
    Percent,
}

/// A named completion sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SoundName {
    #[default]
    Complete,
    Bell,
    Chime,
    Alarm,
}

/// Visual configuration for a timer. `#[serde(default)]` at the struct level
/// fills any missing field from the manual `Default` impl, so partial JSON
/// (including `{}`) deserializes to the full set of spec defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VisualConfig {
    pub style: VisualStyle,
    pub size: u32,
    pub thickness: u32,
    pub fill: bool,
    pub reverse: bool,
    pub accent_hue: Option<u16>,
    pub track_opacity: u8,
    pub label_visibility: TextVisibility,
    pub time_visibility: TextVisibility,
    pub text_position: TextPosition,
    pub text_color: TextColor,
    pub time_format: TimeFormat,
    pub font_scale: u16,
    pub font_weight: u16,
    pub uppercase: bool,
    pub gradient_stroke: bool,
    pub fill_border: bool,
    pub fill_border_width: u32,
    pub fill_border_color: FillBorderColor,
    pub depth_sheen: bool,
}

impl Default for VisualConfig {
    fn default() -> VisualConfig {
        VisualConfig {
            style: VisualStyle::Ring,
            size: 100,
            thickness: 20,
            fill: true,
            reverse: true,
            accent_hue: None,
            track_opacity: 0,
            label_visibility: TextVisibility::Hover,
            time_visibility: TextVisibility::Hover,
            text_position: TextPosition::Center,
            text_color: TextColor::Muted,
            time_format: TimeFormat::Compact,
            font_scale: 100,
            font_weight: 600,
            uppercase: true,
            gradient_stroke: true,
            fill_border: true,
            fill_border_width: 1,
            fill_border_color: FillBorderColor::Dark,
            depth_sheen: false,
        }
    }
}

/// Notification configuration for a timer. `#[serde(default)]` at the struct
/// level fills any missing field from the manual `Default` impl, so partial
/// JSON (including `{}`) deserializes to the full set of spec defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifyConfig {
    pub notification: bool,
    pub sound: Option<SoundName>,
    pub urgency_ramp: bool,
    pub ramp_threshold: u8,
    pub pulse: bool,
    pub flash: bool,
}

impl Default for NotifyConfig {
    fn default() -> NotifyConfig {
        NotifyConfig {
            notification: true,
            sound: None,
            urgency_ramp: true,
            ramp_threshold: 20,
            pulse: true,
            flash: true,
        }
    }
}

/// Stable identifier for a timer. Serializes transparently as its inner string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TimerId(String);

impl TimerId {
    /// Borrow the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for TimerId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for TimerId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl fmt::Display for TimerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lifecycle status of a timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TimerStatus {
    #[default]
    Active,
    Expired,
}

/// A 2D point. Used for scattered placement of timers. Cannot derive `Eq`
/// because the coordinates are `f64`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// What kind of timer this is and when it fires. Serialized internally tagged
/// on a `type` field in snake_case (`one_shot` / `recurring`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimerKind {
    OneShot {
        end_unix: u64,
    },
    Recurring {
        days: WeekdaySet,
        time: TimeOfDay,
        next_fire_unix: u64,
    },
}

/// A configured timer: identity, label, kind, and its visual and notification
/// settings, plus runtime status and optional scattered position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timer {
    pub id: TimerId,
    pub label: String,
    pub kind: TimerKind,
    pub visual: VisualConfig,
    pub notify: NotifyConfig,
    #[serde(default)]
    pub status: TimerStatus,
    #[serde(default)]
    pub scatter_pos: Option<Point>,
}

impl Timer {
    /// The Unix timestamp at which this timer next fires: `end_unix` for a
    /// one-shot timer, `next_fire_unix` for a recurring timer.
    pub fn fires_at_unix(&self) -> u64 {
        match &self.kind {
            TimerKind::OneShot { end_unix } => *end_unix,
            TimerKind::Recurring { next_fire_unix, .. } => *next_fire_unix,
        }
    }
}

/// The current civil ("wall clock") moment as a weekday plus seconds elapsed
/// since midnight. Carries no date or timezone: it is the calendar projection
/// of "now" used to drive recurring-timer arithmetic via `seconds_until_next`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CivilNow {
    pub weekday: Weekday,
    pub secs_into_day: u32,
}

/// Subsystem-wide timer settings: how timers are laid out and the default
/// visual and notification configuration applied to new timers.
/// `#[serde(default)]` at the struct level fills any missing field from the
/// manual `Default` impl, so partial JSON (including `{}`) deserializes to the
/// full set of spec defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimerSettings {
    pub layout: String,
    pub gap: u32,
    pub align: String,
    pub defaults_visual: VisualConfig,
    pub defaults_notify: NotifyConfig,
}

impl Default for TimerSettings {
    fn default() -> TimerSettings {
        TimerSettings {
            layout: "scatter".to_string(),
            gap: 24,
            align: "top_right".to_string(),
            defaults_visual: VisualConfig::default(),
            defaults_notify: NotifyConfig::default(),
        }
    }
}

/// The full persisted state of the timer subsystem: settings plus the list of
/// configured timers. `#[serde(default)]` lets partial JSON (including `{}`)
/// deserialize to an empty timer list with default settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TimerStoreData {
    pub settings: TimerSettings,
    pub timers: Vec<Timer>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekday_index_roundtrip() {
        for i in 0..=6 {
            assert_eq!(Weekday::from_index(i).unwrap().index(), i);
        }
        assert!(Weekday::from_index(7).is_none());
    }

    #[test]
    fn weekdayset_serializes_as_name_array() {
        let set = WeekdaySet::from_days(&[Weekday::Monday, Weekday::Thursday]);
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"["monday","thursday"]"#);
        let back: WeekdaySet = serde_json::from_str(&json).unwrap();
        assert!(back.contains(Weekday::Monday) && back.contains(Weekday::Thursday));
        assert!(!back.contains(Weekday::Tuesday));
    }

    #[test]
    fn timeofday_validates_and_computes_seconds() {
        assert!(TimeOfDay::new(24, 0).is_err());
        assert!(TimeOfDay::new(10, 60).is_err());
        assert_eq!(TimeOfDay::new(1, 30).unwrap().secs_into_day(), 5400);
    }

    #[test]
    fn weekday_serde_round_trips() {
        let day = Weekday::Wednesday;
        let json = serde_json::to_string(&day).unwrap();
        assert_eq!(json, "\"wednesday\"");
        let back: Weekday = serde_json::from_str(&json).unwrap();
        assert_eq!(back, day);
    }

    #[test]
    fn weekdayset_round_trips_all() {
        let all = WeekdaySet::all();
        let json = serde_json::to_string(&all).unwrap();
        let back: WeekdaySet = serde_json::from_str(&json).unwrap();
        assert_eq!(back, all);
        assert!(!all.is_empty());
    }

    #[test]
    fn weekdayset_empty() {
        let empty = WeekdaySet::from_days(&[]);
        assert!(empty.is_empty());
        let json = serde_json::to_string(&empty).unwrap();
        assert_eq!(json, "[]");
        let back: WeekdaySet = serde_json::from_str(&json).unwrap();
        assert_eq!(back, empty);
    }

    #[test]
    fn timeofday_serde_round_trips() {
        let time = TimeOfDay::new(7, 45).unwrap();
        let json = serde_json::to_value(time).unwrap();
        let back: TimeOfDay = serde_json::from_value(json).unwrap();
        assert_eq!(back, time);
    }

    #[test]
    fn timeofday_deserialize_rejects_out_of_range() {
        assert!(serde_json::from_str::<TimeOfDay>(r#"{"hour":25,"minute":0}"#).is_err());
        assert!(serde_json::from_str::<TimeOfDay>(r#"{"hour":10,"minute":60}"#).is_err());
        // valid still works:
        let t: TimeOfDay = serde_json::from_str(r#"{"hour":17,"minute":15}"#).unwrap();
        assert_eq!(t.secs_into_day(), 17 * 3600 + 15 * 60);
    }

    #[test]
    fn weekdayset_from_days_collapses_duplicates() {
        let doubled = WeekdaySet::from_days(&[Weekday::Monday, Weekday::Monday]);
        let single = WeekdaySet::from_days(&[Weekday::Monday]);
        assert_eq!(doubled, single);
        assert!(doubled.contains(Weekday::Monday));
    }

    #[test]
    fn weekdayset_deserialize_canonicalizes_order() {
        let set: WeekdaySet = serde_json::from_str(r#"["thursday","monday"]"#).unwrap();
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"["monday","thursday"]"#);
    }

    #[test]
    fn weekdayset_deserialize_rejects_unknown_day() {
        assert!(serde_json::from_str::<WeekdaySet>(r#"["funday"]"#).is_err());
    }

    #[test]
    fn timeofday_boundary_values_are_valid() {
        assert!(TimeOfDay::new(23, 59).is_ok());
        assert!(TimeOfDay::new(0, 0).is_ok());
    }

    #[test]
    fn same_day_future_target() {
        let r = seconds_until_next(
            Weekday::Monday,
            9 * 3600,
            &WeekdaySet::all(),
            TimeOfDay::new(17, 0).unwrap(),
        );
        assert_eq!(r, Some(8 * 3600));
    }

    #[test]
    fn rolls_to_next_included_day_when_passed() {
        let days = WeekdaySet::from_days(&[Weekday::Tuesday, Weekday::Thursday]);
        let r = seconds_until_next(
            Weekday::Tuesday,
            18 * 3600,
            &days,
            TimeOfDay::new(18, 0).unwrap(),
        );
        assert_eq!(r, Some(2 * 24 * 3600));
    }

    #[test]
    fn wraps_across_week() {
        let days = WeekdaySet::from_days(&[Weekday::Monday]);
        let r = seconds_until_next(
            Weekday::Sunday,
            12 * 3600,
            &days,
            TimeOfDay::new(9, 0).unwrap(),
        );
        assert_eq!(r, Some(21 * 3600));
    }

    #[test]
    fn empty_set_is_none() {
        assert_eq!(
            seconds_until_next(
                Weekday::Monday,
                0,
                &WeekdaySet::from_days(&[]),
                TimeOfDay::new(9, 0).unwrap()
            ),
            None
        );
    }

    #[test]
    fn only_today_passed_rolls_full_week() {
        let days = WeekdaySet::from_days(&[Weekday::Monday]);
        // Monday 10:00 now, target 09:00 Monday -> next Monday, 7 days minus 1 hour
        let r = seconds_until_next(
            Weekday::Monday,
            10 * 3600,
            &days,
            TimeOfDay::new(9, 0).unwrap(),
        );
        assert_eq!(r, Some(7 * 86400 - 3600));
    }

    #[test]
    fn enum_defaults_match_spec() {
        assert_eq!(VisualStyle::default(), VisualStyle::Mixed);
        assert_eq!(SoundName::default(), SoundName::Complete);
    }

    #[test]
    fn enums_serialize_lowercase() {
        assert_eq!(
            serde_json::to_string(&VisualStyle::Ring).unwrap(),
            "\"ring\""
        );
        assert_eq!(
            serde_json::to_string(&TextVisibility::Hover).unwrap(),
            "\"hover\""
        );
        assert_eq!(
            serde_json::to_string(&TextPosition::Center).unwrap(),
            "\"center\""
        );
        assert_eq!(
            serde_json::to_string(&TextColor::Accent).unwrap(),
            "\"accent\""
        );
        assert_eq!(
            serde_json::to_string(&TimeFormat::Compact).unwrap(),
            "\"compact\""
        );
        assert_eq!(serde_json::to_string(&SoundName::Bell).unwrap(), "\"bell\"");
    }

    #[test]
    fn visual_style_spiral_and_pulse_serialize() {
        assert_eq!(
            serde_json::to_string(&VisualStyle::Spiral).unwrap(),
            "\"spiral\""
        );
        assert_eq!(
            serde_json::to_string(&VisualStyle::Pulse).unwrap(),
            "\"pulse\""
        );
        let back: VisualStyle = serde_json::from_str("\"pulse\"").unwrap();
        assert_eq!(back, VisualStyle::Pulse);
    }

    #[test]
    fn visual_config_default_values_match_spec() {
        let config = VisualConfig::default();
        assert_eq!(config.style, VisualStyle::Ring);
        assert_eq!(config.size, 100);
        assert_eq!(config.thickness, 20);
        assert!(config.fill);
        assert!(config.reverse);
        assert_eq!(config.accent_hue, None);
        assert_eq!(config.track_opacity, 0);
        assert_eq!(config.label_visibility, TextVisibility::Hover);
        assert_eq!(config.time_visibility, TextVisibility::Hover);
        assert_eq!(config.text_position, TextPosition::Center);
        assert_eq!(config.text_color, TextColor::Muted);
        assert_eq!(config.time_format, TimeFormat::Compact);
        assert_eq!(config.font_scale, 100);
        assert_eq!(config.font_weight, 600);
        assert!(config.uppercase);
        assert!(config.gradient_stroke);
        assert!(config.fill_border);
        assert_eq!(config.fill_border_width, 1);
        assert_eq!(config.fill_border_color, FillBorderColor::Dark);
        assert!(!config.depth_sheen);
    }

    #[test]
    fn fill_border_color_serializes_lowercase_and_defaults_dark() {
        assert_eq!(FillBorderColor::default(), FillBorderColor::Dark);
        assert_eq!(
            serde_json::to_string(&FillBorderColor::Light).unwrap(),
            "\"light\""
        );
    }

    #[test]
    fn accent_hue_none_is_default_and_some_round_trips() {
        assert_eq!(VisualConfig::default().accent_hue, None);
        let json = r#"{"accent_hue": 151}"#;
        let cfg: VisualConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.accent_hue, Some(151));
    }

    #[test]
    fn visual_config_empty_json_is_full_default() {
        let from_empty: VisualConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(from_empty, VisualConfig::default());
    }

    #[test]
    fn visual_config_partial_json_keeps_other_defaults() {
        let partial: VisualConfig = serde_json::from_str(r#"{"size": 90}"#).unwrap();
        assert_eq!(partial.size, 90);
        let default = VisualConfig::default();
        assert_eq!(partial.style, default.style);
        assert_eq!(partial.thickness, default.thickness);
        assert_eq!(partial.fill, default.fill);
        assert_eq!(partial.reverse, default.reverse);
        assert_eq!(partial.accent_hue, default.accent_hue);
        assert_eq!(partial.track_opacity, default.track_opacity);
        assert_eq!(partial.label_visibility, default.label_visibility);
        assert_eq!(partial.time_visibility, default.time_visibility);
        assert_eq!(partial.text_position, default.text_position);
        assert_eq!(partial.text_color, default.text_color);
        assert_eq!(partial.time_format, default.time_format);
        assert_eq!(partial.font_scale, default.font_scale);
        assert_eq!(partial.font_weight, default.font_weight);
        assert_eq!(partial.uppercase, default.uppercase);
        assert_eq!(partial.gradient_stroke, default.gradient_stroke);
        assert_eq!(partial.fill_border, default.fill_border);
        assert_eq!(partial.fill_border_width, default.fill_border_width);
        assert_eq!(partial.fill_border_color, default.fill_border_color);
        assert_eq!(partial.depth_sheen, default.depth_sheen);
    }

    #[test]
    fn visual_config_round_trips() {
        let config = VisualConfig {
            style: VisualStyle::Bar,
            size: 200,
            thickness: 20,
            fill: false,
            reverse: false,
            accent_hue: Some(300),
            track_opacity: 50,
            label_visibility: TextVisibility::Always,
            time_visibility: TextVisibility::Hidden,
            text_position: TextPosition::Below,
            text_color: TextColor::White,
            time_format: TimeFormat::Clock,
            font_scale: 120,
            font_weight: 700,
            uppercase: false,
            gradient_stroke: false,
            fill_border: false,
            fill_border_width: 3,
            fill_border_color: FillBorderColor::Accent,
            depth_sheen: true,
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: VisualConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn notify_config_default_values_match_spec() {
        let config = NotifyConfig::default();
        assert!(config.notification);
        assert_eq!(config.sound, None);
        assert!(config.urgency_ramp);
        assert_eq!(config.ramp_threshold, 20);
        assert!(config.pulse);
        assert!(config.flash);
    }

    #[test]
    fn notify_config_empty_json_is_full_default() {
        let from_empty: NotifyConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(from_empty, NotifyConfig::default());
    }

    #[test]
    fn notify_config_partial_json_keeps_other_defaults() {
        let partial: NotifyConfig = serde_json::from_str(r#"{"ramp_threshold": 5}"#).unwrap();
        assert_eq!(partial.ramp_threshold, 5);
        let default = NotifyConfig::default();
        assert_eq!(partial.notification, default.notification);
        assert_eq!(partial.sound, default.sound);
        assert_eq!(partial.urgency_ramp, default.urgency_ramp);
        assert_eq!(partial.pulse, default.pulse);
        assert_eq!(partial.flash, default.flash);
    }

    #[test]
    fn notify_config_round_trips() {
        let config = NotifyConfig {
            notification: false,
            sound: Some(SoundName::Chime),
            urgency_ramp: false,
            ramp_threshold: 80,
            pulse: false,
            flash: false,
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: NotifyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn timer_id_from_str_and_string_equal() {
        assert_eq!(TimerId::from("t1"), TimerId::from("t1".to_string()));
    }

    #[test]
    fn timer_id_as_str_and_display() {
        let id = TimerId::from("morning");
        assert_eq!(id.as_str(), "morning");
        assert_eq!(id.to_string(), "morning");
    }

    #[test]
    fn timer_id_serde_transparent() {
        let id = TimerId::from("abc");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"abc\"");
        let back: TimerId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn timer_status_default_is_active() {
        assert_eq!(TimerStatus::default(), TimerStatus::Active);
    }

    #[test]
    fn timer_status_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&TimerStatus::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&TimerStatus::Expired).unwrap(),
            "\"expired\""
        );
    }

    #[test]
    fn point_round_trips() {
        let point = Point { x: 1.5, y: -2.25 };
        let json = serde_json::to_string(&point).unwrap();
        let back: Point = serde_json::from_str(&json).unwrap();
        assert_eq!(back, point);
    }

    #[test]
    fn timer_kind_one_shot_serializes_with_tag() {
        let kind = TimerKind::OneShot { end_unix: 1000 };
        let value = serde_json::to_value(&kind).unwrap();
        assert_eq!(value["type"], "one_shot");
        assert_eq!(value["end_unix"], 1000);
    }

    #[test]
    fn timer_kind_recurring_serializes_with_tag() {
        let kind = TimerKind::Recurring {
            days: WeekdaySet::from_days(&[Weekday::Monday]),
            time: TimeOfDay::new(8, 30).unwrap(),
            next_fire_unix: 2000,
        };
        let value = serde_json::to_value(&kind).unwrap();
        assert_eq!(value["type"], "recurring");
        assert_eq!(value["days"], serde_json::json!(["monday"]));
        assert_eq!(value["time"], serde_json::json!({"hour": 8, "minute": 30}));
        assert_eq!(value["next_fire_unix"], 2000);
    }

    #[test]
    fn timer_fires_at_unix_one_shot() {
        let timer = sample_one_shot();
        assert_eq!(timer.fires_at_unix(), 1700);
    }

    #[test]
    fn timer_fires_at_unix_recurring() {
        let timer = sample_recurring();
        assert_eq!(timer.fires_at_unix(), 9000);
    }

    #[test]
    fn timer_one_shot_round_trips() {
        let timer = sample_one_shot();
        let json = serde_json::to_string(&timer).unwrap();
        let back: Timer = serde_json::from_str(&json).unwrap();
        assert_eq!(back, timer);
    }

    #[test]
    fn timer_recurring_round_trips() {
        let timer = sample_recurring();
        let json = serde_json::to_string(&timer).unwrap();
        let back: Timer = serde_json::from_str(&json).unwrap();
        assert_eq!(back, timer);
    }

    #[test]
    fn timer_status_and_scatter_pos_default_when_absent() {
        let json = r#"{
            "id": "t1",
            "label": "Tea",
            "kind": {"type": "one_shot", "end_unix": 1700},
            "visual": {},
            "notify": {}
        }"#;
        let timer: Timer = serde_json::from_str(json).unwrap();
        assert_eq!(timer.status, TimerStatus::Active);
        assert_eq!(timer.scatter_pos, None);
    }

    #[test]
    fn timer_error_has_all_variants() {
        let variants = [
            TimerError::InvalidTime("x".to_string()),
            TimerError::EmptyWeekdaySet,
            TimerError::NotFound("t1".to_string()),
            TimerError::Persistence("disk".to_string()),
            TimerError::InvalidDuration("neg".to_string()),
        ];
        assert_eq!(variants.len(), 5);
        assert_eq!(
            TimerError::NotFound("t1".to_string()).to_string(),
            "timer not found: t1"
        );
        assert_eq!(
            TimerError::EmptyWeekdaySet.to_string(),
            "recurring timer needs at least one weekday"
        );
    }

    #[test]
    fn timer_settings_default_values_match_spec() {
        let settings = TimerSettings::default();
        assert_eq!(settings.layout, "scatter");
        assert_eq!(settings.gap, 24);
        assert_eq!(settings.align, "top_right");
        assert_eq!(settings.defaults_visual, VisualConfig::default());
        assert_eq!(settings.defaults_notify, NotifyConfig::default());
    }

    #[test]
    fn timer_settings_empty_json_is_full_default() {
        let from_empty: TimerSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(from_empty, TimerSettings::default());
    }

    #[test]
    fn timer_settings_partial_json_keeps_other_defaults() {
        let partial: TimerSettings = serde_json::from_str(r#"{"gap": 8}"#).unwrap();
        assert_eq!(partial.gap, 8);
        let default = TimerSettings::default();
        assert_eq!(partial.layout, default.layout);
        assert_eq!(partial.align, default.align);
        assert_eq!(partial.defaults_visual, default.defaults_visual);
        assert_eq!(partial.defaults_notify, default.defaults_notify);
    }

    #[test]
    fn timer_settings_round_trips() {
        let settings = TimerSettings {
            layout: "grid".to_string(),
            gap: 40,
            align: "bottom_left".to_string(),
            defaults_visual: VisualConfig {
                size: 99,
                ..VisualConfig::default()
            },
            defaults_notify: NotifyConfig {
                ramp_threshold: 7,
                ..NotifyConfig::default()
            },
        };
        let json = serde_json::to_string(&settings).unwrap();
        let back: TimerSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn timer_store_data_default_is_empty_timers_and_default_settings() {
        let data = TimerStoreData::default();
        assert!(data.timers.is_empty());
        assert_eq!(data.settings, TimerSettings::default());
    }

    #[test]
    fn timer_store_data_empty_json_is_full_default() {
        let from_empty: TimerStoreData = serde_json::from_str("{}").unwrap();
        assert_eq!(from_empty, TimerStoreData::default());
    }

    #[test]
    fn timer_store_data_round_trips_with_one_timer() {
        let data = TimerStoreData {
            settings: TimerSettings::default(),
            timers: vec![sample_one_shot()],
        };
        let json = serde_json::to_string(&data).unwrap();
        let back: TimerStoreData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, data);
        assert_eq!(back.timers.len(), 1);
    }

    #[test]
    fn civil_now_is_copy_and_holds_fields() {
        let now = CivilNow {
            weekday: Weekday::Wednesday,
            secs_into_day: 3600,
        };
        let copy = now;
        assert_eq!(copy, now);
        assert_eq!(copy.weekday, Weekday::Wednesday);
        assert_eq!(copy.secs_into_day, 3600);
    }

    fn sample_one_shot() -> Timer {
        Timer {
            id: TimerId::from("t1"),
            label: "Tea".to_string(),
            kind: TimerKind::OneShot { end_unix: 1700 },
            visual: VisualConfig::default(),
            notify: NotifyConfig::default(),
            status: TimerStatus::Active,
            scatter_pos: Some(Point { x: 12.0, y: 34.0 }),
        }
    }

    fn sample_recurring() -> Timer {
        Timer {
            id: TimerId::from("t2"),
            label: "Standup".to_string(),
            kind: TimerKind::Recurring {
                days: WeekdaySet::from_days(&[Weekday::Monday, Weekday::Friday]),
                time: TimeOfDay::new(9, 0).unwrap(),
                next_fire_unix: 9000,
            },
            visual: VisualConfig::default(),
            notify: NotifyConfig::default(),
            status: TimerStatus::Expired,
            scatter_pos: None,
        }
    }
}
