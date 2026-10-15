//! Repository for the `segments` table (resume offsets, SPEC 6.2).

use fetchbox_core::Id;
use rusqlite::{params, Connection};

use crate::Result;

/// One persisted segment: inclusive byte range plus bytes already written.
pub use fetchbox_core::SegmentState as SegmentRow;

/// Replace all segments of a download with a fresh plan (single transaction).
pub fn replace(conn: &mut Connection, download_id: Id, rows: &[SegmentRow]) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM segments WHERE download_id = ?1", [download_id])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO segments (download_id, idx, start, \"end\", downloaded)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for r in rows {
            stmt.execute(params![
                download_id,
                r.idx,
                to_i64(r.start),
                to_i64(r.end),
                to_i64(r.downloaded)
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Segments of a download ordered by index.
pub fn list(conn: &Connection, download_id: Id) -> Result<Vec<SegmentRow>> {
    let mut stmt = conn.prepare(
        "SELECT idx, start, \"end\", downloaded FROM segments
          WHERE download_id = ?1 ORDER BY idx",
    )?;
    let rows = stmt.query_map([download_id], |r| {
        Ok(SegmentRow {
            idx: r.get(0)?,
            start: to_u64(r.get(1)?),
            end: to_u64(r.get(2)?),
            downloaded: to_u64(r.get(3)?),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Batched offset update: `(idx, downloaded)` pairs in one transaction.
pub fn update_offsets(
    conn: &mut Connection,
    download_id: Id,
    offsets: &[(u32, u64)],
) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt =
            tx.prepare("UPDATE segments SET downloaded = ?3 WHERE download_id = ?1 AND idx = ?2")?;
        for (idx, downloaded) in offsets {
            stmt.execute(params![download_id, idx, to_i64(*downloaded)])?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn to_u64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::downloads::{self, NewDownload};
    use crate::open_in_memory;
    use fetchbox_core::{Category, SourceKind};

    fn seed(conn: &Connection) -> Id {
        downloads::insert(
            conn,
            &NewDownload {
                url: "http://x/a".into(),
                filename: "a".into(),
                category: Category::Other,
                source_kind: SourceKind::Http,
                overwrite: false,
                ignore_space: false,
                subdir: String::new(),
            },
        )
        .expect("insert")
    }

    fn plan() -> Vec<SegmentRow> {
        vec![
            SegmentRow {
                idx: 0,
                start: 0,
                end: 499,
                downloaded: 0,
            },
            SegmentRow {
                idx: 1,
                start: 500,
                end: 999,
                downloaded: 0,
            },
        ]
    }

    #[test]
    fn replace_and_list() {
        let mut conn = open_in_memory().expect("db");
        let id = seed(&conn);
        replace(&mut conn, id, &plan()).expect("replace");
        assert_eq!(list(&conn, id).expect("list"), plan());
        // Replacing again must not duplicate rows.
        replace(&mut conn, id, &plan()[..1]).expect("replace");
        assert_eq!(list(&conn, id).expect("list").len(), 1);
    }

    #[test]
    fn offsets_are_updated_in_batch() {
        let mut conn = open_in_memory().expect("db");
        let id = seed(&conn);
        replace(&mut conn, id, &plan()).expect("replace");
        update_offsets(&mut conn, id, &[(0, 120), (1, 30)]).expect("update");
        let got: Vec<u64> = list(&conn, id)
            .expect("list")
            .iter()
            .map(|s| s.downloaded)
            .collect();
        assert_eq!(got, vec![120, 30]);
    }

    #[test]
    fn deleting_download_cascades() {
        let mut conn = open_in_memory().expect("db");
        let id = seed(&conn);
        replace(&mut conn, id, &plan()).expect("replace");
        downloads::delete(&conn, id).expect("delete");
        assert!(list(&conn, id).expect("list").is_empty());
    }
}
