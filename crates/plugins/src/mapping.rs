//! Turning the JSON a plugin's probe printed into entries for the add dialog, by the JSON pointers
//! the manifest names (`fetchbox_core::plugin::ResultMap`). No code of the plugin is involved: it
//! only has to print JSON.

use fetchbox_core::plugin::{Pointers, ResultMap};
use serde_json::Value;

use crate::{Error, Result};

/// Most entries kept from one answer (the same cap as a directory listing).
pub const MAX_ENTRIES: usize = 2000;

/// One item of a playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub url: String,
    pub size: Option<u64>,
}

/// What a probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mapped {
    /// One file; the address is the one that was asked about.
    Single { name: String, size: Option<u64> },
    /// A list of things to choose from.
    List {
        items: Vec<Item>,
        skipped: usize,
        truncated: bool,
    },
}

/// A text from the first pointer that finds something usable (a string, or a number as text).
fn text(v: &Value, ptrs: &Pointers) -> Option<String> {
    ptrs.iter().find_map(|p| match v.pointer(p)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

/// A size from the first pointer that finds a number or a numeric string.
fn size(v: &Value, ptrs: Option<&Pointers>) -> Option<u64> {
    ptrs?.iter().find_map(|p| match v.pointer(p)? {
        Value::Number(n) => n.as_u64().or_else(|| {
            // yt-dlp prints estimates as floats.
            n.as_f64()
                .filter(|f| *f >= 0.0 && f.is_finite())
                .map(|f| f as u64)
        }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    })
}

/// Apply the mapping to the JSON a probe printed.
pub fn map_result(value: &Value, map: &ResultMap) -> Result<Mapped> {
    if let (Some(items_ptr), Some(item)) = (&map.items, &map.item) {
        // A list when the pointer finds an array; otherwise the answer may still be a single file.
        if let Some(Value::Array(list)) = value.pointer(items_ptr) {
            let mut items = Vec::new();
            let mut skipped = 0;
            for v in list {
                match (text(v, &item.name), text(v, &item.url)) {
                    (Some(name), Some(url)) => {
                        if items.len() < MAX_ENTRIES {
                            items.push(Item {
                                name,
                                url,
                                size: size(v, item.size.as_ref()),
                            });
                        }
                    }
                    _ => skipped += 1,
                }
            }
            let truncated = list.len() - skipped > items.len();
            if items.is_empty() {
                return Err(Error::Run(
                    "the plugin found nothing to download there".into(),
                ));
            }
            return Ok(Mapped::List {
                items,
                skipped,
                truncated,
            });
        }
    }
    let Some(single) = &map.single else {
        return Err(Error::Run(
            "the plugin's answer has no list where it should".into(),
        ));
    };
    let name = text(value, &single.name)
        .ok_or_else(|| Error::Run("the plugin's answer has no name for the file".into()))?;
    Ok(Mapped::Single {
        name,
        size: size(value, single.size.as_ref()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(v: Value) -> ResultMap {
        serde_json::from_value(v).expect("map")
    }

    fn yt() -> ResultMap {
        map(json!({
            "items": "/entries",
            "item": {"name": "/title", "url": ["/url", "/webpage_url"], "size": "/filesize"},
            "single": {"name": "/title", "size": ["/filesize", "/filesize_approx"]}
        }))
    }

    #[test]
    fn a_single_video_is_named_and_sized() {
        let out = map_result(
            &json!({"title": "A video", "filesize_approx": 1234.7}),
            &yt(),
        )
        .expect("ok");
        assert_eq!(
            out,
            Mapped::Single {
                name: "A video".into(),
                size: Some(1234)
            }
        );
        let out = map_result(&json!({"title": "No size"}), &yt()).expect("ok");
        assert_eq!(
            out,
            Mapped::Single {
                name: "No size".into(),
                size: None
            }
        );
    }

    #[test]
    fn a_playlist_becomes_a_list_and_skips_what_is_unusable() {
        let doc = json!({
            "title": "Playlist",
            "entries": [
                {"title": "One", "url": "https://v/1", "filesize": 10},
                {"title": "Two", "webpage_url": "https://v/2", "filesize": "20"},
                {"title": "No address"},
                {"url": "https://v/4"},
                {"title": 5, "url": "https://v/5"},
                "junk"
            ]
        });
        let Mapped::List {
            items,
            skipped,
            truncated,
        } = map_result(&doc, &yt()).expect("ok")
        else {
            panic!("a list");
        };
        assert_eq!(skipped, 3);
        assert!(!truncated);
        assert_eq!(
            items,
            vec![
                Item {
                    name: "One".into(),
                    url: "https://v/1".into(),
                    size: Some(10)
                },
                Item {
                    name: "Two".into(),
                    url: "https://v/2".into(),
                    size: Some(20)
                },
                Item {
                    name: "5".into(),
                    url: "https://v/5".into(),
                    size: None
                },
            ]
        );
    }

    #[test]
    fn a_list_is_capped() {
        let entries: Vec<_> = (0..MAX_ENTRIES + 5)
            .map(|i| json!({"title": format!("t{i}"), "url": format!("https://v/{i}")}))
            .collect();
        let Mapped::List {
            items, truncated, ..
        } = map_result(&json!({"entries": entries}), &yt()).expect("ok")
        else {
            panic!("a list");
        };
        assert_eq!(items.len(), MAX_ENTRIES);
        assert!(truncated);
    }

    #[test]
    fn an_empty_list_or_a_nameless_file_is_an_error() {
        let e = map_result(&json!({"entries": []}), &yt()).expect_err("empty");
        assert!(e.to_string().contains("nothing to download"), "{e}");
        let e = map_result(&json!({"entries": [{"x": 1}]}), &yt()).expect_err("unusable");
        assert!(e.to_string().contains("nothing to download"), "{e}");
        let e = map_result(&json!({"id": "abc"}), &yt()).expect_err("no name");
        assert!(e.to_string().contains("no name"), "{e}");
    }

    #[test]
    fn a_mapping_without_a_list_part_still_reads_a_single_file() {
        let only_single = map(json!({"single": {"name": "/n"}}));
        let out = map_result(&json!({"n": "x", "entries": [1]}), &only_single).expect("ok");
        assert_eq!(
            out,
            Mapped::Single {
                name: "x".into(),
                size: None
            }
        );
        let only_list = map(json!({"items": "/e", "item": {"name": "/n", "url": "/u"}}));
        let e = map_result(&json!({"n": "x"}), &only_list).expect_err("not a list");
        assert!(e.to_string().contains("no list"), "{e}");
    }
}
