//! Timer value types for the domain layer.
//! No imports from other workspace crates and no IO.

use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Errors produced by timer value types. Expanded by later tasks.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimerError {
    #[error("invalid time: {0}")]
    InvalidTime(String),
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
        let json = serde_json::to_value(&time).unwrap();
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
}
