//! Checksums a download can be verified against (SPEC 6.2.2).
//!
//! Only the text side lives here: which algorithms there are, and how what a user pastes becomes a
//! normalised value. Computing a checksum is the engine's job (`fetchbox-http-engine`).

use serde::{Deserialize, Serialize};

/// A hash algorithm offered for verification. The common ones that sites publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ChecksumAlgo {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl ChecksumAlgo {
    /// All algorithms, shortest digest first.
    pub const ALL: [ChecksumAlgo; 4] = [
        ChecksumAlgo::Md5,
        ChecksumAlgo::Sha1,
        ChecksumAlgo::Sha256,
        ChecksumAlgo::Sha512,
    ];

    /// Stable lowercase name, as stored in SQLite and used in the API.
    pub fn as_str(self) -> &'static str {
        match self {
            ChecksumAlgo::Md5 => "md5",
            ChecksumAlgo::Sha1 => "sha1",
            ChecksumAlgo::Sha256 => "sha256",
            ChecksumAlgo::Sha512 => "sha512",
        }
    }

    /// Parse the stored name.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == s)
    }

    /// Length of the digest written in hexadecimal.
    pub fn hex_len(self) -> usize {
        match self {
            ChecksumAlgo::Md5 => 32,
            ChecksumAlgo::Sha1 => 40,
            ChecksumAlgo::Sha256 => 64,
            ChecksumAlgo::Sha512 => 128,
        }
    }
}

/// What the user entered when adding a download: the algorithm and the text they pasted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ChecksumInput {
    pub algo: ChecksumAlgo,
    /// The checksum as pasted: `d41d8cd9...`, `SHA256: d41d...`, or a line of a checksum file
    /// (`d41d...  name.iso`). Normalised by the server.
    pub value: String,
}

/// A checksum a download is verified against, and what came out of the verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ChecksumState {
    pub algo: ChecksumAlgo,
    /// The expected digest: lowercase hexadecimal.
    pub expected: String,
    /// The digest of the downloaded file when it did not match.
    #[serde(default)]
    pub actual: Option<String>,
}

/// The expected digest in its normal form (lowercase hexadecimal of the right length), from what
/// was pasted. `Err` says what is wrong, in words for the user.
///
/// Forgiving about what people copy: surrounding spaces, an `sha256:` or `0x` prefix, upper case,
/// separators (`:` or spaces inside the digest) and a whole line of a checksum file, in which the
/// digest is the first word and the file name follows.
pub fn normalize(algo: ChecksumAlgo, pasted: &str) -> Result<String, String> {
    let line = pasted.trim().lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return Err("the checksum is empty".to_owned());
    }
    let mut text = line.to_ascii_lowercase();
    // `sha256:abc...` or `sha256=abc...` or `sha256sum: abc...`
    for prefix in [algo.as_str(), "checksum"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            let rest = rest.trim_start_matches("sum");
            if let Some(rest) = rest.trim_start().strip_prefix([':', '=']) {
                text = rest.trim().to_owned();
                break;
            }
        }
    }
    if let Some(rest) = text.strip_prefix("0x") {
        text = rest.to_owned();
    }
    // A checksum file line: `<digest>  <file name>` or `<digest> *<file name>`.
    let first = text.split_whitespace().next().unwrap_or("");
    let digest = if first.len() == algo.hex_len() {
        first.to_owned()
    } else {
        // Digests written with separators, in groups: `d4:1d:8c...`, `d41d8cd9 8f00b204 ...`.
        text.chars()
            .filter(|c| !matches!(c, ':' | ' ' | '-'))
            .collect()
    };
    if !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("a checksum has only the digits 0-9 and the letters a-f".to_owned());
    }
    if digest.len() != algo.hex_len() {
        return Err(format!(
            "a {} checksum has {} characters, this one has {}",
            algo.as_str().to_ascii_uppercase(),
            algo.hex_len(),
            digest.len()
        ));
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD5: &str = "d41d8cd98f00b204e9800998ecf8427e";
    const SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn names_round_trip_and_lengths_are_distinct() {
        for a in ChecksumAlgo::ALL {
            assert_eq!(ChecksumAlgo::parse(a.as_str()), Some(a));
        }
        assert_eq!(ChecksumAlgo::parse("crc32"), None);
        let mut lens: Vec<_> = ChecksumAlgo::ALL.iter().map(|a| a.hex_len()).collect();
        lens.dedup();
        assert_eq!(lens.len(), 4, "the length alone tells the algorithm");
    }

    #[test]
    fn a_plain_digest_is_lowercased() {
        assert_eq!(normalize(ChecksumAlgo::Md5, MD5), Ok(MD5.to_owned()));
        assert_eq!(
            normalize(ChecksumAlgo::Md5, &MD5.to_ascii_uppercase()),
            Ok(MD5.to_owned())
        );
        assert_eq!(
            normalize(ChecksumAlgo::Sha256, &format!("  {SHA256}\n")),
            Ok(SHA256.to_owned())
        );
    }

    #[test]
    fn what_people_copy_is_understood() {
        let sha = ChecksumAlgo::Sha256;
        for pasted in [
            format!("sha256:{SHA256}"),
            format!("SHA256: {SHA256}"),
            format!("sha256={SHA256}"),
            format!("0x{SHA256}"),
            format!("{SHA256}  ubuntu-24.04.iso"),
            format!("{SHA256} *ubuntu-24.04.iso"),
            format!("{SHA256}\nsecond line is ignored"),
        ] {
            assert_eq!(normalize(sha, &pasted), Ok(SHA256.to_owned()), "{pasted}");
        }
        let grouped: String = MD5
            .as_bytes()
            .chunks(8)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(normalize(ChecksumAlgo::Md5, &grouped), Ok(MD5.to_owned()));
        let colons: String = MD5
            .as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(":");
        assert_eq!(normalize(ChecksumAlgo::Md5, &colons), Ok(MD5.to_owned()));
    }

    #[test]
    fn wrong_input_is_explained() {
        assert!(normalize(ChecksumAlgo::Md5, "").is_err());
        assert!(normalize(ChecksumAlgo::Md5, "   \n").is_err());
        let short = normalize(ChecksumAlgo::Sha256, MD5).expect_err("md5 is not sha256");
        assert!(short.contains("64") && short.contains("32"), "{short}");
        let bad = normalize(ChecksumAlgo::Md5, &"z".repeat(32)).expect_err("not hex");
        assert!(bad.contains("0-9"), "{bad}");
    }
}
