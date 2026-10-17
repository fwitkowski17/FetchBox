//! Reading magnet links and `.torrent` files without a session: what they are called, their
//! identity and the trackers they name. Used to check a request before anything is started.

use librqbit::{torrent_from_bytes, Magnet};

use crate::TorrentError;

/// What a magnet link or a `.torrent` file says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// 40 lowercase hexadecimal characters.
    pub info_hash: String,
    /// The torrent's name (a magnet link's `dn`, which may be absent or empty).
    pub name: Option<String>,
    /// The tracker addresses, as written.
    pub trackers: Vec<String>,
    /// Total size and number of files; only a `.torrent` file knows them.
    pub contents: Option<Contents>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contents {
    pub size: u64,
    pub file_count: u32,
}

pub fn parse_magnet(uri: &str) -> Result<Parsed, TorrentError> {
    let magnet = Magnet::parse(uri.trim())
        .map_err(|_| TorrentError::Invalid("this is not a valid magnet link".into()))?;
    let info_hash = magnet.as_id20().ok_or_else(|| {
        TorrentError::Invalid(
            "the magnet link has no BitTorrent v1 info hash (xt=urn:btih:...)".into(),
        )
    })?;
    Ok(Parsed {
        info_hash: info_hash.as_string(),
        name: magnet.name.filter(|n| !n.trim().is_empty()),
        trackers: magnet.trackers,
        contents: None,
    })
}

pub fn parse_torrent_file(bytes: &[u8]) -> Result<Parsed, TorrentError> {
    let torrent = torrent_from_bytes(bytes)
        .map_err(|_| TorrentError::Invalid("this is not a valid .torrent file".into()))?;
    let trackers: Vec<String> = torrent
        .iter_announce()
        .filter_map(|t| std::str::from_utf8(t.as_ref()).ok())
        .map(str::to_owned)
        .collect();
    let info_hash = torrent.info_hash.as_string();
    let info = torrent
        .info
        .data
        .validate()
        .map_err(|e| TorrentError::Invalid(format!("the .torrent file is not usable: {e}")))?;
    let (mut size, mut file_count) = (0u64, 0u32);
    for f in info.iter_file_details() {
        size = size.saturating_add(f.len);
        file_count = file_count.saturating_add(1);
    }
    Ok(Parsed {
        info_hash,
        name: info
            .name()
            .map(|n| n.into_owned())
            .filter(|n| !n.trim().is_empty()),
        trackers,
        contents: Some(Contents { size, file_count }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn a_magnet_link_gives_its_hash_name_and_trackers() {
        let p = parse_magnet(&format!(
            "magnet:?xt=urn:btih:{HASH}&dn=Some+Name&tr=udp%3A%2F%2Ftracker.example%3A80"
        ))
        .expect("parses");
        assert_eq!(p.info_hash, HASH);
        assert_eq!(p.name.as_deref(), Some("Some Name"));
        assert_eq!(p.trackers, vec!["udp://tracker.example:80"]);
        assert!(p.contents.is_none());
    }

    #[test]
    fn the_hash_is_lowercased_and_a_base32_one_is_understood() {
        let upper =
            parse_magnet(&format!("magnet:?xt=urn:btih:{}", HASH.to_uppercase())).expect("upper");
        assert_eq!(upper.info_hash, HASH);
        // 32 characters of base32 are the same 20 bytes.
        let b32 =
            parse_magnet("magnet:?xt=urn:btih:AERUKZ4JVPG66AJDIVTYTK6N54ASGRLH").expect("base32");
        assert_eq!(b32.info_hash, HASH);
    }

    #[test]
    fn something_that_is_not_a_magnet_link_is_refused() {
        for bad in [
            "",
            "hello",
            "https://example.com/a.torrent",
            "magnet:?dn=no-hash",
            "magnet:?xt=urn:btmh:1220abcd",
        ] {
            assert!(
                matches!(parse_magnet(bad), Err(TorrentError::Invalid(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn something_that_is_not_a_torrent_file_is_refused() {
        for bad in [&b""[..], b"hello", b"d4:spam4:eggse", b"\xff\xfe"] {
            assert!(matches!(
                parse_torrent_file(bad),
                Err(TorrentError::Invalid(_))
            ));
        }
    }
}
