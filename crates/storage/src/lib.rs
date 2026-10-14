//! SQLite storage: connection setup, migrations, repositories.

pub mod downloads;
pub mod segments;
pub mod settings;

use std::path::Path;

use rusqlite::Connection;

/// Storage errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A stored value could not be mapped to a domain type (corrupt or newer DB).
    #[error("invalid stored value: {0}")]
    Corrupt(String),
}

impl From<Error> for dlm_core::Error {
    fn from(e: Error) -> Self {
        dlm_core::Error::Internal(e.to_string())
    }
}

/// Result alias for storage operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Migrations in order. Index + 1 is the schema version stored in `PRAGMA user_version`.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_overwrite.sql"),
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
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
        tracing::info!(version = i + 1, "applied migration");
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
