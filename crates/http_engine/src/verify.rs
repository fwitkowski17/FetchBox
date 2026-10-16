//! Checksum of a finished file (SPEC 6.2.2).

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use fetchbox_core::ChecksumAlgo;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

/// How much is read at a time. Large enough for a hard disk to read sequentially at full speed.
const CHUNK: usize = 1 << 20;

/// The digest of the file at `path`, in lowercase hexadecimal.
///
/// Runs on the blocking pool. `hashed` counts the bytes read so far (for a progress bar); once
/// `cancel` is set the work stops at the next chunk and the answer is `Ok(None)`.
pub async fn hash_file(
    path: &Path,
    algo: ChecksumAlgo,
    hashed: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
) -> std::io::Result<Option<String>> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || hash_blocking(&path, algo, &hashed, &cancel))
        .await
        .map_err(std::io::Error::other)?
}

fn hash_blocking(
    path: &Path,
    algo: ChecksumAlgo,
    hashed: &AtomicU64,
    cancel: &AtomicBool,
) -> std::io::Result<Option<String>> {
    match algo {
        ChecksumAlgo::Md5 => run::<Md5>(path, hashed, cancel),
        ChecksumAlgo::Sha1 => run::<Sha1>(path, hashed, cancel),
        ChecksumAlgo::Sha256 => run::<Sha256>(path, hashed, cancel),
        ChecksumAlgo::Sha512 => run::<Sha512>(path, hashed, cancel),
    }
}

fn run<D: Digest>(
    path: &Path,
    hashed: &AtomicU64,
    cancel: &AtomicBool,
) -> std::io::Result<Option<String>> {
    let mut file = File::open(path)?;
    let mut digest = D::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
        hashed.fetch_add(n as u64, Ordering::Relaxed);
    }
    let out = digest.finalize();
    Ok(Some(out.iter().map(|b| format!("{b:02x}")).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn digest_of(content: &[u8], algo: ChecksumAlgo) -> String {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("f");
        std::fs::write(&path, content).expect("write");
        hash_file(&path, algo, Arc::default(), Arc::default())
            .await
            .expect("hash")
            .expect("not cancelled")
    }

    #[tokio::test]
    async fn known_digests() {
        // The standard test vectors: "" for md5, "abc" for the others.
        assert_eq!(
            digest_of(b"", ChecksumAlgo::Md5).await,
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(
            digest_of(b"abc", ChecksumAlgo::Md5).await,
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            digest_of(b"abc", ChecksumAlgo::Sha1).await,
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            digest_of(b"abc", ChecksumAlgo::Sha256).await,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            digest_of(b"abc", ChecksumAlgo::Sha512).await,
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[tokio::test]
    async fn a_file_larger_than_a_chunk_is_hashed_whole_and_counted() {
        // One million 'a' (a standard SHA-256 vector), spanning several reads.
        let content = vec![b'a'; 1_000_000];
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("f");
        std::fs::write(&path, &content).expect("write");
        let hashed = Arc::new(AtomicU64::new(0));
        let digest = hash_file(&path, ChecksumAlgo::Sha256, hashed.clone(), Arc::default())
            .await
            .expect("hash");
        assert_eq!(
            digest.as_deref(),
            Some("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0")
        );
        assert_eq!(hashed.load(Ordering::Relaxed), 1_000_000);
    }

    #[tokio::test]
    async fn cancelling_stops_the_work_and_a_missing_file_is_an_error() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("f");
        std::fs::write(&path, vec![0u8; 3 * CHUNK]).expect("write");
        let cancel = Arc::new(AtomicBool::new(true));
        let cancelled = hash_file(&path, ChecksumAlgo::Md5, Arc::default(), cancel)
            .await
            .expect("no error");
        assert_eq!(cancelled, None);
        let missing = hash_file(
            &dir.path().join("none"),
            ChecksumAlgo::Md5,
            Arc::default(),
            Arc::default(),
        )
        .await
        .expect_err("missing");
        assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
    }
}
