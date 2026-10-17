//! SQLite storage: connection setup, migrations, repositories.

pub mod downloads;
pub mod known_hosts;
pub mod segments;
pub mod settings;
pub mod stats;
pub mod torrents;

use std::path::Path;

pub use rusqlite::Connection;

/// Storage errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A stored value could not be mapped to a domain type (corrupt or newer DB).
    #[error("invalid stored value: {0}")]
    Corrupt(String),
}

impl From<Error> for fetchbox_core::Error {
    fn from(e: Error) -> Self {
        fetchbox_core::Error::Internal(e.to_string())
    }
}

/// Result alias for storage operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Migrations in order. Index + 1 is the schema version stored in `PRAGMA user_version`.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_overwrite.sql"),
    include_str!("../migrations/0003_subdir.sql"),
    include_str!("../migrations/0004_ignore_space.sql"),
    include_str!("../migrations/0005_waiting_space.sql"),
    include_str!("../migrations/0006_archived.sql"),
    include_str!("../migrations/0007_speed_limit.sql"),
    include_str!("../migrations/0008_stats.sql"),
    include_str!("../migrations/0009_schedule.sql"),
    include_str!("../migrations/0010_checksum.sql"),
    include_str!("../migrations/0011_sources.sql"),
    include_str!("../migrations/0012_torrents.sql"),
    include_str!("../migrations/0013_torrent_start_now.sql"),
];

/// Open (creating if needed) the database at `path` and run pending migrations.
pub fn open(path: &Path) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Open an in-memory database with migrations applied (tests).
pub fn open_in_memory() -> Result<Connection> {
    let mut conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let current: usize = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current >= MIGRATIONS.len() {
        return Ok(());
    }
    // A migration may rebuild a table (SQLite cannot alter a CHECK): that needs foreign keys off,
    // which cannot be changed inside a transaction. They are checked afterwards.
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let applied = (|| -> Result<()> {
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i + 1)?;
            tx.commit()?;
            tracing::info!(version = i + 1, "applied migration");
        }
        Ok(())
    })();
    conn.pragma_update(None, "foreign_keys", "ON")?;
    applied?;
    let broken: usize = conn
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |_| Ok(()))?
        .count();
    if broken > 0 {
        return Err(Error::Corrupt(format!(
            "{broken} rows break a foreign key after the migrations"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .is_ok()
    }

    #[test]
    fn migrations_create_tables() {
        let conn = open_in_memory().expect("open");
        for t in ["downloads", "segments", "settings"] {
            assert!(table_exists(&conn, t), "missing table {t}");
        }
    }

    /// The database as it was after `version` migrations.
    fn database_at_version(version: usize) -> Connection {
        let mut conn = Connection::open_in_memory().expect("open");
        conn.pragma_update(None, "foreign_keys", "ON").expect("fk");
        for (i, sql) in MIGRATIONS.iter().enumerate().take(version) {
            let tx = conn.transaction().expect("tx");
            tx.execute_batch(sql).expect("migration");
            tx.pragma_update(None, "user_version", i + 1).expect("v");
            tx.commit().expect("commit");
        }
        conn
    }

    #[test]
    fn the_checksum_migration_keeps_rows_segments_history_and_the_id_counter() {
        let mut conn = database_at_version(9);
        for name in ["a", "b", "c"] {
            conn.execute(
                "INSERT INTO downloads (url, filename, category, status, size, downloaded, subdir, archived, speed_limit)
                 VALUES (?1, ?1, 'video', 'queued', 100, 10, 'video', 1, 500)",
                [name],
            )
            .expect("insert");
        }
        conn.execute("INSERT INTO segments VALUES (1, 0, 0, 99, 10)", [])
            .expect("segment");
        conn.execute(
            "UPDATE downloads SET status = 'completed', finished_at = 1000 WHERE id = 2",
            [],
        )
        .expect("completed");
        // The highest id is deleted: the counter must stay above it.
        conn.execute("DELETE FROM downloads WHERE id = 3", [])
            .expect("delete");

        migrate(&mut conn).expect("migrate");

        let (name, size, subdir, archived, limit): (String, i64, String, bool, i64) = conn
            .query_row(
                "SELECT filename, size, subdir, archived, speed_limit FROM downloads WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("row");
        assert_eq!(
            (name.as_str(), size, subdir.as_str(), archived, limit),
            ("a", 100, "video", true, 500)
        );
        let segments: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segments WHERE download_id = 1",
                [],
                |r| r.get(0),
            )
            .expect("segments");
        assert_eq!(segments, 1, "segments survive the rebuild");
        let history: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM history WHERE download_id = 2",
                [],
                |r| r.get(0),
            )
            .expect("history");
        assert_eq!(history, 1);
        // Never reused: the next id is 4, not 3.
        conn.execute(
            "INSERT INTO downloads (url, filename) VALUES ('u', 'f')",
            [],
        )
        .expect("insert");
        assert_eq!(conn.last_insert_rowid(), 4);
        // The new status is allowed, a made-up one is not, and the history trigger still works.
        conn.execute("UPDATE downloads SET status = 'verifying' WHERE id = 4", [])
            .expect("verifying");
        assert!(conn
            .execute("UPDATE downloads SET status = 'bogus' WHERE id = 4", [])
            .is_err());
        conn.execute("UPDATE downloads SET status = 'failed' WHERE id = 4", [])
            .expect("failed");
        let outcome: String = conn
            .query_row(
                "SELECT outcome FROM history WHERE download_id = 4",
                [],
                |r| r.get(0),
            )
            .expect("trigger");
        assert_eq!(outcome, "failed");
        // Deleting a download still removes its segments (the foreign key survived).
        conn.execute("DELETE FROM downloads WHERE id = 1", [])
            .expect("delete");
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM segments", [], |r| r.get(0))
            .expect("count");
        assert_eq!(left, 0);
        let fk_on: bool = conn
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .expect("pragma");
        assert!(fk_on, "foreign keys are back on after the migrations");
    }

    #[test]
    fn the_sources_migration_keeps_downloads_and_allows_new_kinds() {
        let mut conn = database_at_version(10);
        conn.execute(
            "INSERT INTO downloads (url, filename, status, size, downloaded, checksum_algo, checksum_expected)
             VALUES ('http://x/a', 'a', 'paused', 100, 10, 'md5', 'd41d8cd98f00b204e9800998ecf8427e')",
            [],
        )
        .expect("insert");
        conn.execute("INSERT INTO segments VALUES (1, 0, 0, 99, 10)", [])
            .expect("segment");
        conn.execute("UPDATE downloads SET status = 'failed' WHERE id = 1", [])
            .expect("failed");

        migrate(&mut conn).expect("migrate");

        let (url, status, algo, size, username): (String, String, String, i64, Option<String>) =
            conn.query_row(
                "SELECT url, status, checksum_algo, size, username FROM downloads WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("row");
        assert_eq!(
            (url.as_str(), status.as_str(), algo.as_str(), size, username),
            ("http://x/a", "failed", "md5", 100, None)
        );
        let kept: i64 = conn
            .query_row("SELECT COUNT(*) FROM segments", [], |r| r.get(0))
            .expect("segments");
        assert_eq!(kept, 1, "segments survive the rebuild");
        let history: i64 = conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .expect("history");
        assert_eq!(history, 1, "history is kept, and its trigger still fires");
        for kind in ["ftp", "sftp"] {
            conn.execute(
                "INSERT INTO downloads (url, filename, source_kind, username, password)
                 VALUES ('u', 'f', ?1, 'me', 'pw')",
                [kind],
            )
            .unwrap_or_else(|e| panic!("{kind} is allowed: {e}"));
        }
        assert!(conn
            .execute(
                "INSERT INTO downloads (url, filename, source_kind) VALUES ('u', 'f', 'gopher')",
                [],
            )
            .is_err());
        conn.execute("UPDATE downloads SET status = 'cancelled' WHERE id = 2", [])
            .expect("cancel");
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM history WHERE download_id = 2",
                [],
                |r| r.get(0),
            )
            .expect("history");
        assert_eq!(rows, 1);
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut conn = open_in_memory().expect("open");
        migrate(&mut conn).expect("second run");
        let v: usize = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .expect("version");
        assert_eq!(v, MIGRATIONS.len());
    }
}
