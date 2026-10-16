//! How the console log writes numbers and settings: short, readable and the same everywhere.

use fetchbox_core::{OnHoursEnd, Settings};

/// `1536` -> `1.5 KiB`, `0` -> `0 B`.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Bytes per second: `0` means "no limit" in the settings, which is how it is written there.
pub fn rate(n: u64) -> String {
    format!("{}/s", bytes(n))
}

/// A speed limit as the settings keep it (`0` = none).
pub fn limit(n: u64) -> String {
    if n == 0 {
        "unlimited".to_owned()
    } else {
        rate(n)
    }
}

/// `3725` -> `1h 2m 5s`, `45` -> `45s`.
pub fn duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    match (h, m) {
        (0, 0) => format!("{s}s"),
        (0, _) => format!("{m}m {s}s"),
        _ => format!("{h}h {m}m {s}s"),
    }
}

/// Minutes after midnight as `06:30`.
pub fn clock(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60)
}

/// The host of a URL, which is what the log shows of it: an address may carry a token.
pub fn host(url: &str) -> String {
    url.split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .and_then(|authority| authority.rsplit('@').next())
        .unwrap_or("")
        .to_owned()
}

/// The average speed of a download: what it fetched over the time it spent transferring. `None`
/// when no time was measured (rows from before the statistics, or nothing fetched yet).
pub fn average_speed(bytes: u64, active_ms: u64) -> Option<u64> {
    (active_ms > 0).then(|| (u128::from(bytes) * 1000 / u128::from(active_ms)) as u64)
}

fn hours(s: &Settings) -> String {
    if s.schedule_enabled {
        format!(
            "{}-{} {}",
            clock(s.schedule_from),
            clock(s.schedule_to),
            s.schedule_timezone
        )
    } else {
        "off".to_owned()
    }
}

fn on_off(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}

fn on_end(s: &Settings) -> &'static str {
    match s.schedule_on_end {
        OnHoursEnd::Pause => "pause running downloads",
        OnHoursEnd::Finish => "let running downloads finish",
    }
}

/// What differs between two settings, one short phrase each (`connections 4 -> 6`).
pub fn settings_changes(old: &Settings, new: &Settings) -> Vec<String> {
    let mut out = Vec::new();
    let mut change = |name: &str, a: String, b: String| {
        if a != b {
            out.push(format!("{name} {a} -> {b}"));
        }
    };
    change(
        "connections",
        old.connections.to_string(),
        new.connections.to_string(),
    );
    change(
        "simultaneous downloads",
        old.max_concurrent.to_string(),
        new.max_concurrent.to_string(),
    );
    change(
        "speed of each download",
        limit(old.speed_per_download),
        limit(new.speed_per_download),
    );
    change(
        "total speed",
        limit(old.speed_global),
        limit(new.speed_global),
    );
    change(
        "category folders",
        on_off(old.category_subdirs).to_owned(),
        on_off(new.category_subdirs).to_owned(),
    );
    change("download hours", hours(old), hours(new));
    change(
        "when the hours end",
        on_end(old).to_owned(),
        on_end(new).to_owned(),
    );
    change(
        "update check",
        on_off(old.check_updates).to_owned(),
        on_off(new.check_updates).to_owned(),
    );
    out
}

/// The settings in one line, for the start-up log.
pub fn settings_summary(s: &Settings) -> String {
    format!(
        "connections {}, simultaneous downloads {}, speed of each download {}, total speed {}, category folders {}, download hours {}, update check {}",
        s.connections,
        s.max_concurrent,
        limit(s.speed_per_download),
        limit(s.speed_global),
        on_off(s.category_subdirs),
        hours(s),
        on_off(s.check_updates),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_speeds_read_like_a_file_manager() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(bytes(1 << 40), "1.0 TiB");
        assert!(
            bytes(u64::MAX).ends_with(" TiB"),
            "the largest unit does not overflow"
        );
        assert_eq!(rate(2048), "2.0 KiB/s");
        assert_eq!(limit(0), "unlimited");
        assert_eq!(limit(1024 * 1024), "1.0 MiB/s");
    }

    #[test]
    fn durations_and_clock_times() {
        assert_eq!(duration(0), "0s");
        assert_eq!(duration(45), "45s");
        assert_eq!(duration(125), "2m 5s");
        assert_eq!(duration(3725), "1h 2m 5s");
        assert_eq!(clock(0), "00:00");
        assert_eq!(clock(6 * 60 + 5), "06:05");
        assert_eq!(clock(23 * 60 + 59), "23:59");
    }

    #[test]
    fn only_the_host_of_an_address_is_logged() {
        assert_eq!(
            host("https://example.com/file.zip?token=secret"),
            "example.com"
        );
        assert_eq!(host("http://user:pass@nas.lan:8080/a/b"), "nas.lan:8080");
        assert_eq!(host("http://10.0.0.5/x#frag"), "10.0.0.5");
        assert_eq!(host("nonsense"), "nonsense");
    }

    #[test]
    fn the_average_is_bytes_over_the_time_spent_transferring() {
        assert_eq!(average_speed(10_000_000, 5_000), Some(2_000_000));
        assert_eq!(average_speed(1234, 0), None);
        assert_eq!(average_speed(0, 1000), Some(0));
    }

    #[test]
    fn settings_changes_name_only_what_changed() {
        let old = Settings::default();
        assert!(settings_changes(&old, &old).is_empty());
        let new = Settings {
            connections: 8,
            speed_global: 5 * 1024 * 1024,
            schedule_enabled: true,
            schedule_from: 22 * 60,
            schedule_to: 6 * 60,
            schedule_timezone: "Europe/Warsaw".into(),
            schedule_on_end: OnHoursEnd::Finish,
            check_updates: false,
            ..Settings::default()
        };
        assert_eq!(
            settings_changes(&old, &new),
            vec![
                "connections 4 -> 8",
                "total speed unlimited -> 5.0 MiB/s",
                "download hours off -> 22:00-06:00 Europe/Warsaw",
                "when the hours end pause running downloads -> let running downloads finish",
                "update check on -> off",
            ]
        );
    }

    #[test]
    fn the_summary_names_every_setting_that_matters() {
        let s = settings_summary(&Settings::default());
        assert!(
            s.contains("connections 4") && s.contains("download hours off"),
            "{s}"
        );
    }
}
