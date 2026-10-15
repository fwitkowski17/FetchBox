//! Free space on the download file system.
//!
//! The engine pre-sizes files with `set_len`, which creates sparse files on every file system
//! we care about (ext4, xfs, btrfs, apfs, tmpfs): the space is only taken when bytes are
//! written. So `available` already reflects what has been downloaded, and what a download
//! still needs is its size minus what it has written.

use std::io;
use std::path::Path;

use dlm_core::DiskSpace;

use crate::error::EngineError;

/// Total and available bytes of the file system holding `path` (which must exist).
/// `pending` is left at 0 for the caller to fill in.
#[cfg(unix)]
pub fn space(path: &Path) -> io::Result<DiskSpace> {
    let s = rustix::fs::statvfs(path)?;
    // Block counts are in units of the fragment size.
    let unit = s.f_frsize;
    let used = s.f_blocks.saturating_sub(s.f_bfree).saturating_mul(unit);
    let available = s.f_bavail.saturating_mul(unit);
    Ok(DiskSpace {
        // Blocks reserved for root are neither used nor available to us.
        total: used.saturating_add(available),
        available,
        pending: 0,
    })
}

#[cfg(not(unix))]
pub fn space(_path: &Path) -> io::Result<DiskSpace> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "free disk space is not available on this platform",
    ))
}

/// Does a download needing `needed` more bytes fit, given what other running downloads still
/// have to write? Unknown sizes are the caller's to skip: nothing can be said about them.
pub fn ensure_fits(needed: u64, others: u64, available: u64) -> Result<(), EngineError> {
    let total = needed.saturating_add(others);
    if total <= available {
        Ok(())
    } else {
        Err(EngineError::NoSpace {
            needed: total,
            available,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_fit_is_allowed_and_one_byte_more_is_not() {
        assert!(ensure_fits(100, 0, 100).is_ok());
        assert!(matches!(
            ensure_fits(101, 0, 100),
            Err(EngineError::NoSpace {
                needed: 101,
                available: 100
            })
        ));
    }

    #[test]
    fn other_downloads_count_against_the_free_space() {
        assert!(ensure_fits(60, 40, 100).is_ok());
        assert!(matches!(
            ensure_fits(60, 41, 100),
            Err(EngineError::NoSpace { needed: 101, .. })
        ));
    }

    #[test]
    fn huge_sizes_do_not_overflow() {
        assert!(ensure_fits(u64::MAX, u64::MAX, 1).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn reports_a_sane_figure_for_a_real_directory() {
        let dir = tempfile::tempdir().expect("tmp");
        let s = space(dir.path()).expect("statvfs");
        assert!(s.total > 0, "{s:?}");
        assert!(s.available <= s.total, "{s:?}");
        assert!(s.used() + s.available == s.total);
        assert_eq!(s.pending, 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_directory_is_an_error() {
        let dir = tempfile::tempdir().expect("tmp");
        assert!(space(&dir.path().join("nope")).is_err());
    }
}
