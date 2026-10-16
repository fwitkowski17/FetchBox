//! Repository for `known_hosts`: the SSH servers whose host key the user accepted (SPEC 6.12).

use fetchbox_core::{Id, KnownHost};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::Result;

const COLUMNS: &str = "id, host, port, key_type, fingerprint, added_at";

fn read_row(row: &Row<'_>) -> rusqlite::Result<KnownHost> {
    let port: i64 = row.get(2)?;
    Ok(KnownHost {
        id: row.get(0)?,
        host: row.get(1)?,
        port: u16::try_from(port).unwrap_or(0),
        key_type: row.get(3)?,
        fingerprint: row.get(4)?,
        added_at: row.get(5)?,
    })
}

/// The names are compared in lowercase.
fn key(host: &str) -> String {
    host.trim().to_ascii_lowercase()
}

/// All accepted servers, in the order they were accepted.
pub fn list(conn: &Connection) -> Result<Vec<KnownHost>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM known_hosts ORDER BY id"))?;
    let rows = stmt.query_map([], read_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The accepted key of a server, if any.
pub fn find(conn: &Connection, host: &str, port: u16) -> Result<Option<KnownHost>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM known_hosts WHERE host = ?1 AND port = ?2"),
            params![key(host), port],
            read_row,
        )
        .optional()?)
}

/// Accept `fingerprint` for a server. A key accepted earlier for the same host and port is
/// replaced: the user decided again.
pub fn trust(
    conn: &Connection,
    host: &str,
    port: u16,
    key_type: &str,
    fingerprint: &str,
) -> Result<KnownHost> {
    conn.execute(
        "INSERT INTO known_hosts (host, port, key_type, fingerprint) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(host, port) DO UPDATE
            SET key_type = excluded.key_type, fingerprint = excluded.fingerprint,
                added_at = unixepoch()",
        params![key(host), port, key_type, fingerprint],
    )?;
    let found = find(conn, host, port)?;
    found.ok_or_else(|| crate::Error::Corrupt("known host vanished after insert".into()))
}

/// Forget a server. Returns whether it was there.
pub fn forget(conn: &Connection, id: Id) -> Result<bool> {
    Ok(conn.execute("DELETE FROM known_hosts WHERE id = ?1", [id])? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;

    #[test]
    fn trust_find_replace_forget() {
        let conn = open_in_memory().expect("db");
        assert!(find(&conn, "nas.lan", 22).expect("find").is_none());
        let a = trust(&conn, "NAS.lan", 22, "ssh-ed25519", "SHA256:aaa").expect("trust");
        assert_eq!((a.host.as_str(), a.port), ("nas.lan", 22));
        // The name is case-insensitive, the port is part of the identity.
        assert_eq!(find(&conn, "nas.LAN", 22).expect("find"), Some(a.clone()));
        assert!(find(&conn, "nas.lan", 2222).expect("find").is_none());
        // Trusting again replaces the key instead of adding a second row.
        let b = trust(&conn, "nas.lan", 22, "ssh-rsa", "SHA256:bbb").expect("trust again");
        assert_eq!(b.id, a.id);
        assert_eq!(b.fingerprint, "SHA256:bbb");
        assert_eq!(list(&conn).expect("list").len(), 1);
        assert!(forget(&conn, a.id).expect("forget"));
        assert!(!forget(&conn, a.id).expect("forget twice"));
        assert!(list(&conn).expect("list").is_empty());
    }
}
