//! Repository for `plugins` and `plugin_settings` (SPEC 6.15). The files of a plugin are the source
//! of truth for what it is; here is only what the user decided about it.

use std::collections::BTreeMap;

use fetchbox_core::plugin::PluginSource;
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{Error, Result};

/// A row of `plugins`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPlugin {
    pub id: String,
    pub version: String,
    pub enabled: bool,
    pub source: PluginSource,
    /// SHA-256 (hexadecimal) of the `plugin.json` the user approved.
    pub approved_hash: String,
    /// SHA-256 of the ZIP a store install was made from.
    pub store_sha256: Option<String>,
    pub installed_at: i64,
    pub updated_at: Option<i64>,
}

/// What is needed to record an install or an update.
#[derive(Debug, Clone)]
pub struct NewPlugin<'a> {
    pub id: &'a str,
    pub version: &'a str,
    pub source: PluginSource,
    pub approved_hash: &'a str,
    pub store_sha256: Option<&'a str>,
}

const COLUMNS: &str =
    "id, version, enabled, source, approved_hash, store_sha256, installed_at, updated_at";

fn read_row(row: &Row<'_>) -> rusqlite::Result<Result<StoredPlugin>> {
    let source: String = row.get(3)?;
    let plugin = |source| -> rusqlite::Result<StoredPlugin> {
        Ok(StoredPlugin {
            id: row.get(0)?,
            version: row.get(1)?,
            enabled: row.get(2)?,
            source,
            approved_hash: row.get(4)?,
            store_sha256: row.get(5)?,
            installed_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    };
    match PluginSource::parse(&source) {
        Some(source) => plugin(source).map(Ok),
        None => Ok(Err(Error::Corrupt(format!(
            "unknown plugin source {source:?}"
        )))),
    }
}

/// Record an install, or an update of an installed plugin (version, source and approval change;
/// whether it is enabled and its settings stay). A new plugin starts disabled.
pub fn upsert(conn: &Connection, p: &NewPlugin<'_>) -> Result<StoredPlugin> {
    conn.execute(
        "INSERT INTO plugins (id, version, source, approved_hash, store_sha256)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
             version = excluded.version,
             source = excluded.source,
             approved_hash = excluded.approved_hash,
             store_sha256 = excluded.store_sha256,
             updated_at = unixepoch()",
        params![
            p.id,
            p.version,
            p.source.as_str(),
            p.approved_hash,
            p.store_sha256
        ],
    )?;
    get(conn, p.id)?.ok_or_else(|| Error::Corrupt("plugin vanished after upsert".into()))
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<StoredPlugin>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM plugins WHERE id = ?1"),
        [id],
        read_row,
    )
    .optional()?
    .transpose()
}

/// All plugins, by id.
pub fn list(conn: &Connection) -> Result<Vec<StoredPlugin>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM plugins ORDER BY id"))?;
    let rows = stmt.query_map([], read_row)?;
    rows.map(|r| r?).collect()
}

/// Switch a plugin on or off. Returns whether it exists.
pub fn set_enabled(conn: &Connection, id: &str, enabled: bool) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE plugins SET enabled = ?2 WHERE id = ?1",
        params![id, enabled],
    )? > 0)
}

/// Accept another manifest (the user reviewed it). Returns whether the plugin exists.
pub fn set_approved_hash(conn: &Connection, id: &str, hash: &str) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE plugins SET approved_hash = ?2 WHERE id = ?1",
        params![id, hash],
    )? > 0)
}

/// Forget a plugin and its settings. Returns whether it existed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM plugins WHERE id = ?1", [id])? > 0)
}

/// The stored settings of a plugin: key to JSON scalar text.
pub fn settings(conn: &Connection, id: &str) -> Result<BTreeMap<String, String>> {
    let mut stmt = conn.prepare("SELECT key, value FROM plugin_settings WHERE plugin_id = ?1")?;
    let rows = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Change settings in one transaction: `Some(value)` stores it, `None` clears the key.
pub fn save_settings(
    conn: &mut Connection,
    id: &str,
    changes: &[(&str, Option<&str>)],
) -> Result<()> {
    let tx = conn.transaction()?;
    for (key, value) in changes {
        match value {
            Some(v) => tx.execute(
                "INSERT INTO plugin_settings (plugin_id, key, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT(plugin_id, key) DO UPDATE SET value = excluded.value",
                params![id, key, v],
            )?,
            None => tx.execute(
                "DELETE FROM plugin_settings WHERE plugin_id = ?1 AND key = ?2",
                params![id, key],
            )?,
        };
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new<'a>(id: &'a str, version: &'a str, source: PluginSource) -> NewPlugin<'a> {
        NewPlugin {
            id,
            version,
            source,
            approved_hash: "aa",
            store_sha256: None,
        }
    }

    #[test]
    fn a_new_plugin_starts_disabled() {
        let conn = crate::open_in_memory().expect("open");
        let p = upsert(&conn, &new("demo", "1.0.0", PluginSource::Zip)).expect("insert");
        assert!(!p.enabled);
        assert_eq!(p.source, PluginSource::Zip);
        assert_eq!(p.updated_at, None);
        assert!(set_enabled(&conn, "demo", true).expect("enable"));
        assert!(get(&conn, "demo").expect("get").expect("row").enabled);
        assert!(!set_enabled(&conn, "ghost", true).expect("enable"));
    }

    #[test]
    fn an_update_keeps_the_choice_and_the_settings() {
        let mut conn = crate::open_in_memory().expect("open");
        upsert(&conn, &new("demo", "1.0.0", PluginSource::Zip)).expect("insert");
        set_enabled(&conn, "demo", true).expect("enable");
        save_settings(&mut conn, "demo", &[("mode", Some("\"fast\""))]).expect("save");
        let updated = upsert(
            &conn,
            &NewPlugin {
                approved_hash: "bb",
                store_sha256: Some("cc"),
                ..new("demo", "1.1.0", PluginSource::Store)
            },
        )
        .expect("update");
        assert_eq!(updated.version, "1.1.0");
        assert_eq!(updated.source, PluginSource::Store);
        assert_eq!(updated.approved_hash, "bb");
        assert_eq!(updated.store_sha256.as_deref(), Some("cc"));
        assert!(updated.enabled, "an update does not switch it off");
        assert!(updated.updated_at.is_some());
        assert_eq!(settings(&conn, "demo").expect("settings").len(), 1);
    }

    #[test]
    fn settings_are_stored_replaced_and_cleared() {
        let mut conn = crate::open_in_memory().expect("open");
        upsert(&conn, &new("demo", "1.0.0", PluginSource::Folder)).expect("insert");
        save_settings(
            &mut conn,
            "demo",
            &[("a", Some("1")), ("b", Some("\"x\"")), ("c", Some("true"))],
        )
        .expect("save");
        save_settings(&mut conn, "demo", &[("a", Some("2")), ("b", None)]).expect("save");
        let s = settings(&conn, "demo").expect("settings");
        assert_eq!(s.get("a").map(String::as_str), Some("2"));
        assert_eq!(s.get("b"), None);
        assert_eq!(s.get("c").map(String::as_str), Some("true"));
    }

    #[test]
    fn settings_need_their_plugin_and_go_with_it() {
        let mut conn = crate::open_in_memory().expect("open");
        assert!(
            save_settings(&mut conn, "ghost", &[("a", Some("1"))]).is_err(),
            "no plugin, no settings"
        );
        upsert(&conn, &new("demo", "1.0.0", PluginSource::Folder)).expect("insert");
        upsert(&conn, &new("other", "1.0.0", PluginSource::Folder)).expect("insert");
        save_settings(&mut conn, "demo", &[("a", Some("1"))]).expect("save");
        save_settings(&mut conn, "other", &[("a", Some("9"))]).expect("save");
        assert!(delete(&conn, "demo").expect("delete"));
        assert!(!delete(&conn, "demo").expect("delete"));
        assert!(settings(&conn, "demo").expect("settings").is_empty());
        assert_eq!(settings(&conn, "other").expect("settings").len(), 1);
        assert_eq!(list(&conn).expect("list").len(), 1);
    }

    #[test]
    fn plugins_are_listed_by_id() {
        let conn = crate::open_in_memory().expect("open");
        for id in ["zeta", "alpha", "mid"] {
            upsert(&conn, &new(id, "1.0.0", PluginSource::Bundled)).expect("insert");
        }
        let ids: Vec<_> = list(&conn)
            .expect("list")
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, ["alpha", "mid", "zeta"]);
    }

    #[test]
    fn a_changed_manifest_is_approved_again() {
        let conn = crate::open_in_memory().expect("open");
        upsert(&conn, &new("demo", "1.0.0", PluginSource::Folder)).expect("insert");
        assert!(set_approved_hash(&conn, "demo", "ff").expect("approve"));
        assert_eq!(
            get(&conn, "demo").expect("get").expect("row").approved_hash,
            "ff"
        );
        assert!(!set_approved_hash(&conn, "ghost", "ff").expect("approve"));
    }
}
