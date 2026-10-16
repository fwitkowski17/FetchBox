//! Making written data durable.
//!
//! Writes go to the operating system's cache. A killed process loses nothing of it, but a power
//! cut or a kernel crash does, so the offsets that are stored for resuming must only name bytes
//! that have been forced to the disk (SPEC 6.2, write path).

use std::path::Path;

/// Flush the data of `path` to the disk (`fdatasync`). Runs on the blocking pool: it can take
/// seconds on a slow or busy disk, and the download keeps running meanwhile.
pub async fn sync_file(path: &Path) -> std::io::Result<()> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || {
        // Opened for writing so that it also works where syncing needs a writable handle.
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)?
            .sync_data()
    })
    .await
    .map_err(std::io::Error::other)?
}

/// Make a rename in `dir` durable. Best effort: not every file system can sync a directory, and
/// a missed one only means the finished file may show up under its `.part` name after a power cut.
pub async fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    {
        let dir = dir.to_owned();
        let _ = tokio::task::spawn_blocking(move || std::fs::File::open(dir)?.sync_all()).await;
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn syncs_an_existing_file_and_reports_a_missing_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.part");
        std::fs::write(&file, b"data").expect("write");
        sync_file(&file).await.expect("sync");
        let missing = sync_file(&dir.path().join("none.part")).await;
        assert_eq!(
            missing.expect_err("missing").kind(),
            std::io::ErrorKind::NotFound
        );
        sync_dir(dir.path()).await;
    }
}
