//! Repository for `torrents` (SPEC 6.13): what FetchBox keeps about a torrent whatever the engine
//! remembers. The numbers that change every second (speed, peers) are not stored.

use fetchbox_core::Id;
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::Result;

/// A row of `torrents` without the `.torrent` file itself (see [`metainfo`]): lists stay light.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTorrent {
    pub id: Id,
    /// 40 lowercase hexadecimal characters.
    pub info_hash: String,
    pub name: String,
    pub magnet: Option<String>,
    /// There is a `.torrent` file stored for it.
    pub has_metainfo: bool,
    pub size: Option<u64>,
    pub file_count: u32,
    /// What the user last chose.
    pub paused: bool,
    /// Bytes sent over all runs.
    pub uploaded: u64,
    pub added_at: i64,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
}

/// What is needed to add a torrent.
#[derive(Debug, Clone)]
pub struct NewTorrent<'a> {
    pub info_hash: &'a str,
    pub name: &'a str,
    pub magnet: Option<&'a str>,
    pub metainfo: Option<&'a [u8]>,
    pub size: Option<u64>,
    pub file_count: u32,
}

const COLUMNS: &str = "id, info_hash, name, magnet, metainfo IS NOT NULL, size, file_count, \
                       paused, uploaded, added_at, finished_at, error";

fn count(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

fn read_row(row: &Row<'_>) -> rusqlite::Result<StoredTorrent> {
    let size: Option<i64> = row.get(5)?;
    let file_count: i64 = row.get(6)?;
    Ok(StoredTorrent {
        id: row.get(0)?,
        info_hash: row.get(1)?,
        name: row.get(2)?,
        magnet: row.get(3)?,
        has_metainfo: row.get(4)?,
        size: size.map(count),
        file_count: u32::try_from(file_count).unwrap_or(0),
        paused: row.get(7)?,
        uploaded: count(row.get(8)?),
        added_at: row.get(9)?,
        finished_at: row.get(10)?,
        error: row.get(11)?,
    })
}

fn num(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// Add a torrent. The hash must be new (`find_by_hash` first): a second row with the same hash is
/// a constraint error.
pub fn insert(conn: &Connection, t: &NewTorrent<'_>) -> Result<StoredTorrent> {
    conn.execute(
        "INSERT INTO torrents (info_hash, name, magnet, metainfo, size, file_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            t.info_hash,
            t.name,
            t.magnet,
            t.metainfo,
            t.size.map(num),
            t.file_count
        ],
    )?;
    get(conn, conn.last_insert_rowid())?
        .ok_or_else(|| crate::Error::Corrupt("torrent vanished after insert".into()))
}

pub fn get(conn: &Connection, id: Id) -> Result<Option<StoredTorrent>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM torrents WHERE id = ?1"),
            [id],
            read_row,
        )
        .optional()?)
}

pub fn find_by_hash(conn: &Connection, info_hash: &str) -> Result<Option<StoredTorrent>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM torrents WHERE info_hash = ?1"),
            [info_hash],
            read_row,
        )
        .optional()?)
}

/// All torrents, newest first.
pub fn list(conn: &Connection) -> Result<Vec<StoredTorrent>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM torrents ORDER BY id DESC"))?;
    let rows = stmt.query_map([], read_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The stored `.torrent` file, if there is one.
pub fn metainfo(conn: &Connection, id: Id) -> Result<Option<Vec<u8>>> {
    Ok(conn
        .query_row("SELECT metainfo FROM torrents WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .flatten())
}

/// The description of the torrent has arrived (a magnet link) or was read from the file: the real
/// name, the size of what is wanted, the number of files and, when known, the `.torrent` file.
pub fn set_description(
    conn: &Connection,
    id: Id,
    name: &str,
    size: u64,
    file_count: u32,
    metainfo: Option<&[u8]>,
) -> Result<()> {
    conn.execute(
        "UPDATE torrents SET name = ?2, size = ?3, file_count = ?4,
                metainfo = COALESCE(?5, metainfo)
         WHERE id = ?1",
        params![id, name, num(size), file_count, metainfo],
    )?;
    Ok(())
}

pub fn set_paused(conn: &Connection, id: Id, paused: bool) -> Result<()> {
    conn.execute(
        "UPDATE torrents SET paused = ?2 WHERE id = ?1",
        params![id, paused],
    )?;
    Ok(())
}

/// Bytes sent so far (only ever grows) and when the last byte arrived. `finished_at` is kept once
/// set, and cleared by [`clear_finished`] when the torrent needs data again.
pub fn set_progress(
    conn: &Connection,
    id: Id,
    uploaded: u64,
    finished_at: Option<i64>,
) -> Result<()> {
    conn.execute(
        "UPDATE torrents SET uploaded = MAX(uploaded, ?2),
                finished_at = COALESCE(finished_at, ?3)
         WHERE id = ?1",
        params![id, num(uploaded), finished_at],
    )?;
    Ok(())
}

pub fn clear_finished(conn: &Connection, id: Id) -> Result<()> {
    conn.execute("UPDATE torrents SET finished_at = NULL WHERE id = ?1", [id])?;
    Ok(())
}

/// Note why a torrent stopped, or clear the note.
pub fn set_error(conn: &Connection, id: Id, error: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE torrents SET error = ?2 WHERE id = ?1",
        params![id, error],
    )?;
    Ok(())
}

/// Remove a torrent. Returns whether it was there.
pub fn remove(conn: &Connection, id: Id) -> Result<bool> {
    Ok(conn.execute("DELETE FROM torrents WHERE id = ?1", [id])? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    fn magnet_row<'a>() -> NewTorrent<'a> {
        NewTorrent {
            info_hash: HASH,
            name: HASH,
            magnet: Some("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"),
            metainfo: None,
            size: None,
            file_count: 0,
        }
    }

    #[test]
    fn a_magnet_gets_its_description_later() {
        let conn = open_in_memory().expect("db");
        let t = insert(&conn, &magnet_row()).expect("insert");
        assert_eq!((t.size, t.file_count, t.has_metainfo), (None, 0, false));
        assert!(!t.paused && t.finished_at.is_none() && t.error.is_none());

        set_description(
            &conn,
            t.id,
            "Ubuntu",
            4_000_000_000,
            1,
            Some(b"d4:infod...e"),
        )
        .expect("set");
        let t = get(&conn, t.id).expect("get").expect("there");
        assert_eq!(
            (t.name.as_str(), t.size, t.file_count),
            ("Ubuntu", Some(4_000_000_000), 1)
        );
        assert!(t.has_metainfo);
        assert_eq!(
            metainfo(&conn, t.id).expect("bytes").as_deref(),
            Some(&b"d4:infod...e"[..])
        );
        // A description without the file keeps the file that is stored.
        set_description(&conn, t.id, "Ubuntu 24", 4_000_000_000, 1, None).expect("again");
        assert!(metainfo(&conn, t.id).expect("bytes").is_some());
    }

    #[test]
    fn the_hash_is_unique_and_found_again() {
        let conn = open_in_memory().expect("db");
        let a = insert(&conn, &magnet_row()).expect("insert");
        assert_eq!(find_by_hash(&conn, HASH).expect("find"), Some(a));
        assert!(
            insert(&conn, &magnet_row()).is_err(),
            "no second row for one torrent"
        );
        assert!(find_by_hash(&conn, &"f".repeat(40))
            .expect("find")
            .is_none());
    }

    #[test]
    fn a_hash_of_the_wrong_length_is_refused() {
        let conn = open_in_memory().expect("db");
        let bad = NewTorrent {
            info_hash: "abc",
            ..magnet_row()
        };
        assert!(insert(&conn, &bad).is_err());
    }

    #[test]
    fn what_was_sent_only_grows_and_the_finish_time_is_kept() {
        let conn = open_in_memory().expect("db");
        let t = insert(&conn, &magnet_row()).expect("insert");
        set_progress(&conn, t.id, 500, None).expect("progress");
        set_progress(&conn, t.id, 300, Some(1000)).expect("lower, finished");
        set_progress(&conn, t.id, 900, Some(2000)).expect("higher, finished again");
        let t = get(&conn, t.id).expect("get").expect("there");
        assert_eq!((t.uploaded, t.finished_at), (900, Some(1000)));
        clear_finished(&conn, t.id).expect("clear");
        assert_eq!(
            get(&conn, t.id).expect("get").expect("there").finished_at,
            None
        );
    }

    #[test]
    fn pause_error_list_and_remove() {
        let conn = open_in_memory().expect("db");
        let a = insert(&conn, &magnet_row()).expect("a");
        let other = "f".repeat(40);
        let b = insert(
            &conn,
            &NewTorrent {
                info_hash: &other,
                name: "b",
                ..magnet_row()
            },
        )
        .expect("b");
        set_paused(&conn, a.id, true).expect("pause");
        set_error(&conn, b.id, Some("disk full")).expect("error");
        let all = list(&conn).expect("list");
        assert_eq!(
            all.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![b.id, a.id],
            "newest first"
        );
        assert!(all[1].paused);
        assert_eq!(all[0].error.as_deref(), Some("disk full"));
        set_error(&conn, b.id, None).expect("clear");
        assert!(remove(&conn, a.id).expect("remove"));
        assert!(!remove(&conn, a.id).expect("twice"));
        assert_eq!(list(&conn).expect("list").len(), 1);
    }
}
