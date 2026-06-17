//! System implementation of the domain `Clock` port using chrono.

use chrono::{Datelike, Local, Timelike, Utc};
use quantum_domain::{CivilNow, Clock, Weekday};

/// Wall-clock implementation of the domain `Clock` port backed by the system clock.
pub struct SystemClock;

impl SystemClock {
    /// Construct a new `SystemClock`.
    pub fn new() -> Self {
        SystemClock
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        SystemClock::new()
    }
}

impl Clock for SystemClock {
    fn now_unix(&self) -> u64 {
        Utc::now().timestamp() as u64
    }

    fn local_civil(&self) -> CivilNow {
        // Capture the local time once so weekday and time-of-day stay consistent.
        let now = Local::now();
        // `num_days_from_monday()` is always in 0..=6, so `from_index` never
        // returns None; fall back to Monday rather than unwrapping.
        let weekday = Weekday::from_index(now.weekday().num_days_from_monday() as u8)
            .unwrap_or(Weekday::Monday);
        let secs_into_day = now.hour() * 3600 + now.minute() * 60 + now.second();
        CivilNow {
            weekday,
            secs_into_day,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_unix_returns_plausible_epoch() {
        let clock = SystemClock::new();
        assert!(clock.now_unix() > 1_700_000_000);
    }

    #[test]
    fn local_civil_is_within_a_day() {
        let clock = SystemClock::new();
        let civil = clock.local_civil();
        assert!(civil.secs_into_day < 86_400);
    }
}
