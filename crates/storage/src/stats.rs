//! Statistics: the `history` table (one row per download that ended, written by a trigger, so a
//! download that is removed from the list stays in the numbers) plus what is still running.

use fetchbox_core::{Category, CategoryStat, DayStat, Id, LargestFile, Stats, StatsRange};
use rusqlite::{params, Connection, OptionalExtension};

use crate::Result;

const DAY: i64 = 86_400;
/// How many days the activity chart has.
pub const CHART_DAYS: i64 = 14;
const PEAK_SPEED: &str = "peak_speed";

/// The time a download has spent transferring so far (milliseconds), `0` when none was measured.
pub fn active_ms(conn: &Connection, id: Id) -> Result<u64> {
    let ms: Option<i64> = conn
        .query_row("SELECT active_ms FROM downloads WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(u64::try_from(ms.unwrap_or(0)).unwrap_or(0))
}

/// Add time spent transferring to a download (called once per progress tick and when it stops).
pub fn add_active_ms(conn: &Connection, id: Id, ms: u64) -> Result<()> {
    let ms = i64::try_from(ms).unwrap_or(i64::MAX);
    conn.execute(
        "UPDATE downloads SET active_ms = active_ms + ?2 WHERE id = ?1",
        params![id, ms],
    )?;
    Ok(())
}

/// The highest speed ever seen, bytes per second (kept next to the settings, not part of them).
pub fn peak_speed(conn: &Connection) -> Result<u64> {
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [PEAK_SPEED],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v.and_then(|v| v.parse().ok()).unwrap_or(0))
}

pub fn set_peak_speed(conn: &Connection, speed: u64) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![PEAK_SPEED, speed.to_string()],
    )?;
    Ok(())
}

/// Unix time of the start of the user's day that contains `now`. `offset` is the user's distance
/// from UTC in seconds (east is positive).
pub fn day_start(now: i64, offset: i64) -> i64 {
    (now + offset).div_euclid(DAY) * DAY - offset
}

fn count(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

/// The summary of the downloads that ended in `range` (counted in the user's days), plus what is
/// unfinished right now. `now` and `offset` (seconds east of UTC) are parameters so that tests do
/// not depend on the clock.
pub fn summary(conn: &Connection, now: i64, range: StatsRange, offset: i64) -> Result<Stats> {
    let today = day_start(now, offset);
    // `NULL` = no lower bound.
    let since: Option<i64> = range.days().map(|d| today - (i64::from(d) - 1) * DAY);

    let mut stats = Stats {
        range,
        peak_speed: peak_speed(conn)?,
        ..Stats::default()
    };

    // Outcomes, bytes and file sizes.
    let mut stmt = conn.prepare(
        "SELECT outcome, COUNT(*), COALESCE(SUM(bytes), 0),
                COALESCE(SUM(CASE WHEN outcome = 'completed' THEN COALESCE(size, bytes) END), 0)
           FROM history WHERE (?1 IS NULL OR finished_at >= ?1) GROUP BY outcome",
    )?;
    let rows = stmt.query_map([since], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    for row in rows {
        let (outcome, n, bytes, size) = row?;
        stats.bytes += count(bytes);
        match outcome.as_str() {
            "completed" => {
                stats.completed = count(n);
                stats.completed_bytes = count(size);
            }
            "failed" => stats.failed = count(n),
            _ => stats.cancelled = count(n),
        }
    }

    // Time spent transferring, and the bytes that were timed (older rows were not).
    let (active_ms, timed_bytes): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(active_ms), 0), COALESCE(SUM(CASE WHEN active_ms > 0 THEN bytes END), 0)
           FROM history WHERE (?1 IS NULL OR finished_at >= ?1)",
        [since],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (mut active_ms, mut timed_bytes) = (count(active_ms), count(timed_bytes));

    // What is not finished yet: it has fetched bytes and spent time already.
    let (in_progress, live_bytes, live_ms, live_timed): (i64, i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(downloaded), 0), COALESCE(SUM(active_ms), 0),
                COALESCE(SUM(CASE WHEN active_ms > 0 THEN downloaded END), 0)
           FROM downloads WHERE status IN ('queued','downloading','paused')",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    stats.in_progress = count(in_progress);
    stats.bytes += count(live_bytes);
    active_ms += count(live_ms);
    timed_bytes += count(live_timed);

    stats.active_seconds = active_ms / 1000;
    if active_ms > 0 {
        let speed = u128::from(timed_bytes) * 1000 / u128::from(active_ms);
        stats.average_speed = u64::try_from(speed).unwrap_or(u64::MAX);
    }

    stats.largest = conn
        .query_row(
            "SELECT name, size FROM history
              WHERE outcome = 'completed' AND size IS NOT NULL AND (?1 IS NULL OR finished_at >= ?1)
              ORDER BY size DESC, finished_at DESC LIMIT 1",
            [since],
            |r| {
                Ok(LargestFile {
                    name: r.get(0)?,
                    size: count(r.get(1)?),
                })
            },
        )
        .optional()?;

    let mut stmt = conn.prepare(
        "SELECT category, COUNT(*), COALESCE(SUM(COALESCE(size, bytes)), 0)
           FROM history WHERE outcome = 'completed' AND (?1 IS NULL OR finished_at >= ?1)
          GROUP BY category ORDER BY 3 DESC, 2 DESC",
    )?;
    let rows = stmt.query_map([since], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (category, n, bytes) = row?;
        if let Some(category) = Category::parse(&category) {
            stats.categories.push(CategoryStat {
                category,
                count: count(n),
                bytes: count(bytes),
            });
        }
    }

    // The chart: the last days, empty ones included, whatever the period.
    let first = today - (CHART_DAYS - 1) * DAY;
    let mut days: Vec<DayStat> = (0..CHART_DAYS)
        .map(|i| DayStat {
            start: first + i * DAY,
            count: 0,
            bytes: 0,
        })
        .collect();
    let mut stmt = conn.prepare(
        "SELECT (finished_at - ?1) / ?2, COUNT(*), COALESCE(SUM(bytes), 0)
           FROM history WHERE finished_at >= ?1 GROUP BY 1",
    )?;
    let rows = stmt.query_map(params![first, DAY], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (i, n, bytes) = row?;
        if let Some(day) = usize::try_from(i).ok().and_then(|i| days.get_mut(i)) {
            day.count = count(n);
            day.bytes = count(bytes);
        }
    }
    stats.days = days;

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::downloads::{self, NewDownload};
    use crate::open_in_memory;
    use fetchbox_core::{SourceKind, Status};

    /// 2026-10-15 12:00:00 UTC.
    const NOW: i64 = 1_792_065_600;

    #[allow(clippy::too_many_arguments)] // a test fixture: one row, every column
    fn history(
        conn: &Connection,
        id: i64,
        ago_days: i64,
        outcome: &str,
        category: &str,
        size: Option<i64>,
        bytes: i64,
        active_ms: i64,
    ) {
        conn.execute(
            "INSERT INTO history (download_id, added_at, finished_at, outcome, name, category, size, bytes, active_ms)
             VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, NOW - ago_days * DAY, outcome, format!("file{id}"), category, size, bytes, active_ms],
        )
        .expect("history row");
    }

    fn new_download(conn: &Connection, name: &str) -> Id {
        downloads::insert(
            conn,
            &NewDownload {
                url: format!("http://x/{name}"),
                filename: name.into(),
                category: Category::Video,
                source_kind: SourceKind::Http,
                overwrite: false,
                subdir: String::new(),
                ignore_space: false,
                start_at: None,
                start_now: false,
            },
        )
        .expect("insert")
    }

    #[test]
    fn an_empty_database_has_zeroes_and_an_empty_chart() {
        let conn = open_in_memory().expect("db");
        let s = summary(&conn, NOW, StatsRange::All, 0).expect("stats");
        assert_eq!(
            (s.completed, s.failed, s.cancelled, s.in_progress, s.bytes),
            (0, 0, 0, 0, 0)
        );
        assert_eq!((s.average_speed, s.peak_speed, s.active_seconds), (0, 0, 0));
        assert!(s.largest.is_none() && s.categories.is_empty());
        assert_eq!(s.days.len(), 14);
        assert!(s.days.iter().all(|d| d.count == 0 && d.bytes == 0));
    }

    #[test]
    fn the_end_of_a_download_is_recorded_and_survives_its_removal() {
        let conn = open_in_memory().expect("db");
        let id = new_download(&conn, "movie.mkv");
        downloads::set_status(&conn, id, Status::Downloading, None).expect("running");
        downloads::set_downloaded(&conn, id, 5_000_000).expect("downloaded");
        downloads::set_size(&conn, id, 5_000_000).expect("size");
        add_active_ms(&conn, id, 2_000).expect("time");
        add_active_ms(&conn, id, 3_000).expect("more time");
        assert_eq!(
            summary(&conn, NOW, StatsRange::All, 0)
                .expect("s")
                .completed,
            0,
            "not over yet"
        );
        downloads::set_status(&conn, id, Status::Completed, None).expect("complete");
        downloads::delete(&conn, id).expect("remove from the list");

        let s = summary(
            &conn,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_secs() as i64,
            StatsRange::All,
            0,
        )
        .expect("stats");
        assert_eq!(
            (s.completed, s.completed_bytes, s.bytes),
            (1, 5_000_000, 5_000_000)
        );
        assert_eq!(s.active_seconds, 5);
        assert_eq!(s.average_speed, 1_000_000, "5 MB in 5 s of transfer");
        assert_eq!(
            s.largest,
            Some(LargestFile {
                name: "movie.mkv".into(),
                size: 5_000_000
            })
        );
        assert_eq!(
            s.categories,
            vec![CategoryStat {
                category: Category::Video,
                count: 1,
                bytes: 5_000_000
            }]
        );
    }

    #[test]
    fn a_retried_download_counts_once_with_its_last_outcome() {
        let conn = open_in_memory().expect("db");
        let id = new_download(&conn, "a.bin");
        downloads::set_status(&conn, id, Status::Failed, Some("boom")).expect("fail");
        downloads::set_status(&conn, id, Status::Downloading, None).expect("retry");
        downloads::set_downloaded(&conn, id, 10).expect("downloaded");
        downloads::set_status(&conn, id, Status::Completed, None).expect("complete");
        let s = summary(&conn, i64::MAX / 4, StatsRange::All, 0).expect("stats");
        assert_eq!((s.completed, s.failed), (1, 0));
    }

    #[test]
    fn cancelling_is_recorded_too_and_unfinished_downloads_are_counted_apart() {
        let conn = open_in_memory().expect("db");
        let gone = new_download(&conn, "gone.bin");
        downloads::set_status(&conn, gone, Status::Downloading, None).expect("running");
        downloads::set_downloaded(&conn, gone, 700).expect("downloaded");
        downloads::set_status(&conn, gone, Status::Cancelled, None).expect("cancel");
        let running = new_download(&conn, "running.bin");
        downloads::set_status(&conn, running, Status::Downloading, None).expect("run");
        downloads::set_downloaded(&conn, running, 300).expect("downloaded");
        let s = summary(&conn, i64::MAX / 4, StatsRange::All, 0).expect("stats");
        assert_eq!((s.cancelled, s.in_progress), (1, 1));
        assert_eq!(
            s.bytes, 1_000,
            "what the cancelled one fetched and what the running one has so far"
        );
    }

    #[test]
    fn periods_cut_by_the_users_days() {
        let conn = open_in_memory().expect("db");
        history(&conn, 1, 0, "completed", "video", Some(100), 100, 1_000);
        history(&conn, 2, 3, "completed", "music", Some(200), 200, 1_000);
        history(&conn, 3, 20, "completed", "music", Some(400), 400, 1_000);
        history(&conn, 4, 60, "failed", "other", None, 50, 0);
        let all = |r| summary(&conn, NOW, r, 0).expect("stats");
        assert_eq!(all(StatsRange::Today).completed, 1);
        assert_eq!(all(StatsRange::Week).completed, 2);
        assert_eq!(all(StatsRange::Month).completed, 3);
        let everything = all(StatsRange::All);
        assert_eq!(
            (everything.completed, everything.failed, everything.bytes),
            (3, 1, 750)
        );
        assert_eq!(everything.largest.map(|l| l.size), Some(400));
        // Music: two files, 600 bytes, so it leads Video's single 100-byte file.
        assert_eq!(everything.categories[0].category, Category::Music);
        assert_eq!(
            (
                everything.categories[0].count,
                everything.categories[0].bytes
            ),
            (2, 600)
        );
    }

    #[test]
    fn the_time_zone_decides_where_a_day_ends() {
        let conn = open_in_memory().expect("db");
        // 23:30 UTC the day before NOW: still "yesterday" in UTC, already "today" in UTC+2.
        let late = NOW - 12 * 3600 - 1800;
        conn.execute(
            "INSERT INTO history (download_id, added_at, finished_at, outcome, name, category, size, bytes, active_ms)
             VALUES (1, 0, ?1, 'completed', 'late', 'other', 10, 10, 0)",
            [late],
        )
        .expect("row");
        assert_eq!(
            summary(&conn, NOW, StatsRange::Today, 0)
                .expect("utc")
                .completed,
            0
        );
        assert_eq!(
            summary(&conn, NOW, StatsRange::Today, 2 * 3600)
                .expect("utc+2")
                .completed,
            1
        );
        // NOW is 12:00 UTC. At UTC-5 it is 07:00 and the day began at 05:00 UTC; at UTC+14 it is
        // 02:00 of the next day, which began at 10:00 UTC.
        assert_eq!(day_start(NOW, 0), NOW - 12 * 3600);
        assert_eq!(day_start(NOW, -5 * 3600), NOW - 7 * 3600);
        assert_eq!(day_start(NOW, 14 * 3600), NOW - 2 * 3600);
    }

    #[test]
    fn the_chart_has_fourteen_days_with_the_empty_ones_in_place() {
        let conn = open_in_memory().expect("db");
        history(&conn, 1, 0, "completed", "video", Some(100), 100, 0);
        history(&conn, 2, 0, "failed", "video", None, 30, 0);
        history(&conn, 3, 5, "completed", "video", Some(7), 7, 0);
        history(&conn, 4, 40, "completed", "video", Some(9), 9, 0); // older than the chart
        let s = summary(&conn, NOW, StatsRange::All, 0).expect("stats");
        assert_eq!(s.days.len(), 14);
        let last = s.days.last().expect("today");
        assert_eq!((last.count, last.bytes), (2, 130));
        assert_eq!(last.start, day_start(NOW, 0));
        assert_eq!((s.days[13 - 5].count, s.days[13 - 5].bytes), (1, 7));
        assert_eq!(s.days.iter().map(|d| d.count).sum::<u64>(), 3);
        // Consecutive days, a day apart.
        assert!(s.days.windows(2).all(|w| w[1].start - w[0].start == DAY));
    }

    #[test]
    fn the_average_speed_ignores_what_was_never_timed() {
        let conn = open_in_memory().expect("db");
        // 10 MB in 10 s, and a big old download with no timing at all.
        history(
            &conn,
            1,
            1,
            "completed",
            "video",
            Some(10_000_000),
            10_000_000,
            10_000,
        );
        history(
            &conn,
            2,
            90,
            "completed",
            "video",
            Some(900_000_000),
            900_000_000,
            0,
        );
        let s = summary(&conn, NOW, StatsRange::All, 0).expect("stats");
        assert_eq!(s.average_speed, 1_000_000);
        assert_eq!(s.bytes, 910_000_000, "the totals still count it");
        assert_eq!(s.active_seconds, 10);
    }

    #[test]
    fn the_migration_fills_the_history_from_the_downloads_that_already_ended() {
        // A database from before the table: migrate step by step, add rows, then the last one.
        let conn = Connection::open_in_memory().expect("db");
        let migrations = crate::MIGRATIONS;
        // The statistics came with the eighth migration (index 7); later ones do not matter here.
        const STATS: usize = 7;
        for sql in &migrations[..STATS] {
            conn.execute_batch(sql).expect("old migration");
        }
        conn.execute("INSERT INTO downloads (url, filename, category, status, size, downloaded, finished_at) VALUES ('u1', 'a', 'photo', 'completed', 10, 10, 100)", []).expect("a");
        conn.execute("INSERT INTO downloads (url, filename, category, status, downloaded) VALUES ('u2', 'b', 'other', 'failed', 3)", []).expect("b");
        conn.execute("INSERT INTO downloads (url, filename, category, status, downloaded) VALUES ('u3', 'c', 'other', 'downloading', 5)", []).expect("c");
        conn.execute_batch(migrations[STATS])
            .expect("stats migration");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .expect("count");
        assert_eq!(rows, 2, "the one still running is not history yet");
        let timed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM history WHERE active_ms > 0",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(timed, 0);
    }

    #[test]
    fn the_peak_speed_is_remembered() {
        let conn = open_in_memory().expect("db");
        assert_eq!(peak_speed(&conn).expect("peak"), 0);
        set_peak_speed(&conn, 12_345_678).expect("set");
        set_peak_speed(&conn, 20_000_000).expect("raise");
        assert_eq!(
            summary(&conn, NOW, StatsRange::Today, 0)
                .expect("stats")
                .peak_speed,
            20_000_000
        );
    }
}
