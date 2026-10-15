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

/// A file system: its identity and its space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume {
    /// Device id; equal for paths on the same file system. Reservations are kept per device,
    /// because a category subdirectory can be a mount point on another disk.
    pub dev: u64,
    /// `reserved` is left at 0 for the caller to fill in.
    pub space: DiskSpace,
}

/// The file system that holds `path`. The path need not exist yet (a category subdirectory is
/// created when its first download starts): the nearest existing parent decides.
pub fn volume(path: &Path) -> io::Result<Volume> {
    let mut existing = path;
    while !existing.exists() {
        existing = existing.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no existing parent directory")
        })?;
    }
    Ok(Volume {
        dev: device(existing)?,
        space: space(existing)?,
    })
}

#[cfg(unix)]
fn device(path: &Path) -> io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::metadata(path)?.dev())
}

#[cfg(not(unix))]
fn device(_path: &Path) -> io::Result<u64> {
    Ok(0)
}

/// Total and available bytes of the file system holding `path` (which must exist).
#[cfg(unix)]
fn space(path: &Path) -> io::Result<DiskSpace> {
    let s = rustix::fs::statvfs(path)?;
    // Block counts are in units of the fragment size.
    let unit = s.f_frsize;
    let used = s.f_blocks.saturating_sub(s.f_bfree).saturating_mul(unit);
    let available = s.f_bavail.saturating_mul(unit);
    Ok(DiskSpace {
        // Blocks reserved for root are neither used nor available to us.
        total: used.saturating_add(available),
        available,
        reserved: 0,
    })
}

#[cfg(not(unix))]
fn space(_path: &Path) -> io::Result<DiskSpace> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "free disk space is not available on this platform",
    ))
}

/// Does a download needing `needed` more bytes fit, once the space promised to other unfinished
/// downloads (`others`) is set aside? Unknown sizes are the caller's to skip: nothing can be said
/// about them. The error carries the raw free space and the share reserved by the others, so the
/// user can see why a file that is smaller than the free space still does not fit.
pub fn ensure_fits(needed: u64, others: u64, free: u64) -> Result<(), EngineError> {
    if needed <= free.saturating_sub(others) {
        Ok(())
    } else {
        Err(EngineError::NoSpace {
            needed,
            free,
            reserved: others,
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
                free: 100,
                reserved: 0
            })
        ));
    }

    #[test]
    fn space_promised_to_other_downloads_is_not_free() {
        assert!(ensure_fits(60, 40, 100).is_ok());
        // The error says both the raw free space and how much of it the others hold.
        assert!(matches!(
            ensure_fits(60, 41, 100),
            Err(EngineError::NoSpace {
                needed: 60,
                free: 100,
                reserved: 41
            })
        ));
    }

    #[test]
    fn over_committed_disks_have_nothing_free_and_sizes_do_not_overflow() {
        assert!(matches!(
            ensure_fits(1, 500, 100),
            Err(EngineError::NoSpace {
                free: 100,
                reserved: 500,
                ..
            })
        ));
        assert!(ensure_fits(u64::MAX, u64::MAX, 1).is_err());
        assert!(ensure_fits(0, u64::MAX, 0).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn reports_a_sane_figure_for_a_real_directory() {
        let dir = tempfile::tempdir().expect("tmp");
        let v = volume(dir.path()).expect("statvfs");
        assert!(v.space.total > 0, "{v:?}");
        assert!(v.space.available <= v.space.total, "{v:?}");
        assert!(v.space.used() + v.space.available == v.space.total);
        assert_eq!(v.space.reserved, 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_that_does_not_exist_yet_is_judged_by_its_parent() {
        let dir = tempfile::tempdir().expect("tmp");
        let here = volume(dir.path()).expect("volume");
        let later = volume(&dir.path().join("video").join("deeper")).expect("volume");
        assert_eq!(here.dev, later.dev);
    }

    #[cfg(unix)]
    #[test]
    fn different_file_systems_have_different_devices() {
        // `/proc` is its own file system on Linux; skip where it does not exist.
        let proc = Path::new("/proc");
        if !proc.exists() {
            return;
        }
        let root = volume(Path::new("/")).expect("root");
        let p = volume(proc).expect("proc");
        assert_ne!(root.dev, p.dev);
    }
}
