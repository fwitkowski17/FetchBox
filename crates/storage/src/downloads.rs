//! Repository for the `downloads` table.

use fetchbox_core::{
    Category, ChecksumAlgo, ChecksumState, Credentials, Download, Id, SourceKind, Status,
};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{Error, Result};

/// Values needed to create a download.
#[derive(Debug, Clone)]
pub struct NewDownload {
    pub url: String,
    pub filename: String,
    pub category: Category,
    pub source_kind: SourceKind,
    /// Replace an existing file with the same name when the download finishes.
    pub overwrite: bool,
    /// Subdirectory below the disk; empty = none.
    pub subdir: String,
    /// The disk (a folder of the download directory) it is saved on; empty = the directory itself.
    pub disk: String,
    /// The user added it although it did not fit; the engine does not check free space for it.
    pub ignore_space: bool,
    /// Do not start before this moment (unix seconds).
    pub start_at: Option<i64>,
    /// Ignore the global download hours for this download.
    pub start_now: bool,
    /// Verify the finished file against this digest (lowercase hex).
    pub checksum: Option<(ChecksumAlgo, String)>,
    /// The login of the source, kept for as long as the download exists.
    pub credentials: Option<Credentials>,
}

const COLUMNS: &str =
    "id, url, filename, category, status, size, downloaded, source_kind, resumable, etag, error, subdir, waiting_space, archived, speed_limit, start_at, checksum_algo, checksum_expected, checksum_actual, verified, verify_failed, username, disk";

/// Insert a new download in `queued` state and return its id.
pub fn insert(conn: &Connection, new: &NewDownload) -> Result<Id> {
    conn.execute(
        "INSERT INTO downloads (url, filename, category, source_kind, overwrite, subdir, ignore_space, start_at, start_now, checksum_algo, checksum_expected, username, password, disk)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            new.url,
            new.filename,
            new.category.as_str(),
            new.source_kind.as_str(),
            new.overwrite,
            new.subdir,
            new.ignore_space,
            new.start_at,
            new.start_now,
            new.checksum.as_ref().map(|(a, _)| a.as_str()),
            new.checksum.as_ref().map(|(_, v)| v.as_str()),
            new.credentials.as_ref().map(|c| c.username.as_str()),
            new.credentials.as_ref().map(|c| c.password.as_str()),
            new.disk
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
/// Terminal statuses set `finished_at`; other statuses clear it. Leaving `paused` also ends a
/// "waiting for space" hold.
pub fn set_status(conn: &Connection, id: Id, status: Status, error: Option<&str>) -> Result<bool> {
    let n = conn.execute(
        "UPDATE downloads
            SET status = ?2, error = ?3,
                finished_at = CASE WHEN ?4 THEN unixepoch() ELSE NULL END,
                waiting_space = CASE WHEN ?2 = 'paused' THEN waiting_space ELSE 0 END
          WHERE id = ?1",
        params![id, status.as_str(), error, status.is_terminal()],
    )?;
    Ok(n > 0)
}

/// Set the speed limit of one download (bytes/s, 0 = none). Returns whether the id exists.
pub fn set_speed_limit(conn: &Connection, id: Id, rate: u64) -> Result<bool> {
    // SQLite integers are signed 64-bit; no real limit comes anywhere near that.
    let rate = i64::try_from(rate).unwrap_or(i64::MAX);
    let n = conn.execute(
        "UPDATE downloads SET speed_limit = ?2 WHERE id = ?1",
        params![id, rate],
    )?;
    Ok(n > 0)
}

/// Hide a download from the main list (or bring it back). Returns whether the id exists.
pub fn set_archived(conn: &Connection, id: Id, archived: bool) -> Result<bool> {
    let n = conn.execute(
        "UPDATE downloads SET archived = ?2 WHERE id = ?1",
        params![id, archived],
    )?;
    Ok(n > 0)
}

/// "Download now": the global download hours no longer apply to this download, and a start time
/// it was given is dropped. Returns whether the id exists.
pub fn set_start_now(conn: &Connection, id: Id) -> Result<bool> {
    let n = conn.execute(
        "UPDATE downloads SET start_now = 1, start_at = NULL WHERE id = ?1",
        [id],
    )?;
    Ok(n > 0)
}

/// Whether the download ignores the global download hours.
pub fn start_now_flag(conn: &Connection, id: Id) -> Result<bool> {
    Ok(conn
        .query_row("SELECT start_now FROM downloads WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .unwrap_or(false))
}

/// Mark a (paused) download as held back because it does not fit on the disk, or clear the mark.
pub fn set_waiting_space(conn: &Connection, id: Id, waiting: bool) -> Result<()> {
    conn.execute(
        "UPDATE downloads SET waiting_space = ?2 WHERE id = ?1",
        params![id, waiting],
    )?;
    Ok(())
}

/// Change status only if the current one is in `from` (guards against late engine events).
/// Returns whether the transition happened.
pub fn transition(
    conn: &Connection,
    id: Id,
    from: &[Status],
    to: Status,
    error: Option<&str>,
) -> Result<bool> {
    let Some(current) = get(conn, id)? else {
        return Ok(false);
    };
    if !from.contains(&current.status) {
        return Ok(false);
    }
    set_status(conn, id, to, error)
}

/// Whether the user chose to overwrite an existing file for this download.
pub fn overwrite_flag(conn: &Connection, id: Id) -> Result<bool> {
    Ok(conn
        .query_row("SELECT overwrite FROM downloads WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .unwrap_or(false))
}

/// Whether the user chose to add this download although it did not fit.
pub fn ignore_space_flag(conn: &Connection, id: Id) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT ignore_space FROM downloads WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false))
}

/// Hand a download to a plugin's own program (SPEC 6.15): `runner` is `<plugin>/<resolver>`, `choice`
/// the id of the preset. Returns whether the id exists.
pub fn set_runner(conn: &Connection, id: Id, runner: &str, choice: Option<&str>) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE downloads SET runner = ?2, runner_ref = ?3 WHERE id = ?1",
        params![id, runner, choice],
    )? > 0)
}

/// The runner of a download and its preset, if a plugin's program makes the file.
pub fn runner(conn: &Connection, id: Id) -> Result<Option<(String, Option<String>)>> {
    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT runner, runner_ref FROM downloads WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(runner, choice)| runner.map(|r| (r, choice))))
}

/// The login of a download, if it has one. Kept apart from [`Download`] so that it can not reach
/// the API by accident.
pub fn credentials(conn: &Connection, id: Id) -> Result<Option<Credentials>> {
    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT username, password FROM downloads WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(username, password)| {
        username.map(|username| Credentials {
            username,
            password: password.unwrap_or_default(),
        })
    }))
}

/// Downloads of this URL that still count (not failed/cancelled), oldest first.
pub fn find_by_url(conn: &Connection, url: &str) -> Result<Vec<Download>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM downloads
          WHERE url = ?1 AND status NOT IN ('failed', 'cancelled') ORDER BY id"
    ))?;
    let rows = stmt.query_map([url], read_row)?;
    rows.map(|r| r?).collect()
}

/// Unfinished download (queued, downloading, paused) that will produce `disk/subdir/filename`.
pub fn find_unfinished_by_filename(
    conn: &Connection,
    disk: &str,
    subdir: &str,
    filename: &str,
) -> Result<Option<Download>> {
    let row = conn
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM downloads
                  WHERE filename = ?1 AND subdir = ?2 AND disk = ?3
                    AND status IN ('queued', 'downloading', 'paused')
                  ORDER BY id LIMIT 1"
            ),
            params![filename, subdir, disk],
            read_row,
        )
        .optional()?;
    row.transpose()
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

/// Record the total size once it is known (e.g. a plain stream that just finished).
pub fn set_size(conn: &Connection, id: Id, size: u64) -> Result<()> {
    conn.execute(
        "UPDATE downloads SET size = ?2 WHERE id = ?1",
        params![id, to_i64(size)],
    )?;
    Ok(())
}

/// Persist the total number of downloaded bytes (batched by the caller).
/// Only applies while the download is `downloading`, so late engine events cannot
/// touch a download the user has meanwhile paused, cancelled or finished.
pub fn set_downloaded(conn: &Connection, id: Id, downloaded: u64) -> Result<()> {
    conn.execute(
        "UPDATE downloads SET downloaded = ?2 WHERE id = ?1 AND status = 'downloading'",
        params![id, to_i64(downloaded)],
    )?;
    Ok(())
}

/// What a verification came to. `verified`: the file matched. Otherwise `actual` is the digest
/// the file really has (`None` when it could not be read) and the download is marked as having
/// failed its verification. Returns whether the id exists.
pub fn set_verification(
    conn: &Connection,
    id: Id,
    verified: bool,
    actual: Option<&str>,
) -> Result<bool> {
    let n = conn.execute(
        "UPDATE downloads SET verified = ?2, verify_failed = NOT ?2, checksum_actual = ?3 WHERE id = ?1",
        params![id, verified, if verified { None } else { actual }],
    )?;
    Ok(n > 0)
}

/// The user keeps the file whatever the verification said: it counts as completed, not verified.
pub fn clear_verification(conn: &Connection, id: Id) -> Result<bool> {
    let n = conn.execute(
        "UPDATE downloads SET verified = 0, verify_failed = 0, checksum_actual = NULL WHERE id = ?1",
        [id],
    )?;
    Ok(n > 0)
}

/// Back to the beginning for a new download of the same entry (after a failed verification): no
/// bytes, no stored plan, no verdict. The caller removes the segments and the file.
pub fn reset_for_redownload(conn: &Connection, id: Id) -> Result<bool> {
    let n = conn.execute(
        "UPDATE downloads
            SET downloaded = 0, etag = NULL, verified = 0, verify_failed = 0, checksum_actual = NULL
          WHERE id = ?1",
        [id],
    )?;
    Ok(n > 0)
}

/// Downloads whose files are being verified (to be started again after a restart).
pub fn list_verifying(conn: &Connection) -> Result<Vec<Download>> {
    list_by_status(conn, Status::Verifying)
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
    let subdir = row.get(11)?;
    let waiting_space = row.get(12)?;
    let archived = row.get(13)?;
    let speed_limit: i64 = row.get(14)?;
    let start_at = row.get(15)?;
    let checksum_algo: Option<String> = row.get(16)?;
    let checksum_expected: Option<String> = row.get(17)?;
    let checksum_actual: Option<String> = row.get(18)?;
    let verified = row.get(19)?;
    let verify_failed = row.get(20)?;
    let username = row.get(21)?;
    let disk = row.get(22)?;

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
            username,
            subdir,
            disk,
            speed: 0,
            waiting_space,
            archived,
            speed_limit: u64::try_from(speed_limit).unwrap_or(0),
            start_at,
            hold: None,
            starts_at: None,
            checksum: match (checksum_algo, checksum_expected) {
                (Some(algo), Some(expected)) => Some(ChecksumState {
                    algo: ChecksumAlgo::parse(&algo)
                        .ok_or_else(|| Error::Corrupt(format!("checksum_algo {algo:?}")))?,
                    expected,
                    actual: checksum_actual,
                }),
                _ => None,
            },
            verified,
            verify_failed,
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
            overwrite: false,
            ignore_space: false,
            start_at: None,
            start_now: false,
            subdir: String::new(),
            disk: String::new(),
            checksum: None,
            credentials: None,
        }
    }

    #[test]
    fn a_login_is_stored_apart_from_the_download() {
        let conn = open_in_memory().expect("db");
        let mut with_login = new("ftp://nas.lan/pub/a.iso");
        with_login.source_kind = SourceKind::Ftp;
        with_login.credentials = Some(Credentials {
            username: "filip".into(),
            password: "s3cret".into(),
        });
        let id = insert(&conn, &with_login).expect("insert");
        let d = get(&conn, id).expect("get").expect("exists");
        assert_eq!(d.source_kind, SourceKind::Ftp);
        assert_eq!(d.username.as_deref(), Some("filip"));
        // The password is not part of what is read into the model (and so never serialized).
        assert!(!format!("{d:?}").contains("s3cret"));
        let login = credentials(&conn, id).expect("credentials").expect("some");
        assert_eq!(
            (login.username.as_str(), login.password.as_str()),
            ("filip", "s3cret")
        );

        let plain = insert(&conn, &new("http://x/f.bin")).expect("insert");
        assert!(credentials(&conn, plain).expect("credentials").is_none());
        assert_eq!(get(&conn, plain).expect("get").expect("row").username, None);

        // A login without a password is still a login.
        let mut no_pass = new("ftp://nas.lan/b");
        no_pass.credentials = Some(Credentials {
            username: "anonymous".into(),
            password: String::new(),
        });
        let id = insert(&conn, &no_pass).expect("insert");
        let login = credentials(&conn, id).expect("credentials").expect("some");
        assert_eq!(login.password, "");
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
        set_status(&conn, id, Status::Downloading, None).expect("status");
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
    fn a_checksum_is_stored_and_the_verdict_recorded() {
        let conn = open_in_memory().expect("db");
        let digest = "d41d8cd98f00b204e9800998ecf8427e".to_owned();
        let mut with = new("http://x/c");
        with.checksum = Some((ChecksumAlgo::Md5, digest.clone()));
        let id = insert(&conn, &with).expect("insert");
        let d = get(&conn, id).expect("get").expect("e");
        let state = d.checksum.expect("checksum");
        assert_eq!(
            (state.algo, state.expected.as_str()),
            (ChecksumAlgo::Md5, digest.as_str())
        );
        assert_eq!(state.actual, None);
        assert!(!d.verified && !d.verify_failed);
        assert!(get(&conn, insert(&conn, &new("http://x/n")).expect("n"))
            .expect("get")
            .expect("e")
            .checksum
            .is_none());

        set_status(&conn, id, Status::Verifying, None).expect("verifying");
        assert_eq!(list_verifying(&conn).expect("list").len(), 1);
        assert!(!Status::Verifying.is_terminal());
        set_verification(&conn, id, false, Some("ffff")).expect("verdict");
        set_status(&conn, id, Status::Failed, Some("checksum mismatch")).expect("failed");
        let d = get(&conn, id).expect("get").expect("e");
        assert!(d.verify_failed && !d.verified);
        assert_eq!(d.checksum.expect("c").actual.as_deref(), Some("ffff"));

        // Keeping the file: completed, but not verified.
        clear_verification(&conn, id).expect("clear");
        set_status(&conn, id, Status::Completed, None).expect("completed");
        let d = get(&conn, id).expect("get").expect("e");
        assert!(!d.verified && !d.verify_failed);
        assert_eq!(d.checksum.expect("c").actual, None);

        set_verification(&conn, id, true, None).expect("ok");
        assert!(get(&conn, id).expect("get").expect("e").verified);
        set_downloaded(&conn, id, 5).expect("noop outside downloading");
        reset_for_redownload(&conn, id).expect("reset");
        let d = get(&conn, id).expect("get").expect("e");
        assert!(!d.verified && d.downloaded == 0 && d.etag.is_none());
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

    #[test]
    fn transition_respects_expected_status() {
        let conn = open_in_memory().expect("db");
        let id = insert(&conn, &new("http://x/a")).expect("insert");
        assert!(transition(&conn, id, &[Status::Queued], Status::Downloading, None).expect("t"));
        // A late event must not resurrect a cancelled download.
        set_status(&conn, id, Status::Cancelled, None).expect("set");
        assert!(!transition(
            &conn,
            id,
            &[Status::Queued, Status::Downloading],
            Status::Paused,
            None
        )
        .expect("t"));
        assert_eq!(
            get(&conn, id).expect("g").expect("e").status,
            Status::Cancelled
        );
        assert!(!transition(&conn, 999, &[Status::Queued], Status::Paused, None).expect("t"));
    }

    #[test]
    fn progress_ignored_unless_downloading() {
        let conn = open_in_memory().expect("db");
        let id = insert(&conn, &new("http://x/a")).expect("insert");
        set_downloaded(&conn, id, 50).expect("p");
        assert_eq!(get(&conn, id).expect("g").expect("e").downloaded, 0);
        set_status(&conn, id, Status::Downloading, None).expect("s");
        set_downloaded(&conn, id, 50).expect("p");
        assert_eq!(get(&conn, id).expect("g").expect("e").downloaded, 50);
    }

    #[test]
    fn conflict_lookups() {
        let conn = open_in_memory().expect("db");
        let a = insert(&conn, &new("http://x/a")).expect("a");
        assert_eq!(find_by_url(&conn, "http://x/a").expect("f").len(), 1);
        assert!(find_unfinished_by_filename(&conn, "", "", "f.bin")
            .expect("f")
            .is_some());
        set_status(&conn, a, Status::Completed, None).expect("s");
        assert_eq!(find_by_url(&conn, "http://x/a").expect("f").len(), 1);
        assert!(find_unfinished_by_filename(&conn, "", "", "f.bin")
            .expect("f")
            .is_none());
        set_status(&conn, a, Status::Cancelled, None).expect("s");
        assert!(find_by_url(&conn, "http://x/a").expect("f").is_empty());
    }

    #[test]
    fn speed_limit_roundtrip() {
        let conn = open_in_memory().expect("db");
        let id = insert(&conn, &new("http://x/a")).expect("insert");
        assert_eq!(get(&conn, id).expect("get").expect("row").speed_limit, 0);
        assert!(set_speed_limit(&conn, id, 500_000).expect("set"));
        assert_eq!(
            get(&conn, id).expect("get").expect("row").speed_limit,
            500_000
        );
        // It survives status changes and can be removed again.
        set_status(&conn, id, Status::Paused, None).expect("pause");
        assert_eq!(
            get(&conn, id).expect("get").expect("row").speed_limit,
            500_000
        );
        assert!(set_speed_limit(&conn, id, 0).expect("clear"));
        assert_eq!(get(&conn, id).expect("get").expect("row").speed_limit, 0);
        assert!(!set_speed_limit(&conn, 999, 1).expect("unknown id"));
    }

    #[test]
    fn archived_flag_roundtrip() {
        let conn = open_in_memory().expect("db");
        let id = insert(&conn, &new("http://x/a")).expect("insert");
        assert!(!get(&conn, id).expect("get").expect("row").archived);
        assert!(set_archived(&conn, id, true).expect("archive"));
        assert!(get(&conn, id).expect("get").expect("row").archived);
        // Status changes do not touch it.
        set_status(&conn, id, Status::Completed, None).expect("complete");
        assert!(get(&conn, id).expect("get").expect("row").archived);
        assert!(set_archived(&conn, id, false).expect("restore"));
        assert!(!get(&conn, id).expect("get").expect("row").archived);
        assert!(!set_archived(&conn, 999, true).expect("unknown id"));
    }

    #[test]
    fn waiting_space_hold_ends_when_the_download_leaves_paused() {
        let conn = open_in_memory().expect("db");
        let id = insert(&conn, &new("http://x/a")).expect("insert");
        assert!(!get(&conn, id).expect("get").expect("row").waiting_space);

        set_status(&conn, id, Status::Paused, None).expect("pause");
        set_waiting_space(&conn, id, true).expect("hold");
        assert!(get(&conn, id).expect("get").expect("row").waiting_space);
        // Staying paused keeps the hold...
        set_status(&conn, id, Status::Paused, None).expect("pause again");
        assert!(get(&conn, id).expect("get").expect("row").waiting_space);
        // ...queueing it (resume) ends it.
        set_status(&conn, id, Status::Queued, None).expect("queue");
        assert!(!get(&conn, id).expect("get").expect("row").waiting_space);
    }

    #[test]
    fn ignore_space_flag_is_stored() {
        let conn = open_in_memory().expect("db");
        let mut n = new("http://x/a");
        n.ignore_space = true;
        let id = insert(&conn, &n).expect("insert");
        assert!(ignore_space_flag(&conn, id).expect("flag"));
        let id2 = insert(&conn, &new("http://x/b")).expect("insert");
        assert!(!ignore_space_flag(&conn, id2).expect("flag"));
        assert!(
            !ignore_space_flag(&conn, 999).expect("flag"),
            "unknown id: false"
        );
    }

    #[test]
    fn overwrite_flag_is_stored() {
        let conn = open_in_memory().expect("db");
        let mut n = new("http://x/a");
        n.overwrite = true;
        let id = insert(&conn, &n).expect("insert");
        assert!(overwrite_flag(&conn, id).expect("f"));
        let id2 = insert(&conn, &new("http://x/b")).expect("insert");
        assert!(!overwrite_flag(&conn, id2).expect("f"));
    }

    #[test]
    fn name_reservation_is_per_disk() {
        let conn = open_in_memory().expect("db");
        let mut n = new("http://x/a");
        n.disk = "ssd1".into();
        n.subdir = "video".into();
        insert(&conn, &n).expect("insert");
        assert!(find_unfinished_by_filename(&conn, "ssd1", "video", "f.bin")
            .expect("f")
            .is_some());
        // The same folder on another disk, or on the download directory, is another place.
        for disk in ["", "hdd10tb"] {
            assert!(find_unfinished_by_filename(&conn, disk, "video", "f.bin")
                .expect("f")
                .is_none());
        }
        let d = list(&conn).expect("list");
        assert_eq!(
            (d[0].disk.as_str(), d[0].subdir.as_str()),
            ("ssd1", "video")
        );
    }

    #[test]
    fn a_download_from_before_disks_is_on_the_download_directory() {
        let conn = open_in_memory().expect("db");
        insert(&conn, &new("http://x/a")).expect("insert");
        assert_eq!(list(&conn).expect("list")[0].disk, "");
    }

    #[test]
    fn name_reservation_is_per_subdirectory() {
        let conn = open_in_memory().expect("db");
        let mut n = new("http://x/a");
        n.subdir = "video".into();
        insert(&conn, &n).expect("insert");
        assert!(find_unfinished_by_filename(&conn, "", "video", "f.bin")
            .expect("f")
            .is_some());
        assert!(find_unfinished_by_filename(&conn, "", "", "f.bin")
            .expect("f")
            .is_none());
        assert!(find_unfinished_by_filename(&conn, "", "music", "f.bin")
            .expect("f")
            .is_none());
        let d = list(&conn).expect("list");
        assert_eq!(d[0].subdir, "video");
    }

    #[test]
    fn a_start_time_and_download_now_are_stored() {
        let conn = open_in_memory().expect("db");
        let mut n = new("http://x/a");
        n.start_at = Some(1_900_000_000);
        let id = insert(&conn, &n).expect("insert");
        assert_eq!(
            get(&conn, id).expect("get").expect("row").start_at,
            Some(1_900_000_000)
        );
        assert!(!start_now_flag(&conn, id).expect("flag"));

        // "Download now" ends the wait for a start time too.
        assert!(set_start_now(&conn, id).expect("set"));
        assert!(start_now_flag(&conn, id).expect("flag"));
        assert_eq!(get(&conn, id).expect("get").expect("row").start_at, None);
        assert!(!set_start_now(&conn, id + 1).expect("missing id"));

        let mut now = new("http://x/b");
        now.start_now = true;
        let id2 = insert(&conn, &now).expect("insert");
        assert!(start_now_flag(&conn, id2).expect("flag"));
    }

    #[test]
    fn a_download_made_by_a_plugin_remembers_its_runner() {
        let conn = open_in_memory().expect("db");
        let plain = insert(&conn, &new("http://x/a")).expect("insert");
        let made = insert(&conn, &new("https://video.example/watch?v=1")).expect("insert");
        assert_eq!(runner(&conn, plain).expect("runner"), None);
        assert!(set_runner(&conn, made, "yt-dlp/media", Some("best")).expect("set"));
        assert_eq!(
            runner(&conn, made).expect("runner"),
            Some(("yt-dlp/media".to_owned(), Some("best".to_owned())))
        );
        assert!(set_runner(&conn, made, "yt-dlp/media", None).expect("set"));
        assert_eq!(
            runner(&conn, made).expect("runner"),
            Some(("yt-dlp/media".to_owned(), None))
        );
        assert_eq!(runner(&conn, plain).expect("runner"), None);
        assert!(!set_runner(&conn, 9999, "x/y", None).expect("missing id"));
        assert_eq!(runner(&conn, 9999).expect("runner"), None);
    }
}
