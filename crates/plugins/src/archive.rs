//! Unpacking archives that come from outside: a plugin's ZIP and the archives of its dependencies.
//!
//! The contents are untrusted, so extraction refuses what could write outside the destination or
//! plant something unexpected in it: absolute paths, `..`, drive letters and backslashes, links and
//! special files, encrypted entries; and it enforces limits (entries, total size, expansion ratio,
//! depth, name length) on the bytes really read, not on what the archive claims.
//! Everything is synchronous: call it from `spawn_blocking`.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use fetchbox_core::plugin::ArchiveKind;

use crate::{Error, Result};

/// What an extraction may use up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Files and folders in the archive.
    pub max_entries: usize,
    /// Bytes of all files together, after decompression.
    pub max_total: u64,
    /// Decompressed bytes per compressed byte, for an entry that is large enough to matter.
    pub max_ratio: u64,
    /// Path components below the destination.
    pub max_depth: usize,
    /// Bytes of one path component.
    pub max_name: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_entries: 2000,
            max_total: 512 * 1024 * 1024,
            max_ratio: 200,
            max_depth: 16,
            max_name: 255,
        }
    }
}

/// What was unpacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Extracted {
    pub files: usize,
    pub bytes: u64,
}

/// An entry below this size is not judged by its expansion ratio (a few zero bytes compress well).
const RATIO_FROM: u64 = 1024 * 1024;

fn bad(msg: impl Into<String>) -> Error {
    Error::Archive(msg.into())
}

/// Where the entry `name` goes below `root`, after dropping `strip` leading components. `None` means
/// the entry is only the stripped prefix itself (nothing to create). Refuses every name that could
/// leave `root`.
pub fn entry_path(root: &Path, name: &str, strip: u32, limits: &Limits) -> Result<Option<PathBuf>> {
    if name.is_empty() {
        return Err(bad("an entry has no name"));
    }
    if name.starts_with('/') {
        return Err(bad(format!("\"{name}\" is an absolute path")));
    }
    if name.contains('\\') {
        return Err(bad(format!("\"{name}\" has a backslash")));
    }
    if name.chars().any(char::is_control) {
        return Err(bad("an entry name has a control character"));
    }
    let mut parts = Vec::new();
    for (i, c) in name.split('/').enumerate() {
        match c {
            "" | "." => continue,
            ".." => return Err(bad(format!("\"{name}\" climbs out of the folder"))),
            _ => {}
        }
        if i == 0 && c.contains(':') {
            return Err(bad(format!("\"{name}\" starts with a drive")));
        }
        if c.len() > limits.max_name {
            return Err(bad(format!("a name in \"{name}\" is too long")));
        }
        parts.push(c);
    }
    let strip = strip as usize;
    if parts.len() <= strip {
        return Ok(None);
    }
    let parts = &parts[strip..];
    if parts.len() > limits.max_depth {
        return Err(bad(format!("\"{name}\" is nested too deeply")));
    }
    let mut path = root.to_path_buf();
    path.extend(parts);
    // Each component is a plain name, so this holds; checked anyway as the last line of defence.
    if !path.starts_with(root) {
        return Err(bad(format!("\"{name}\" leaves the folder")));
    }
    Ok(Some(path))
}

/// Tracks the limits while an archive is read.
struct Budget {
    limits: Limits,
    entries: usize,
    bytes: u64,
}

impl Budget {
    fn new(limits: Limits) -> Self {
        Budget {
            limits,
            entries: 0,
            bytes: 0,
        }
    }

    fn entry(&mut self) -> Result<()> {
        self.entries += 1;
        if self.entries > self.limits.max_entries {
            return Err(bad(format!(
                "the archive has more than {} entries",
                self.limits.max_entries
            )));
        }
        Ok(())
    }

    /// Copy at most what is left of the byte budget into a new file. `packed` is the size of the
    /// entry inside the archive, for the ratio check.
    fn write_file(
        &mut self,
        src: &mut dyn Read,
        path: &Path,
        executable: bool,
        packed: Option<u64>,
    ) -> Result<u64> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let left = self.limits.max_total.saturating_sub(self.bytes);
        // `create_new`: an archive that names the same file twice is refused rather than merged.
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| match e.kind() {
                io::ErrorKind::AlreadyExists => bad(format!(
                    "\"{}\" appears twice in the archive",
                    path.file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                )),
                _ => Error::Io(e),
            })?;
        let written = io::copy(&mut src.take(left.saturating_add(1)), &mut out)?;
        if written > left {
            return Err(bad(format!(
                "the archive unpacks to more than {} MiB",
                self.limits.max_total / (1024 * 1024)
            )));
        }
        if let Some(packed) = packed {
            if written >= RATIO_FROM && written / packed.max(1) > self.limits.max_ratio {
                return Err(bad("an entry expands far more than a real file would"));
            }
        }
        out.flush()?;
        drop(out);
        set_mode(path, executable)?;
        self.bytes += written;
        Ok(written)
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _executable: bool) -> Result<()> {
    Ok(())
}

/// Unpack a ZIP file into `dest` (created if needed).
pub fn extract_zip(zip_path: &Path, dest: &Path, strip: u32, limits: &Limits) -> Result<Extracted> {
    let mut archive = zip::ZipArchive::new(File::open(zip_path)?)
        .map_err(|e| bad(format!("not a usable ZIP file: {e}")))?;
    fs::create_dir_all(dest)?;
    let mut budget = Budget::new(*limits);
    let mut files = 0;
    for i in 0..archive.len() {
        budget.entry()?;
        let mut entry = archive
            .by_index(i)
            .map_err(|e| bad(format!("a ZIP entry cannot be read: {e}")))?;
        let name = entry
            .name()
            .map_err(|e| bad(format!("a ZIP entry has an unusable name: {e}")))?
            .into_owned();
        if entry.encrypted() {
            return Err(bad(format!("\"{name}\" is encrypted")));
        }
        let mode = entry.unix_mode();
        if let Some(kind) = mode.map(|m| m & 0o170_000).filter(|k| *k != 0) {
            // Only regular files and folders: a link could point anywhere, a device is not a file.
            if kind != 0o100_000 && kind != 0o040_000 {
                return Err(bad(format!("\"{name}\" is a link or a special file")));
            }
        }
        let Some(path) = entry_path(dest, &name, strip, limits)? else {
            continue;
        };
        if entry.is_dir() {
            fs::create_dir_all(&path)?;
            continue;
        }
        let executable = mode.is_some_and(|m| m & 0o111 != 0);
        let packed = entry.compressed_size();
        budget.write_file(&mut entry, &path, executable, Some(packed))?;
        files += 1;
    }
    Ok(Extracted {
        files,
        bytes: budget.bytes,
    })
}

/// Unpack a tar stream (already decompressed) into `dest`.
pub fn extract_tar(src: impl Read, dest: &Path, strip: u32, limits: &Limits) -> Result<Extracted> {
    fs::create_dir_all(dest)?;
    let mut budget = Budget::new(*limits);
    let mut files = 0;
    // Headers and padding are not counted as bytes of files; this keeps a stream that is mostly
    // padding from running on for ever.
    let src = src.take(limits.max_total.saturating_add(8 * 1024 * 1024));
    let mut archive = tar::Archive::new(src);
    let entries = archive
        .entries()
        .map_err(|e| bad(format!("not a usable tar archive: {e}")))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| bad(format!("a tar entry cannot be read: {e}")))?;
        let kind = entry.header().entry_type();
        if matches!(
            kind,
            tar::EntryType::XGlobalHeader | tar::EntryType::XHeader
        ) {
            continue;
        }
        budget.entry()?;
        let raw = entry.path_bytes().into_owned();
        let name = String::from_utf8(raw).map_err(|_| bad("a tar entry name is not UTF-8"))?;
        let Some(path) = entry_path(dest, &name, strip, limits)? else {
            continue;
        };
        if kind.is_dir() {
            fs::create_dir_all(&path)?;
        } else if kind.is_file() {
            let executable = entry.header().mode().is_ok_and(|m| m & 0o111 != 0);
            budget.write_file(&mut entry, &path, executable, None)?;
            files += 1;
        } else {
            return Err(bad(format!("\"{name}\" is a link or a special file")));
        }
    }
    Ok(Extracted {
        files,
        bytes: budget.bytes,
    })
}

/// Write one decompressed (or plain) file as `dest/name`.
fn extract_single(
    src: &mut dyn Read,
    dest: &Path,
    name: &str,
    packed: Option<u64>,
    limits: &Limits,
) -> Result<Extracted> {
    fs::create_dir_all(dest)?;
    let path = entry_path(dest, name, 0, limits)?.ok_or_else(|| bad("the file has no name"))?;
    let mut budget = Budget::new(*limits);
    budget.entry()?;
    budget.write_file(src, &path, true, packed)?;
    Ok(Extracted {
        files: 1,
        bytes: budget.bytes,
    })
}

/// Unpack a downloaded dependency. `name` is what a `raw` or `gz` file is called inside `dest` (the
/// program itself); archives bring their own names.
pub fn extract(
    kind: ArchiveKind,
    file: &Path,
    dest: &Path,
    name: &str,
    strip: u32,
    limits: &Limits,
) -> Result<Extracted> {
    match kind {
        ArchiveKind::Raw => {
            let size = fs::metadata(file)?.len();
            extract_single(&mut File::open(file)?, dest, name, Some(size), limits)
        }
        ArchiveKind::Gz => {
            let size = fs::metadata(file)?.len();
            let mut gz = flate2::read::GzDecoder::new(File::open(file)?);
            extract_single(&mut gz, dest, name, Some(size), limits)
        }
        ArchiveKind::Zip => extract_zip(file, dest, strip, limits),
        ArchiveKind::TarGz => extract_tar(
            flate2::read::GzDecoder::new(File::open(file)?),
            dest,
            strip,
            limits,
        ),
        ArchiveKind::TarXz => extract_tar(
            lzma_rust2::XzReader::new(File::open(file)?, true),
            dest,
            strip,
            limits,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn limits() -> Limits {
        Limits::default()
    }

    fn root() -> PathBuf {
        PathBuf::from("/dest")
    }

    #[test]
    fn plain_names_land_inside_the_destination() {
        let p = |n: &str| entry_path(&root(), n, 0, &limits()).expect("ok");
        assert_eq!(p("bin/tool"), Some(PathBuf::from("/dest/bin/tool")));
        assert_eq!(p("./a//b/"), Some(PathBuf::from("/dest/a/b")));
        assert_eq!(p("a/./b"), Some(PathBuf::from("/dest/a/b")));
    }

    #[test]
    fn names_that_could_leave_the_destination_are_refused() {
        for bad in [
            "../evil",
            "a/../../evil",
            "a/..",
            "/etc/passwd",
            "C:/Windows/x",
            "C:evil",
            "a\\b",
            "..\\evil",
            "a/b\0c",
            "",
        ] {
            assert!(
                entry_path(&root(), bad, 0, &limits()).is_err(),
                "{bad:?} should be refused"
            );
        }
        // A colon is fine away from the start (a file called `a:b` is a plain name on Unix).
        assert!(entry_path(&root(), "dir/a:b", 0, &limits()).is_ok());
    }

    #[test]
    fn stripping_drops_leading_components() {
        let p = |n: &str, s| entry_path(&root(), n, s, &limits()).expect("ok");
        assert_eq!(
            p("top/bin/ffmpeg", 1),
            Some(PathBuf::from("/dest/bin/ffmpeg"))
        );
        assert_eq!(p("top/", 1), None);
        assert_eq!(p("top", 1), None);
        assert_eq!(p("a/b", 2), None);
        // `..` is refused even inside the part that is stripped.
        assert!(entry_path(&root(), "../x/y", 1, &limits()).is_err());
    }

    #[test]
    fn depth_and_name_length_are_limited() {
        let deep = vec!["d"; 17].join("/");
        assert!(entry_path(&root(), &deep, 0, &limits()).is_err());
        assert!(entry_path(&root(), &deep, 1, &limits()).is_ok());
        let long = "x".repeat(256);
        assert!(entry_path(&root(), &long, 0, &limits()).is_err());
    }

    // ---- ZIP ----

    fn zip_bytes(build: impl FnOnce(&mut zip::ZipWriter<Cursor<Vec<u8>>>)) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        build(&mut w);
        w.finish().expect("finish").into_inner()
    }

    fn opts() -> zip::write::SimpleFileOptions {
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)
    }

    fn write_to(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, bytes).expect("write");
        p
    }

    #[test]
    fn a_zip_is_unpacked_with_its_folders_and_exec_bits() {
        let tmp = tempfile::tempdir().expect("tmp");
        let bytes = zip_bytes(|w| {
            w.add_directory("bin/", opts()).expect("dir");
            w.start_file("bin/tool", opts().unix_permissions(0o755))
                .expect("file");
            w.write_all(b"#!tool").expect("write");
            w.start_file("plugin.json", opts().unix_permissions(0o600))
                .expect("file");
            w.write_all(b"{}").expect("write");
        });
        let zip = write_to(tmp.path(), "p.zip", &bytes);
        let out = tmp.path().join("out");
        let got = extract_zip(&zip, &out, 0, &limits()).expect("extract");
        assert_eq!(got, Extracted { files: 2, bytes: 8 });
        assert_eq!(fs::read(out.join("bin/tool")).expect("read"), b"#!tool");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &str| {
                fs::metadata(out.join(p))
                    .expect("meta")
                    .permissions()
                    .mode()
                    & 0o777
            };
            assert_eq!(mode("bin/tool"), 0o755);
            assert_eq!(
                mode("plugin.json"),
                0o644,
                "never writable or readable beyond the norm"
            );
        }
    }

    #[test]
    fn a_zip_that_climbs_out_writes_nothing_outside() {
        let tmp = tempfile::tempdir().expect("tmp");
        for name in [
            "../evil",
            "a/../../evil",
            "/abs/evil",
            "..\\evil",
            "C:/evil",
        ] {
            let bytes = zip_bytes(|w| {
                w.start_file(name, opts()).expect("file");
                w.write_all(b"x").expect("write");
            });
            let zip = write_to(tmp.path(), "p.zip", &bytes);
            let out = tmp.path().join("out");
            let err = extract_zip(&zip, &out, 0, &limits()).expect_err(name);
            assert!(matches!(err, Error::Archive(_)), "{name}: {err}");
            let _ = fs::remove_dir_all(&out);
        }
        assert!(!tmp.path().join("evil").exists());
        assert!(!Path::new("/abs/evil").exists());
    }

    #[test]
    fn a_zip_with_a_symlink_is_refused() {
        let tmp = tempfile::tempdir().expect("tmp");
        let bytes = zip_bytes(|w| {
            w.add_symlink("link", "/etc/passwd", opts())
                .expect("symlink");
        });
        let zip = write_to(tmp.path(), "p.zip", &bytes);
        let err = extract_zip(&zip, &tmp.path().join("out"), 0, &limits()).expect_err("refused");
        assert!(err.to_string().contains("link"), "{err}");
    }

    #[test]
    fn the_limits_hold_on_the_bytes_really_read() {
        let tmp = tempfile::tempdir().expect("tmp");
        let bytes = zip_bytes(|w| {
            for i in 0..5 {
                w.start_file(format!("f{i}"), opts()).expect("file");
                w.write_all(&[7u8; 600]).expect("write");
            }
        });
        let zip = write_to(tmp.path(), "p.zip", &bytes);
        let few = Limits {
            max_entries: 4,
            ..limits()
        };
        let err = extract_zip(&zip, &tmp.path().join("a"), 0, &few).expect_err("entries");
        assert!(err.to_string().contains("entries"), "{err}");
        let small = Limits {
            max_total: 2000,
            ..limits()
        };
        let err = extract_zip(&zip, &tmp.path().join("b"), 0, &small).expect_err("size");
        assert!(err.to_string().contains("unpacks to more"), "{err}");
        let ok = Limits {
            max_total: 3000,
            ..limits()
        };
        extract_zip(&zip, &tmp.path().join("c"), 0, &ok).expect("exactly fits");
    }

    #[test]
    fn a_zip_bomb_is_stopped_by_the_ratio() {
        let tmp = tempfile::tempdir().expect("tmp");
        let bytes = zip_bytes(|w| {
            let deflated = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file("zeros", deflated).expect("file");
            w.write_all(&vec![0u8; 8 * 1024 * 1024]).expect("write");
        });
        let zip = write_to(tmp.path(), "p.zip", &bytes);
        let err = extract_zip(&zip, &tmp.path().join("out"), 0, &limits()).expect_err("bomb");
        assert!(err.to_string().contains("expands"), "{err}");
    }

    #[test]
    fn something_that_is_not_a_zip_is_an_archive_error() {
        let tmp = tempfile::tempdir().expect("tmp");
        let junk = write_to(tmp.path(), "p.zip", b"this is not a zip file");
        let err = extract_zip(&junk, &tmp.path().join("out"), 0, &limits()).expect_err("junk");
        assert!(matches!(err, Error::Archive(_)), "{err}");
    }

    // ---- tar ----

    fn tar_with(entries: &[(&str, tar::EntryType, &[u8], u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, kind, body, mode) in entries {
            let mut h = tar::Header::new_gnu();
            // Written by hand: `set_path` would refuse the very names these tests need.
            let raw = name.as_bytes();
            h.as_old_mut().name[..raw.len()].copy_from_slice(raw);
            h.set_entry_type(*kind);
            h.set_size(body.len() as u64);
            h.set_mode(*mode);
            if *kind == tar::EntryType::Symlink {
                h.set_link_name("/etc/passwd").expect("link");
            }
            h.set_cksum();
            out.extend_from_slice(h.as_bytes());
            out.extend_from_slice(body);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend_from_slice(&[0u8; 1024]);
        out
    }

    #[test]
    fn a_tar_is_unpacked_and_stripped() {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = tar_with(&[
            ("ffmpeg-7/", tar::EntryType::Directory, b"", 0o755),
            (
                "ffmpeg-7/bin/ffmpeg",
                tar::EntryType::Regular,
                b"ELF",
                0o755,
            ),
            ("ffmpeg-7/README", tar::EntryType::Regular, b"hi", 0o644),
        ]);
        let got = extract_tar(Cursor::new(data), &tmp.path().join("o"), 1, &limits()).expect("ok");
        assert_eq!(got, Extracted { files: 2, bytes: 5 });
        assert_eq!(
            fs::read(tmp.path().join("o/bin/ffmpeg")).expect("read"),
            b"ELF"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(tmp.path().join("o/bin/ffmpeg"))
                .expect("m")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }

    #[test]
    fn a_tar_that_climbs_out_or_links_is_refused() {
        let tmp = tempfile::tempdir().expect("tmp");
        for (name, kind) in [
            ("../evil", tar::EntryType::Regular),
            ("a/../../evil", tar::EntryType::Regular),
            ("/etc/evil", tar::EntryType::Regular),
            ("link", tar::EntryType::Symlink),
            ("hard", tar::EntryType::Link),
            ("dev", tar::EntryType::Char),
            ("fifo", tar::EntryType::Fifo),
        ] {
            let data = tar_with(&[(name, kind, b"", 0o644)]);
            let out = tmp.path().join("o");
            let err = extract_tar(Cursor::new(data), &out, 0, &limits()).expect_err(name);
            assert!(matches!(err, Error::Archive(_)), "{name}: {err}");
            let _ = fs::remove_dir_all(&out);
        }
        assert!(!tmp.path().join("evil").exists());
    }

    #[test]
    fn a_tar_that_is_too_big_is_stopped() {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = tar_with(&[("big", tar::EntryType::Regular, &[1u8; 5000], 0o644)]);
        let small = Limits {
            max_total: 4096,
            ..limits()
        };
        let err =
            extract_tar(Cursor::new(data), &tmp.path().join("o"), 0, &small).expect_err("big");
        assert!(err.to_string().contains("unpacks to more"), "{err}");
    }

    #[test]
    fn a_tar_that_names_a_file_twice_is_refused() {
        // A ZIP cannot even be written or read with a repeated name; a tar can.
        let tmp = tempfile::tempdir().expect("tmp");
        let data = tar_with(&[
            ("same", tar::EntryType::Regular, b"1", 0o644),
            ("same", tar::EntryType::Regular, b"2", 0o755),
        ]);
        let err =
            extract_tar(Cursor::new(data), &tmp.path().join("o"), 0, &limits()).expect_err("twice");
        assert!(err.to_string().contains("twice"), "{err}");
        assert_eq!(
            fs::read(tmp.path().join("o/same")).expect("read"),
            b"1",
            "the first one stays"
        );
    }

    // ---- compressed dependencies ----

    #[test]
    fn a_gzip_tar_and_a_single_gzip_file_are_unpacked() {
        let tmp = tempfile::tempdir().expect("tmp");
        let gz = |data: &[u8]| {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            e.write_all(data).expect("write");
            e.finish().expect("finish")
        };
        let tar = tar_with(&[("t/tool", tar::EntryType::Regular, b"bin", 0o755)]);
        let file = write_to(tmp.path(), "d.tar.gz", &gz(&tar));
        let got = extract(
            ArchiveKind::TarGz,
            &file,
            &tmp.path().join("a"),
            "tool",
            1,
            &limits(),
        )
        .expect("tar.gz");
        assert_eq!(got.files, 1);
        assert_eq!(fs::read(tmp.path().join("a/tool")).expect("read"), b"bin");

        let file = write_to(tmp.path(), "d.gz", &gz(b"single"));
        extract(
            ArchiveKind::Gz,
            &file,
            &tmp.path().join("b"),
            "tool",
            0,
            &limits(),
        )
        .expect("gz");
        assert_eq!(
            fs::read(tmp.path().join("b/tool")).expect("read"),
            b"single"
        );
    }

    #[test]
    fn a_raw_file_becomes_the_program_and_is_executable() {
        let tmp = tempfile::tempdir().expect("tmp");
        let file = write_to(tmp.path(), "yt-dlp_linux", b"\x7fELF");
        extract(
            ArchiveKind::Raw,
            &file,
            &tmp.path().join("d"),
            "yt-dlp",
            0,
            &limits(),
        )
        .expect("raw");
        let p = tmp.path().join("d/yt-dlp");
        assert_eq!(fs::read(&p).expect("read"), b"\x7fELF");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&p).expect("m").permissions().mode() & 0o777,
                0o755
            );
        }
        // The name is a plain name too.
        assert!(extract(
            ArchiveKind::Raw,
            &file,
            &tmp.path().join("e"),
            "../x",
            0,
            &limits()
        )
        .is_err());
    }

    #[test]
    fn a_zip_dependency_is_stripped_like_a_tar() {
        let tmp = tempfile::tempdir().expect("tmp");
        let bytes = zip_bytes(|w| {
            w.start_file("pkg-1/bin/tool", opts().unix_permissions(0o755))
                .expect("file");
            w.write_all(b"x").expect("write");
        });
        let zip = write_to(tmp.path(), "d.zip", &bytes);
        extract(
            ArchiveKind::Zip,
            &zip,
            &tmp.path().join("o"),
            "tool",
            1,
            &limits(),
        )
        .expect("zip");
        assert!(tmp.path().join("o/bin/tool").is_file());
    }

    #[test]
    fn xz_streams_are_read() {
        // "hello xz\n" compressed with CRC64 by a real `xz` (a fixed vector keeps the test free of an encoder).
        const XZ: &[u8] = &[
            0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00, 0x00, 0x04, 0xe6, 0xd6, 0xb4, 0x46, 0x02, 0x00,
            0x21, 0x01, 0x16, 0x00, 0x00, 0x00, 0x74, 0x2f, 0xe5, 0xa3, 0x01, 0x00, 0x08, 0x68,
            0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x78, 0x7a, 0x0a, 0x00, 0x00, 0x00, 0x00, 0xc1, 0x49,
            0x3a, 0xfa, 0x63, 0x52, 0x14, 0x5a, 0x00, 0x01, 0x21, 0x09, 0x6c, 0x18, 0xc5, 0xd5,
            0x1f, 0xb6, 0xf3, 0x7d, 0x01, 0x00, 0x00, 0x00, 0x00, 0x04, 0x59, 0x5a,
        ];
        let mut out = String::new();
        lzma_rust2::XzReader::new(Cursor::new(XZ), true)
            .read_to_string(&mut out)
            .expect("xz");
        assert_eq!(out, "hello xz\n");
    }
}
