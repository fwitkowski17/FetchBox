//! Repository for the `settings` key/value table.

use dlm_core::Settings;
use rusqlite::{params, Connection, OptionalExtension};

use crate::Result;

const CONNECTIONS: &str = "connections";
const MAX_CONCURRENT: &str = "max_concurrent";
const SPEED_PER_DOWNLOAD: &str = "speed_per_download";
const SPEED_GLOBAL: &str = "speed_global";

/// Load settings; missing or unparsable keys fall back to the defaults.
pub fn load(conn: &Connection) -> Result<Settings> {
    let d = Settings::default();
    Ok(Settings {
        connections: get_num(conn, CONNECTIONS)?.unwrap_or(d.connections),
        max_concurrent: get_num(conn, MAX_CONCURRENT)?.unwrap_or(d.max_concurrent),
        speed_per_download: get_num(conn, SPEED_PER_DOWNLOAD)?.unwrap_or(d.speed_per_download),
        speed_global: get_num(conn, SPEED_GLOBAL)?.unwrap_or(d.speed_global),
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
}
