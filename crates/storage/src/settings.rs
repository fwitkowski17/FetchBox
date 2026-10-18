//! Repository for the `settings` key/value table.

use fetchbox_core::Settings;
use rusqlite::{params, Connection, OptionalExtension};

use crate::Result;

const CONNECTIONS: &str = "connections";
const MAX_CONCURRENT: &str = "max_concurrent";
const SPEED_PER_DOWNLOAD: &str = "speed_per_download";
const SPEED_GLOBAL: &str = "speed_global";
const CATEGORY_SUBDIRS: &str = "category_subdirs";
const CHECK_UPDATES: &str = "check_updates";
const CHECK_PRERELEASES: &str = "check_prereleases";
const SCHEDULE_ENABLED: &str = "schedule_enabled";
const SCHEDULE_FROM: &str = "schedule_from";
const SCHEDULE_TO: &str = "schedule_to";
const SCHEDULE_TIMEZONE: &str = "schedule_timezone";
const SCHEDULE_ON_END: &str = "schedule_on_end";
const TORRENT_DOWN_LIMIT: &str = "torrent_down_limit";
const TORRENT_UP_LIMIT: &str = "torrent_up_limit";
const TORRENT_SCHEDULE_ENABLED: &str = "torrent_schedule_enabled";
const TORRENT_SCHEDULE_FROM: &str = "torrent_schedule_from";
const TORRENT_SCHEDULE_TO: &str = "torrent_schedule_to";
const TORRENT_SCHEDULE_TIMEZONE: &str = "torrent_schedule_timezone";
const DEFAULT_DISK: &str = "default_disk";

/// Key of the Argon2 hash of the password created in the first-run setup. It lives in the
/// same table as the settings but is not part of `Settings`, so it never reaches the API.
const PASSWORD_HASH: &str = "password_hash";

/// The stored password hash, if the account was set up through the UI.
pub fn password_hash(conn: &Connection) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [PASSWORD_HASH],
            |r| r.get(0),
        )
        .optional()?)
}

/// Store (or replace) the password hash.
pub fn set_password_hash(conn: &Connection, hash: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![PASSWORD_HASH, hash],
    )?;
    Ok(())
}

/// Forget the stored password (the account made in the UI). Returns whether there was one. The
/// server then starts like on the first run: locked until a new account is created.
pub fn clear_password_hash(conn: &Connection) -> Result<bool> {
    let removed = conn.execute("DELETE FROM settings WHERE key = ?1", [PASSWORD_HASH])?;
    Ok(removed > 0)
}

/// Load settings; missing or unparsable keys fall back to the defaults.
pub fn load(conn: &Connection) -> Result<Settings> {
    let d = Settings::default();
    Ok(Settings {
        connections: get_num(conn, CONNECTIONS)?.unwrap_or(d.connections),
        max_concurrent: get_num(conn, MAX_CONCURRENT)?.unwrap_or(d.max_concurrent),
        speed_per_download: get_num(conn, SPEED_PER_DOWNLOAD)?.unwrap_or(d.speed_per_download),
        speed_global: get_num(conn, SPEED_GLOBAL)?.unwrap_or(d.speed_global),
        category_subdirs: get_num(conn, CATEGORY_SUBDIRS)?.unwrap_or(d.category_subdirs),
        check_updates: get_num(conn, CHECK_UPDATES)?.unwrap_or(d.check_updates),
        check_prereleases: get_num(conn, CHECK_PRERELEASES)?.unwrap_or(d.check_prereleases),
        schedule_enabled: get_num(conn, SCHEDULE_ENABLED)?.unwrap_or(d.schedule_enabled),
        schedule_from: get_num(conn, SCHEDULE_FROM)?.unwrap_or(d.schedule_from),
        schedule_to: get_num(conn, SCHEDULE_TO)?.unwrap_or(d.schedule_to),
        schedule_timezone: get_num(conn, SCHEDULE_TIMEZONE)?.unwrap_or(d.schedule_timezone),
        schedule_on_end: get_num(conn, SCHEDULE_ON_END)?.unwrap_or(d.schedule_on_end),
        torrent_down_limit: get_num(conn, TORRENT_DOWN_LIMIT)?.unwrap_or(d.torrent_down_limit),
        torrent_up_limit: get_num(conn, TORRENT_UP_LIMIT)?.unwrap_or(d.torrent_up_limit),
        torrent_schedule_enabled: get_num(conn, TORRENT_SCHEDULE_ENABLED)?
            .unwrap_or(d.torrent_schedule_enabled),
        torrent_schedule_from: get_num(conn, TORRENT_SCHEDULE_FROM)?
            .unwrap_or(d.torrent_schedule_from),
        torrent_schedule_to: get_num(conn, TORRENT_SCHEDULE_TO)?.unwrap_or(d.torrent_schedule_to),
        torrent_schedule_timezone: get_num(conn, TORRENT_SCHEDULE_TIMEZONE)?
            .unwrap_or(d.torrent_schedule_timezone),
        default_disk: get_num(conn, DEFAULT_DISK)?.unwrap_or(d.default_disk),
    })
}

/// Save all settings atomically. Validation is the caller's job (`Settings::validate`).
pub fn save(conn: &mut Connection, s: &Settings) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )?;
        stmt.execute(params![CONNECTIONS, s.connections.to_string()])?;
        stmt.execute(params![MAX_CONCURRENT, s.max_concurrent.to_string()])?;
        stmt.execute(params![
            SPEED_PER_DOWNLOAD,
            s.speed_per_download.to_string()
        ])?;
        stmt.execute(params![SPEED_GLOBAL, s.speed_global.to_string()])?;
        stmt.execute(params![CATEGORY_SUBDIRS, s.category_subdirs.to_string()])?;
        stmt.execute(params![CHECK_UPDATES, s.check_updates.to_string()])?;
        stmt.execute(params![CHECK_PRERELEASES, s.check_prereleases.to_string()])?;
        stmt.execute(params![SCHEDULE_ENABLED, s.schedule_enabled.to_string()])?;
        stmt.execute(params![SCHEDULE_FROM, s.schedule_from.to_string()])?;
        stmt.execute(params![SCHEDULE_TO, s.schedule_to.to_string()])?;
        stmt.execute(params![SCHEDULE_TIMEZONE, s.schedule_timezone])?;
        stmt.execute(params![SCHEDULE_ON_END, s.schedule_on_end.as_str()])?;
        stmt.execute(params![
            TORRENT_DOWN_LIMIT,
            s.torrent_down_limit.to_string()
        ])?;
        stmt.execute(params![TORRENT_UP_LIMIT, s.torrent_up_limit.to_string()])?;
        stmt.execute(params![
            TORRENT_SCHEDULE_ENABLED,
            s.torrent_schedule_enabled.to_string()
        ])?;
        stmt.execute(params![
            TORRENT_SCHEDULE_FROM,
            s.torrent_schedule_from.to_string()
        ])?;
        stmt.execute(params![
            TORRENT_SCHEDULE_TO,
            s.torrent_schedule_to.to_string()
        ])?;
        stmt.execute(params![
            TORRENT_SCHEDULE_TIMEZONE,
            s.torrent_schedule_timezone
        ])?;
        stmt.execute(params![DEFAULT_DISK, s.default_disk])?;
    }
    tx.commit()?;
    Ok(())
}

fn get_num<T: std::str::FromStr>(conn: &Connection, key: &str) -> Result<Option<T>> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(raw.and_then(|v| v.parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;

    #[test]
    fn defaults_when_empty() {
        let conn = open_in_memory().expect("db");
        assert_eq!(load(&conn).expect("load"), Settings::default());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let mut conn = open_in_memory().expect("db");
        let s = Settings {
            connections: 8,
            max_concurrent: 5,
            speed_per_download: 1_000_000,
            speed_global: 5_000_000,
            category_subdirs: true,
            check_updates: false,
            check_prereleases: true,
            schedule_enabled: true,
            schedule_from: 22 * 60,
            schedule_to: 6 * 60,
            schedule_timezone: "Europe/Warsaw".to_owned(),
            schedule_on_end: fetchbox_core::OnHoursEnd::Finish,
            torrent_down_limit: 2_000_000,
            torrent_up_limit: 500_000,
            torrent_schedule_enabled: true,
            torrent_schedule_from: 23 * 60,
            torrent_schedule_to: 7 * 60,
            torrent_schedule_timezone: "Europe/Berlin".to_owned(),
            default_disk: "ssd1".to_owned(),
        };
        save(&mut conn, &s).expect("save");
        assert_eq!(load(&conn).expect("load"), s);
        // Saving twice overwrites instead of failing on the primary key.
        let s2 = Settings {
            connections: 2,
            ..s
        };
        save(&mut conn, &s2).expect("save again");
        assert_eq!(load(&conn).expect("load"), s2);
    }

    #[test]
    fn garbage_value_falls_back_to_default() {
        let conn = open_in_memory().expect("db");
        conn.execute("INSERT INTO settings VALUES ('connections', 'abc')", [])
            .expect("insert");
        assert_eq!(
            load(&conn).expect("load").connections,
            Settings::default().connections
        );
    }

    #[test]
    fn the_password_can_be_forgotten_and_the_settings_stay() {
        let mut conn = open_in_memory().expect("db");
        assert!(!clear_password_hash(&conn).expect("nothing to clear"));
        save(
            &mut conn,
            &Settings {
                connections: 7,
                ..Settings::default()
            },
        )
        .expect("save");
        set_password_hash(&conn, "$argon2id$one").expect("set");
        assert!(clear_password_hash(&conn).expect("clear"));
        assert_eq!(password_hash(&conn).expect("get"), None);
        assert_eq!(load(&conn).expect("load").connections, 7);
        assert!(!clear_password_hash(&conn).expect("already gone"));
    }

    #[test]
    fn password_hash_roundtrip_and_isolation() {
        let mut conn = open_in_memory().expect("db");
        assert_eq!(password_hash(&conn).expect("get"), None);
        set_password_hash(&conn, "$argon2id$one").expect("set");
        set_password_hash(&conn, "$argon2id$two").expect("replace");
        assert_eq!(
            password_hash(&conn).expect("get").as_deref(),
            Some("$argon2id$two")
        );
        // Saving the regular settings must neither touch nor expose the hash.
        save(&mut conn, &Settings::default()).expect("save");
        assert_eq!(
            password_hash(&conn).expect("get").as_deref(),
            Some("$argon2id$two")
        );
    }
}
