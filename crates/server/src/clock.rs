//! Local time for the download hours (SPEC 6.3.2): which minute of the day it is in a time zone,
//! and the next moment the local clock shows a given minute. The zones come from the IANA
//! database bundled into the binary (`jiff`), so a container without `tzdata` works the same,
//! and a daylight-saving change moves the hours with the local clock, as the user means them.

use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use jiff::Timestamp;

/// The current unix time in seconds.
pub fn now() -> i64 {
    Timestamp::now().as_second()
}

/// The time zone called `name` (`Europe/Warsaw`, `UTC`), if there is one.
pub fn zone(name: &str) -> Option<TimeZone> {
    TimeZone::get(name.trim()).ok()
}

/// A moment as a person reads it in the log: `2026-10-16 06:00 Europe/Warsaw` (UTC when the zone
/// is unknown).
pub fn describe(at: i64, zone_name: &str) -> String {
    let Ok(ts) = Timestamp::from_second(at) else {
        return String::new();
    };
    match zone(zone_name) {
        Some(tz) => format!(
            "{} {}",
            ts.to_zoned(tz).strftime("%Y-%m-%d %H:%M"),
            zone_name.trim()
        ),
        None => ts.to_string(),
    }
}

/// The minute of the day (minutes after local midnight) at unix time `at` in `tz`.
pub fn minute_of_day(at: i64, tz: &TimeZone) -> Option<u16> {
    let zoned = Timestamp::from_second(at).ok()?.to_zoned(tz.clone());
    u16::try_from(i32::from(zoned.hour()) * 60 + i32::from(zoned.minute())).ok()
}

/// The first moment after `at` when the local clock in `tz` shows `minute` (minutes after local
/// midnight), as unix seconds. In a daylight-saving gap that minute does not exist on the day;
/// the clock then shows the next valid time (`jiff`'s "compatible" choice).
pub fn next_at(at: i64, tz: &TimeZone, minute: u16) -> Option<i64> {
    let now = Timestamp::from_second(at).ok()?.to_zoned(tz.clone());
    let (hour, min) = (
        i8::try_from(minute / 60).ok()?,
        i8::try_from(minute % 60).ok()?,
    );
    let mut date = now.date();
    for _ in 0..3 {
        let candidate = tz
            .to_zoned(DateTime::from_parts(
                date,
                jiff::civil::time(hour, min, 0, 0),
            ))
            .ok()?;
        let ts = candidate.timestamp().as_second();
        if ts > at {
            return Some(ts);
        }
        date = date.tomorrow().ok()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> i64 {
        s.parse::<Timestamp>().expect("timestamp").as_second()
    }

    #[test]
    fn a_moment_is_described_in_its_zone() {
        assert_eq!(
            describe(ts("2026-01-10T21:00:00Z"), "Europe/Warsaw"),
            "2026-01-10 22:00 Europe/Warsaw"
        );
        assert_eq!(
            describe(ts("2026-01-10T21:00:00Z"), "Mars/Olympus"),
            "2026-01-10T21:00:00Z"
        );
    }

    #[test]
    fn zones_are_known_or_not() {
        assert!(zone("Europe/Warsaw").is_some());
        assert!(zone(" UTC ").is_some());
        assert!(zone("Mars/Olympus").is_none());
        assert!(zone("").is_none());
    }

    #[test]
    fn the_minute_of_the_day_follows_the_zone() {
        let warsaw = zone("Europe/Warsaw").expect("zone");
        // 23:30 UTC in January is 00:30 the next day in Warsaw (UTC+1)...
        assert_eq!(minute_of_day(ts("2026-01-10T23:30:00Z"), &warsaw), Some(30));
        // ...and 01:30 in July (UTC+2).
        assert_eq!(minute_of_day(ts("2026-07-10T23:30:00Z"), &warsaw), Some(90));
        assert_eq!(
            minute_of_day(ts("2026-07-10T23:30:00Z"), &zone("UTC").expect("zone")),
            Some(23 * 60 + 30)
        );
    }

    #[test]
    fn the_next_moment_is_today_when_it_is_still_ahead() {
        let warsaw = zone("Europe/Warsaw").expect("zone");
        // 10:00 Warsaw (UTC+1); 22:00 is ahead the same day = 21:00 UTC.
        let now = ts("2026-01-10T09:00:00Z");
        assert_eq!(
            next_at(now, &warsaw, 22 * 60),
            Some(ts("2026-01-10T21:00:00Z"))
        );
    }

    #[test]
    fn the_next_moment_is_tomorrow_when_it_has_passed() {
        let warsaw = zone("Europe/Warsaw").expect("zone");
        // 23:00 Warsaw; 06:00 comes tomorrow = 05:00 UTC.
        let now = ts("2026-01-10T22:00:00Z");
        assert_eq!(
            next_at(now, &warsaw, 6 * 60),
            Some(ts("2026-01-11T05:00:00Z"))
        );
        // Exactly at the minute it is the next day's, not "now".
        assert_eq!(
            next_at(ts("2026-01-10T05:00:00Z"), &warsaw, 6 * 60),
            Some(ts("2026-01-11T05:00:00Z"))
        );
    }

    #[test]
    fn daylight_saving_moves_the_hours_with_the_clock() {
        let warsaw = zone("Europe/Warsaw").expect("zone");
        // Clocks go forward on 2026-03-29 (02:00 -> 03:00). 00:00 on the 29th is still UTC+1,
        // 00:00 on the 30th is UTC+2: the two moments are 23 hours apart, not 24.
        let a = next_at(ts("2026-03-28T12:00:00Z"), &warsaw, 0).expect("a");
        let b = next_at(a, &warsaw, 0).expect("b");
        assert_eq!(a, ts("2026-03-28T23:00:00Z"));
        assert_eq!(b - a, 23 * 3600);
    }

    #[test]
    fn a_minute_that_falls_into_the_gap_still_has_a_moment() {
        let warsaw = zone("Europe/Warsaw").expect("zone");
        // 02:30 does not exist on 2026-03-29; the answer is still a moment on that day.
        let got = next_at(ts("2026-03-29T00:00:00Z"), &warsaw, 2 * 60 + 30).expect("moment");
        assert!(got > ts("2026-03-29T00:00:00Z") && got < ts("2026-03-30T00:00:00Z"));
    }
}
