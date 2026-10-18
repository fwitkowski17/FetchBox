//! Free space on the download file system.
//!
//! The engine pre-sizes files with `set_len`, which creates sparse files on every file system
//! we care about (ext4, xfs, btrfs, apfs, tmpfs): the space is only taken when bytes are
//! written. So `available` already reflects what has been downloaded, and what a download
//! still needs is its size minus what it has written.

use std::io;
use std::path::{Path, PathBuf};

use fetchbox_core::{disks, DiskSpace};

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

/// A folder of the download directory that is a disk of its own (SPEC 6.14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedDisk {
    /// The folder name.
    pub name: String,
    pub path: PathBuf,
    /// Device id of the file system.
    pub dev: u64,
}

/// Is this folder of `root` a disk? It is when it is another file system than `root` (a mount), or
/// a symbolic link to a folder: a link is made on purpose, so it names a place to save to even when
/// the target is on the same file system. A link to a file or to nothing is not.
fn is_disk(root_dev: u64, path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?; // follows a link
    if !meta.is_dir() {
        return None;
    }
    let dev = device(path).ok()?;
    let linked = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    (dev != root_dev || linked).then_some(dev)
}

/// The folders of `root` that are disks, by name: file systems of their own (mounted there) or
/// symbolic links to folders. Hidden folders, files, and the folders that mean something else inside
/// the download directory (`fetchbox_core::disks::is_reserved`) are not disks. A link is followed,
/// which is what writing through it does, so the space shown is the target's.
pub fn find_disks(root: &Path) -> io::Result<Vec<MountedDisk>> {
    let root_dev = device(root)?;
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let Ok(entry) = entry else { continue };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue; // a name that is not text cannot be stored or shown
        };
        if !disks::is_valid_name(&name) || disks::is_reserved(&name) {
            continue;
        }
        let path = entry.path();
        if let Some(dev) = is_disk(root_dev, &path) {
            found.push(MountedDisk { name, path, dev });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// Why a disk cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiskProblem {
    #[error("{0:?} is not the name of a folder in the download directory")]
    BadName(String),
    #[error(
        "the disk {0:?} is not available: it is not a separate file system in the download directory (is it mounted?)"
    )]
    NotMounted(String),
}

/// The directory of a disk, if it can be used now: the empty name is `root` itself; any other has
/// to be a folder of `root` that is a disk (`find_disks`: a file system of its own, or a link to a
/// folder). The check is what keeps a disk that is not mounted (an empty folder on the system disk)
/// from being filled in its place.
pub fn disk_path(root: &Path, name: &str) -> Result<PathBuf, DiskProblem> {
    if !disks::is_valid_name(name) || (!name.is_empty() && disks::is_reserved(name)) {
        return Err(DiskProblem::BadName(name.to_owned()));
    }
    if name.is_empty() {
        return Ok(root.to_owned());
    }
    let path = root.join(name);
    let usable = device(root).is_ok_and(|root_dev| is_disk(root_dev, &path).is_some());
    if usable {
        Ok(path)
    } else {
        Err(DiskProblem::NotMounted(name.to_owned()))
    }
}

/// Empty folders of `root` that are not disks: what a mount point that did not mount looks like (a
/// folder that was created for it, on the system disk). Folders with something in them are the
/// user's own and are not mentioned; hidden and reserved names are skipped like for disks.
pub fn find_unmounted(root: &Path) -> io::Result<Vec<String>> {
    let root_dev = device(root)?;
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let Ok(entry) = entry else { continue };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !disks::is_valid_name(&name) || disks::is_reserved(&name) {
            continue;
        }
        let path = entry.path();
        let folder = std::fs::metadata(&path).is_ok_and(|m| m.is_dir());
        let empty = std::fs::read_dir(&path).is_ok_and(|mut d| d.next().is_none());
        if folder && empty && is_disk(root_dev, &path).is_none() {
            found.push(name);
        }
    }
    found.sort();
    Ok(found)
}

/// What the program can tell about where it runs, to doubt a disk that may not be what the user
/// meant (SPEC 6.14). Only containers are looked at: on a host a folder is where the user put it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Environment {
    /// Docker or Podman (`/.dockerenv`, `/run/.containerenv`).
    pub in_container: bool,
    /// Device of `/`: in a container its own (overlay) file system.
    pub root_dev: Option<u64>,
    /// Device of `/etc/hostname`, which Docker bind-mounts from the host's data directory for the
    /// container (usually on the host's system disk). A disk on the same device is likely just a
    /// folder of that disk.
    pub host_dev: Option<u64>,
}

impl Environment {
    pub fn detect() -> Self {
        let in_container =
            Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists();
        Self {
            in_container,
            root_dev: device(Path::new("/")).ok(),
            host_dev: in_container
                .then(|| device(Path::new("/etc/hostname")).ok())
                .flatten(),
        }
    }
}

/// A reason to doubt the disk on device `dev`; `is_main` is the download directory itself. The
/// download directory on the container's own file system means nothing was mounted for it. A named
/// disk on the same device as the container's data on the host means the disk may be just a folder
/// of the host's system disk (the host path was not a mounted disk when the container started). The
/// download directory on the host's system disk is what a plain `./downloads` volume is, so it is
/// not doubted. Outside a container nothing is.
pub fn warning_for(env: &Environment, is_main: bool, dev: u64) -> Option<disks::DiskWarning> {
    if !env.in_container {
        return None;
    }
    if is_main {
        (env.root_dev == Some(dev)).then_some(disks::DiskWarning::InsideContainer)
    } else {
        (env.host_dev == Some(dev)).then_some(disks::DiskWarning::HostSystemDisk)
    }
}

/// Does a download needing `needed` more bytes fit, once what the running downloads still have to
/// write (`others`) is set aside? Unknown sizes are the caller's to skip: nothing can be said
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

    /// A directory on another file system than `base`, as far as the machine has one to offer:
    /// `/dev/shm` is a tmpfs on Linux, so a link to a folder there stands in for a mounted disk.
    #[cfg(unix)]
    fn other_filesystem(base: &Path) -> Option<tempfile::TempDir> {
        let shm = Path::new("/dev/shm");
        if !shm.is_dir() || device(shm).ok()? == device(base).ok()? {
            return None;
        }
        tempfile::tempdir_in(shm).ok()
    }

    #[cfg(unix)]
    #[test]
    fn plain_folders_are_not_disks_but_a_link_to_another_file_system_is() {
        let root = tempfile::tempdir().expect("tmp");
        std::fs::create_dir(root.path().join("plain")).expect("dir");
        std::fs::write(root.path().join("file.bin"), b"x").expect("file");
        assert_eq!(find_disks(root.path()).expect("scan"), vec![]);

        let Some(other) = other_filesystem(root.path()) else {
            return; // nothing to stand in for a second disk on this machine
        };
        let link = |name: &str| {
            std::os::unix::fs::symlink(other.path(), root.path().join(name)).expect("link");
        };
        link("ssd1");
        link("hdd10tb");
        // Folders that mean something else, hidden ones and dead links are never disks.
        link("video");
        link("torrents");
        link(".hidden");
        std::os::unix::fs::symlink("/nonexistent/target", root.path().join("dead")).expect("link");

        let found = find_disks(root.path()).expect("scan");
        let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["hdd10tb", "ssd1"], "by name");
        assert!(found
            .iter()
            .all(|d| d.dev == device(other.path()).expect("dev")));
    }

    #[cfg(unix)]
    #[test]
    fn a_disk_is_usable_only_while_it_is_a_file_system_of_its_own() {
        let root = tempfile::tempdir().expect("tmp");
        assert_eq!(disk_path(root.path(), ""), Ok(root.path().to_owned()));
        // An empty folder on the same file system is a disk that is not mounted.
        std::fs::create_dir(root.path().join("ssd1")).expect("dir");
        assert_eq!(
            disk_path(root.path(), "ssd1"),
            Err(DiskProblem::NotMounted("ssd1".into()))
        );
        assert_eq!(
            disk_path(root.path(), "missing"),
            Err(DiskProblem::NotMounted("missing".into()))
        );
        for bad in ["../x", "a/b", ".hidden", "..", "video", "torrents"] {
            assert_eq!(
                disk_path(root.path(), bad),
                Err(DiskProblem::BadName(bad.into())),
                "{bad}"
            );
        }
        if let Some(other) = other_filesystem(root.path()) {
            std::os::unix::fs::symlink(other.path(), root.path().join("hdd")).expect("link");
            assert_eq!(disk_path(root.path(), "hdd"), Ok(root.path().join("hdd")));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_folder_is_a_disk_even_on_the_same_file_system() {
        let root = tempfile::tempdir().expect("tmp");
        let elsewhere = tempfile::tempdir().expect("tmp");
        let file = root.path().join("some.bin");
        std::fs::write(&file, b"x").expect("file");
        std::os::unix::fs::symlink(elsewhere.path(), root.path().join("fast")).expect("link");
        std::os::unix::fs::symlink(&file, root.path().join("to-a-file")).expect("link");
        std::os::unix::fs::symlink("/nonexistent/target", root.path().join("dead")).expect("link");
        std::fs::create_dir(root.path().join("plain")).expect("dir");

        let names: Vec<String> = find_disks(root.path())
            .expect("scan")
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert_eq!(
            names,
            ["fast"],
            "only the link to a folder; not a file, a dead link or a plain folder"
        );
        assert_eq!(disk_path(root.path(), "fast"), Ok(root.path().join("fast")));
        for not in ["to-a-file", "dead", "plain"] {
            assert_eq!(
                disk_path(root.path(), not),
                Err(DiskProblem::NotMounted(not.into())),
                "{not}"
            );
        }
        // Writing through the link lands in its target.
        std::fs::write(root.path().join("fast/a.bin"), b"y").expect("write through the link");
        assert!(elsewhere.path().join("a.bin").exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_empty_folder_that_is_not_a_disk_is_what_a_failed_mount_looks_like() {
        let root = tempfile::tempdir().expect("tmp");
        for dir in ["hdd10tb", "ssd1", "music", "torrents", ".cache", "mine"] {
            std::fs::create_dir(root.path().join(dir)).expect("dir");
        }
        std::fs::write(root.path().join("mine/photo.jpg"), b"x").expect("file");
        std::fs::write(root.path().join("file.bin"), b"x").expect("file");
        let linked = tempfile::tempdir().expect("tmp");
        std::os::unix::fs::symlink(linked.path(), root.path().join("linked")).expect("link");
        assert_eq!(
            find_unmounted(root.path()).expect("scan"),
            ["hdd10tb", "ssd1"],
            "empty, plain, not reserved or hidden; not the user's own folder with files, not a link"
        );
    }

    #[test]
    fn a_disk_is_doubted_only_in_a_container_and_only_where_the_host_shows_through() {
        const OVERLAY: u64 = 10;
        const HOST_DISK: u64 = 20;
        const OTHER: u64 = 30;
        let docker = Environment {
            in_container: true,
            root_dev: Some(OVERLAY),
            host_dev: Some(HOST_DISK),
        };
        // The download directory on the container's own file system: nothing was mounted there.
        assert_eq!(
            warning_for(&docker, true, OVERLAY),
            Some(disks::DiskWarning::InsideContainer)
        );
        // On the host's system disk is what a plain `./downloads` volume is: not doubted.
        assert_eq!(warning_for(&docker, true, HOST_DISK), None);
        assert_eq!(warning_for(&docker, true, OTHER), None);
        // A named disk that is on the host's system disk was probably not mounted on the host.
        assert_eq!(
            warning_for(&docker, false, HOST_DISK),
            Some(disks::DiskWarning::HostSystemDisk)
        );
        assert_eq!(warning_for(&docker, false, OTHER), None);
        assert_eq!(warning_for(&docker, false, OVERLAY), None);
        // On a host, or when the devices cannot be read, nothing is said.
        let host = Environment {
            in_container: false,
            root_dev: Some(OVERLAY),
            host_dev: None,
        };
        assert_eq!(warning_for(&host, true, OVERLAY), None);
        assert_eq!(warning_for(&Environment::default(), false, HOST_DISK), None);
        let unknown = Environment {
            in_container: true,
            ..Environment::default()
        };
        assert_eq!(warning_for(&unknown, true, OVERLAY), None);
        assert_eq!(warning_for(&unknown, false, HOST_DISK), None);
    }
}
