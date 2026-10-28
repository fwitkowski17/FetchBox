//! FetchBox's own list of programs that plugins may share (SPEC 6.15), `plugins/programs.json`.
//!
//! A plugin names an entry (`"shared": "ffmpeg"` in a dependency) instead of repeating the
//! addresses and hashes of the files. The list is embedded in the program: its addresses and
//! SHA-256 values are vouched for by FetchBox, and change with a FetchBox update, not with the
//! plugin. The program is installed once for every plugin that names it.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use super::manifest::DepSource;

/// A program in the list: pinned, so its version is known before it is downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SharedProgram {
    /// What a dependency names in `shared`.
    pub id: String,
    /// Shown where the dependency has no label of its own.
    pub label: String,
    /// Names the folder it is installed in (`plugin-programs/<id>/<version>/`), so a new version
    /// is a new folder: one path segment, no separators.
    pub version: String,
    /// A file per platform, as in a pinned dependency (each `bin` names the program).
    pub platforms: BTreeMap<String, DepSource>,
}

#[derive(Deserialize)]
struct Document {
    programs: Vec<SharedProgram>,
}

const LIST: &str = include_str!("../../../../plugins/programs.json");

/// The programs FetchBox shares. A list that cannot be read is empty here and is caught by the
/// test of this module, so a typo cannot ship.
pub fn shared_programs() -> &'static [SharedProgram] {
    static PARSED: OnceLock<Vec<SharedProgram>> = OnceLock::new();
    PARSED.get_or_init(|| {
        serde_json::from_str::<Document>(LIST)
            .map(|d| d.programs)
            .unwrap_or_default()
    })
}

/// The entry a dependency's `shared` names.
pub fn shared_program(id: &str) -> Option<&'static SharedProgram> {
    shared_programs().iter().find(|p| p.id == id)
}

/// Is `version` safe to be a folder name?
pub fn is_valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 64
        && !version.starts_with('.')
        && version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.+".contains(&c))
}

#[cfg(test)]
mod tests {
    use super::super::text::{is_safe_relative, is_valid_id, PLATFORMS};
    use super::*;

    #[test]
    fn the_list_is_valid() {
        let doc: Document = serde_json::from_str(LIST).expect("programs.json parses");
        assert!(!doc.programs.is_empty());
        assert_eq!(doc.programs, shared_programs());
        let mut seen = std::collections::BTreeSet::new();
        for p in shared_programs() {
            assert!(is_valid_id(&p.id), "{}", p.id);
            assert!(seen.insert(&p.id), "{} twice", p.id);
            assert!(!p.label.is_empty(), "{}", p.id);
            assert!(is_valid_version(&p.version), "{}: {}", p.id, p.version);
            assert!(!p.platforms.is_empty(), "{}", p.id);
            for (platform, src) in &p.platforms {
                let at = format!("{} for {platform}", p.id);
                assert!(PLATFORMS.contains(&platform.as_str()), "{at}");
                assert!(src.url.starts_with("https://"), "{at}");
                assert!(
                    src.sha256.len() == 64 && src.sha256.bytes().all(|c| c.is_ascii_hexdigit()),
                    "{at}: sha256"
                );
                let bin = src.bin.as_deref().unwrap_or_default();
                assert!(is_safe_relative(bin), "{at}: bin \"{bin}\"");
                assert!(src.size.is_some(), "{at}: a size bounds the download");
            }
        }
    }

    #[test]
    fn ffmpeg_is_there_and_versions_are_folder_names() {
        let ffmpeg = shared_program("ffmpeg").expect("ffmpeg");
        assert!(ffmpeg.platforms.contains_key("linux-x86_64"));
        assert!(shared_program("nope").is_none());
        assert!(is_valid_version("imageio-0.6.0"));
        for bad in ["", ".hidden", "a/b", "..", "a b"] {
            assert!(!is_valid_version(bad), "{bad}");
        }
    }
}
