//! Repository for the `downloads` table.

use dlm_core::{Category, Download, Id, SourceKind, Status};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{Error, Result};

/// Values needed to create a download.
#[derive(Debug, Clone)]
pub struct NewDownload {
    pub url: String,
    pub filename: String,
    pub category: Category,
    pub source_kind: SourceKind,
}

const COLUMNS: &str =
    "id, url, filename, category, status, size, downloaded, source_kind, resumable, etag, error";

/// Insert a new download in `queued` state and return its id.
pub fn insert(conn: &Connection, new: &NewDownload) -> Result<Id> {
    conn.execute(
        "INSERT INTO downloads (url, filename, category, source_kind) VALUES (?1, ?2, ?3, ?4)",
        params![
            new.url,
            new.filename,
            new.category.as_str(),
            new.source_kind.as_str()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Fetch one download.
pub fn get(conn: &Connection, id: Id) -> Result<Option<Download>> {
    let row = conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM downloads WHERE id = ?1"),
            [id],
            read_row,
        )
        .optional()?;
    row.transpose()
}

/// All downloads, newest first.
pub fn list(conn: &Connection) -> Result<Vec<Download>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM downloads ORDER BY id DESC"))?;
    let rows = stmt.query_map([], read_row)?;
    rows.map(|r| r?).collect()
}

/// Downloads in a given status, oldest first (queue order).
pub fn list_by_status(conn: &Connection, status: Status) -> Result<Vec<Download>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM downloads WHERE status = ?1 ORDER BY id"
    ))?;
    let rows = stmt.query_map([status.as_str()], read_row)?;
    rows.map(|r| r?).collect()
}

/// Change status. `error` is stored as given (cleared when `None`).
/// Terminal statuses set `finished_at`; other statuses clear it.
pub fn set_status(conn: &Connection, id: Id, status: Status, error: Option<&str>) -> Result<bool> {
    let n = conn.execute(
        "UPDATE downloads
            SET status = ?2, error = ?3,
                finished_at = CASE WHEN ?4 THEN unixepoch() ELSE NULL END
          WHERE id = ?1",
        params![id, status.as_str(), error, status.is_terminal()],
    )?;
    Ok(n > 0)
}

/// Persist what the engine learned about the remote file.
pub fn set_remote_info(
    conn: &Connection,
    id: Id,
    size: Option<u64>,
    resumable: bool,
    etag: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE downloads SET size = ?2, resumable = ?3, etag = ?4 WHERE id = ?1",
        params![id, size.map(to_i64), resumable, etag],
    )?;
    Ok(())
}

/// Persist the total number of downloaded bytes (batched by the caller).
pub fn set_downloaded(conn: &Connection, id: Id, downloaded: u64) -> Result<()> {
    conn.execute(
        "UPDATE downloads SET downloaded = ?2 WHERE id = ?1",
        params![id, to_i64(downloaded)],
    )?;
    Ok(())
}

/// Change the category (manual override).
pub fn set_category(conn: &Connection, id: Id, category: Category) -> Result<bool> {
    let n = conn.execute(
        "UPDATE downloads SET category = ?2 WHERE id = ?1",
        params![id, category.as_str()],
    )?;
    Ok(n > 0)
}

/// Change the final file name (e.g. after collision handling).
pub fn set_filename(conn: &Connection, id: Id, filename: &str) -> Result<()> {
    conn.execute(
        "UPDATE downloads SET filename = ?2 WHERE id = ?1",
        params![id, filename],
    )?;
    Ok(())
}

/// Delete a download; its segments go with it (`ON DELETE CASCADE`).
pub fn delete(conn: &Connection, id: Id) -> Result<bool> {
    Ok(conn.execute("DELETE FROM downloads WHERE id = ?1", [id])? > 0)
}

/// Crash recovery: downloads left `downloading` by a dead process go back to `queued`.
/// Returns how many were reset.
pub fn requeue_interrupted(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE downloads SET status = 'queued' WHERE status = 'downloading'",
        [],
    )?)
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// Map a row; the inner `Result` reports unknown enum values instead of panicking.
fn read_row(row: &Row<'_>) -> rusqlite::Result<Result<Download>> {
    let category: String = row.get(3)?;
    let status: String = row.get(4)?;
    let source_kind: String = row.get(7)?;
    let size: Option<i64> = row.get(5)?;
    let downloaded: i64 = row.get(6)?;
    let id: Id = row.get(0)?;
    let url = row.get(1)?;
    let filename = row.get(2)?;
    let resumable = row.get(8)?;
    let etag = row.get(9)?;
    let error = row.get(10)?;

    let mapped = (|| {
        Ok(Download {
            id,
            url,
            filename,
            category: Category::parse(&category)
                .ok_or_else(|| Error::Corrupt(format!("category {category:?}")))?,
            status: Status::parse(&status)
                .ok_or_else(|| Error::Corrupt(format!("status {status:?}")))?,
            size: size.map(|s| u64::try_from(s).unwrap_or(0)),
            downloaded: u64::try_from(downloaded).unwrap_or(0),
            source_kind: SourceKind::parse(&source_kind)
                .ok_or_else(|| Error::Corrupt(format!("source_kind {source_kind:?}")))?,
            resumable,
            etag,
            error,
        })
    })();
    Ok(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;

    fn new(url: &str) -> NewDownload {
        NewDownload {
            url: url.into(),
            filename: "f.bin".into(),
            category: Category::Other,
            source_kind: SourceKind::Http,
        }
    }

    #[test]
    fn insert_get_roundtrip() {
        let conn = open_in_memory().expect("db");
        let id = insert(&conn, &new("http://x/f.bin")).expect("insert");
        let d = get(&conn, id).expect("get").expect("exists");
        assert_eq!(d.url, "http://x/f.bin");
        assert_eq!(d.status, Status::Queued);
        assert_eq!(d.downloaded, 0);
        assert_eq!(d.size, None);
        assert!(get(&conn, id + 1).expect("get").is_none());
    }

    #[test]
    fn status_transitions_set_finished_at() {
        let conn = open_in_memory().expect("db");
        let id = insert(&conn, &new("http://x/a")).expect("insert");
        let finished = |conn: &Connection| -> Option<i64> {
            conn.query_row("SELECT finished_at FROM downloads WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .expect("query")
        };
        set_status(&conn, id, Status::Failed, Some("boom")).expect("set");
        assert!(finished(&conn).is_some());
        let d = get(&conn, id).expect("get").expect("exists");
        assert_eq!(d.error.as_deref(), Some("boom"));
        set_status(&conn, id, Status::Queued, None).expect("set");
        assert!(finished(&conn).is_none());
        assert_eq!(get(&conn, id).expect("get").expect("e").error, None);
    }

    #[test]
    fn progress_and_remote_info() {
        let conn = open_in_memory().expect("db");
        let id = insert(&conn, &new("http://x/a")).expect("insert");
        set_remote_info(&conn, id, Some(1000), true, Some("\"abc\"")).expect("info");
        set_downloaded(&conn, id, 400).expect("progress");
        let d = get(&conn, id).expect("get").expect("exists");
        assert_eq!(d.size, Some(1000));
        assert_eq!(d.downloaded, 400);
        assert!(d.resumable);
        assert_eq!(d.etag.as_deref(), Some("\"abc\""));
    }

    #[test]
    fn list_orders_and_filters() {
        let conn = open_in_memory().expect("db");
        let a = insert(&conn, &new("http://x/a")).expect("a");
        let b = insert(&conn, &new("http://x/b")).expect("b");
        set_status(&conn, a, Status::Paused, None).expect("set");
        let all: Vec<Id> = list(&conn).expect("list").iter().map(|d| d.id).collect();
        assert_eq!(all, vec![b, a]);
        let queued = list_by_status(&conn, Status::Queued).expect("queued");
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].id, b);
    }

    #[test]
    fn category_override_and_delete() {
        let conn = open_in_memory().expect("db");
        let id = insert(&conn, &new("http://x/a")).expect("insert");
        assert!(set_category(&conn, id, Category::Video).expect("cat"));
        assert_eq!(
            get(&conn, id).expect("get").expect("e").category,
            Category::Video
        );
        assert!(delete(&conn, id).expect("delete"));
        assert!(!delete(&conn, id).expect("delete again"));
    }

    #[test]
    fn requeue_interrupted_only_touches_downloading() {
        let conn = open_in_memory().expect("db");
        let a = insert(&conn, &new("http://x/a")).expect("a");
        let b = insert(&conn, &new("http://x/b")).expect("b");
        set_status(&conn, a, Status::Downloading, None).expect("set");
        set_status(&conn, b, Status::Paused, None).expect("set");
        assert_eq!(requeue_interrupted(&conn).expect("requeue"), 1);
        assert_eq!(get(&conn, a).expect("g").expect("e").status, Status::Queued);
        assert_eq!(get(&conn, b).expect("g").expect("e").status, Status::Paused);
    }
}
