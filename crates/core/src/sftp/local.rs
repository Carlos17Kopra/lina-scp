//! Local file operations of the commander's left pane.

use std::collections::HashMap;
use std::ffi::CString;
use std::fs::{self, Metadata, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::OnceLock;

use super::{Entry, EntryKind, LinkTarget, Listing, join};
use crate::error::{CoreError, FsKind, Result};
use crate::paths::expand_tilde;
use crate::validate::validate_file_name;

pub(crate) fn io_error(error: io::Error, path: &str) -> CoreError {
    let kind = match error.kind() {
        io::ErrorKind::PermissionDenied => FsKind::PermissionDenied,
        io::ErrorKind::NotFound => FsKind::NotFound,
        io::ErrorKind::AlreadyExists => FsKind::AlreadyExists,
        io::ErrorKind::DirectoryNotEmpty => FsKind::NotEmpty,
        io::ErrorKind::CrossesDevices => FsKind::CrossDevice,
        _ => FsKind::Other,
    };
    CoreError::fs(kind, path, error.to_string())
}

/// uid -> user name from `/etc/passwd`, read once.
// ponytail: ignores NSS (LDAP/sssd users show their uid); fine for a desktop.
fn user_name(uid: u32) -> String {
    static USERS: OnceLock<HashMap<u32, String>> = OnceLock::new();
    let users = USERS.get_or_init(|| {
        fs::read_to_string("/etc/passwd")
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let mut fields = line.split(':');
                let name = fields.next()?;
                let uid = fields.nth(1)?.parse().ok()?;
                Some((uid, name.to_string()))
            })
            .collect()
    });
    users.get(&uid).cloned().unwrap_or_else(|| uid.to_string())
}

fn link_target(path: &Path) -> Option<LinkTarget> {
    let meta = fs::metadata(path).ok()?;
    Some(if meta.is_dir() {
        LinkTarget::Dir
    } else if meta.is_file() {
        LinkTarget::File
    } else {
        LinkTarget::Other
    })
}

fn entry(dir: &Path, name: &std::ffi::OsStr, meta: &Metadata) -> Entry {
    let kind = if meta.file_type().is_symlink() {
        EntryKind::Symlink { target: link_target(&dir.join(name)) }
    } else if meta.is_dir() {
        EntryKind::Dir
    } else if meta.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let utf8 = name.to_str();
    Entry {
        name: utf8.map_or_else(|| name.to_string_lossy().into_owned(), str::to_string),
        kind,
        size: meta.len(),
        mtime: Some(meta.mtime()),
        mode: Some(meta.mode()),
        owner: Some(user_name(meta.uid())),
        name_lossy: utf8.is_none(),
    }
}

pub fn list(path: &str) -> Result<Listing> {
    let canonical = fs::canonicalize(path).map_err(|e| io_error(e, path))?;
    let shown = canonical.to_string_lossy().into_owned();
    let mut entries = Vec::new();
    for item in fs::read_dir(&canonical).map_err(|e| io_error(e, &shown))? {
        let item = item.map_err(|e| io_error(e, &shown))?;
        // An entry that vanished between readdir and lstat is simply skipped.
        if let Ok(meta) = item.metadata() {
            entries.push(entry(&canonical, &item.file_name(), &meta));
        }
    }
    Ok(Listing { path: shown, entries })
}

/// Start directory of the local pane: the host's `local_start` (`~` expanded), else `$HOME`.
pub fn home(local_start: Option<&str>, home: &Path) -> String {
    match local_start.map(str::trim).filter(|s| !s.is_empty()) {
        Some(start) => expand_tilde(start, home).to_string_lossy().into_owned(),
        None => home.to_string_lossy().into_owned(),
    }
}

pub fn mkdir(dir: &str, name: &str) -> Result<()> {
    validate_file_name(name)?;
    let path = join(dir, name);
    fs::DirBuilder::new().mode(0o755).create(&path).map_err(|e| io_error(e, &path))
}

/// Creates an empty file; an existing name is never touched.
pub fn create_file(dir: &str, name: &str) -> Result<()> {
    validate_file_name(name)?;
    let path = join(dir, name);
    OpenOptions::new().write(true).create_new(true).mode(0o644).open(&path).map(drop).map_err(|e| io_error(e, &path))
}

/// Same-directory rename that never replaces an existing target (`RENAME_NOREPLACE`).
pub fn rename(dir: &str, from: &str, to: &str) -> Result<()> {
    validate_file_name(from)?;
    validate_file_name(to)?;
    let (old, new) = (join(dir, from), join(dir, to));
    rename_noreplace(Path::new(&old), Path::new(&new)).map_err(|e| io_error(e, &new))
}

/// `rename` that fails with `AlreadyExists` instead of replacing `new` (`RENAME_NOREPLACE`).
fn rename_noreplace(old: &Path, new: &Path) -> io::Result<()> {
    let c = |p: &Path| CString::new(p.as_os_str().as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput));
    let (c_old, c_new) = (c(old)?, c(new)?);
    // SAFETY: both pointers are valid NUL-terminated strings for the duration of the call.
    let rc = unsafe {
        libc::renameat2(libc::AT_FDCWD, c_old.as_ptr(), libc::AT_FDCWD, c_new.as_ptr(), libc::RENAME_NOREPLACE)
    };
    if rc == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Permanent delete; directories recursively. `remove_dir_all` never follows symlinks, and a link
/// itself (even one to a directory) is removed with `remove_file`.
pub fn delete(dir: &str, names: &[String]) -> Result<()> {
    for name in names {
        validate_file_name(name)?;
    }
    for name in names {
        let path = join(dir, name);
        let meta = fs::symlink_metadata(&path).map_err(|e| io_error(e, &path))?;
        let result = if meta.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
        result.map_err(|e| io_error(e, &path))?;
    }
    Ok(())
}

/// Percent-encodes a path for `.trashinfo` (RFC 2396 escaping, `/` kept).
fn percent_encode(path: &[u8]) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Local time as `YYYY-MM-DDThh:mm:ss` (freedesktop `DeletionDate`).
fn deletion_date() -> String {
    // SAFETY: `time` and `localtime_r` only write into the provided, properly sized structs.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

/// Moves entries into the freedesktop home trash `trash_root` (`files/` + `info/*.trashinfo`).
// ponytail: home trash only (rename); other file systems fail with EXDEV -> user deletes permanently.
pub fn trash(trash_root: &Path, dir: &str, names: &[String]) -> Result<()> {
    for name in names {
        validate_file_name(name)?;
    }
    let (files, info) = (trash_root.join("files"), trash_root.join("info"));
    for d in [&files, &info] {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(d).map_err(|e| io_error(e, &d.to_string_lossy()))?;
    }
    for name in names {
        let path = join(dir, name);
        fs::symlink_metadata(&path).map_err(|e| io_error(e, &path))?;
        let mut n = 0u32;
        loop {
            let candidate = if n == 0 { name.clone() } else { format!("{name}.{n}") };
            let info_path = info.join(format!("{candidate}.trashinfo"));
            let mut file = match OpenOptions::new().write(true).create_new(true).mode(0o600).open(&info_path) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    n += 1;
                    continue;
                }
                Err(e) => return Err(io_error(e, &info_path.to_string_lossy())),
            };
            let body = format!(
                "[Trash Info]\nPath={}\nDeletionDate={}\n",
                percent_encode(Path::new(&path).as_os_str().as_bytes()),
                deletion_date()
            );
            // NOREPLACE: an orphan in `files/` without its `.trashinfo` is never overwritten.
            let moved = file
                .write_all(body.as_bytes())
                .and_then(|()| rename_noreplace(Path::new(&path), &files.join(&candidate)));
            if let Err(e) = moved {
                let _ = fs::remove_file(&info_path);
                if e.kind() == io::ErrorKind::AlreadyExists {
                    n += 1;
                    continue;
                }
                // EXDEV -> FsKind::CrossDevice: the UI tells the user to delete permanently instead.
                return Err(io_error(e, &path));
            }
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FsKind;
    use crate::sftp::{EntryKind, LinkTarget};
    use std::os::unix::ffi::OsStrExt;

    fn tmp() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().to_str().unwrap().to_string();
        (dir, path)
    }

    fn entry<'a>(listing: &'a Listing, name: &str) -> &'a crate::sftp::Entry {
        listing.entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} missing"))
    }

    #[test]
    fn list_reports_kinds_sizes_and_links() {
        let (_t, d) = tmp();
        std::fs::write(format!("{d}/f.txt"), b"abc").unwrap();
        std::fs::create_dir(format!("{d}/sub")).unwrap();
        std::os::unix::fs::symlink(format!("{d}/sub"), format!("{d}/to-dir")).unwrap();
        std::os::unix::fs::symlink(format!("{d}/gone"), format!("{d}/dangling")).unwrap();
        let listing = list(&d).unwrap();
        assert_eq!(listing.path, d);
        assert_eq!(listing.entries.len(), 4, "no . or ..");
        let f = entry(&listing, "f.txt");
        assert_eq!((f.kind.clone(), f.size), (EntryKind::File, 3));
        assert!(f.mtime.is_some() && f.mode.is_some() && f.owner.is_some());
        assert_eq!(entry(&listing, "sub").kind, EntryKind::Dir);
        assert_eq!(entry(&listing, "to-dir").kind, EntryKind::Symlink { target: Some(LinkTarget::Dir) });
        assert_eq!(entry(&listing, "dangling").kind, EntryKind::Symlink { target: None });
    }

    #[test]
    fn list_canonicalizes_the_path() {
        let (_t, d) = tmp();
        std::fs::create_dir(format!("{d}/sub")).unwrap();
        assert_eq!(list(&format!("{d}/sub/..")).unwrap().path, d);
    }

    #[test]
    fn list_of_a_missing_dir_is_not_found() {
        let (_t, d) = tmp();
        let error = list(&format!("{d}/nope")).unwrap_err();
        assert!(matches!(error, CoreError::Fs { kind: FsKind::NotFound, .. }), "{error:?}");
    }

    #[test]
    fn mkdir_refuses_an_existing_name() {
        let (_t, d) = tmp();
        mkdir(&d, "new").unwrap();
        assert!(std::path::Path::new(&format!("{d}/new")).is_dir());
        assert!(matches!(mkdir(&d, "new"), Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
    }

    #[test]
    fn create_file_makes_an_empty_file_and_never_overwrites() {
        use std::os::unix::fs::PermissionsExt;
        let (_t, d) = tmp();
        std::fs::write(format!("{d}/old.txt"), "keep").unwrap();
        create_file(&d, "new.txt").unwrap();
        let meta = std::fs::metadata(format!("{d}/new.txt")).unwrap();
        assert_eq!(meta.len(), 0);
        assert_eq!(meta.permissions().mode() & 0o111, 0, "never executable");
        assert!(matches!(create_file(&d, "old.txt"), Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
        assert_eq!(std::fs::read_to_string(format!("{d}/old.txt")).unwrap(), "keep");
        assert!(matches!(create_file(&d, "a/b"), Err(CoreError::Config { .. })));
    }

    #[test]
    fn rename_never_overwrites() {
        let (_t, d) = tmp();
        std::fs::write(format!("{d}/a"), b"a").unwrap();
        std::fs::write(format!("{d}/b"), b"b").unwrap();
        assert!(matches!(rename(&d, "a", "b"), Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
        assert_eq!(std::fs::read(format!("{d}/b")).unwrap(), b"b");
        rename(&d, "a", "c").unwrap();
        assert_eq!(std::fs::read(format!("{d}/c")).unwrap(), b"a");
    }

    #[test]
    fn delete_removes_links_not_their_targets() {
        let (_t, d) = tmp();
        let (_o, outside) = tmp();
        std::fs::write(format!("{outside}/keep"), b"k").unwrap();
        std::fs::create_dir_all(format!("{d}/tree/inner")).unwrap();
        std::fs::write(format!("{d}/tree/inner/x"), b"x").unwrap();
        std::os::unix::fs::symlink(&outside, format!("{d}/tree/link")).unwrap();
        std::os::unix::fs::symlink(&outside, format!("{d}/toplink")).unwrap();
        delete(&d, &["tree".into(), "toplink".into()]).unwrap();
        assert!(list(&d).unwrap().entries.is_empty());
        assert!(std::path::Path::new(&format!("{outside}/keep")).exists());
    }

    #[test]
    fn invalid_names_are_config_errors() {
        let (_t, d) = tmp();
        assert!(matches!(mkdir(&d, "a/b"), Err(CoreError::Config { .. })));
        assert!(matches!(rename(&d, "..", "x"), Err(CoreError::Config { .. })));
        assert!(matches!(delete(&d, &["".into()]), Err(CoreError::Config { .. })));
    }

    #[test]
    fn non_utf8_names_are_listed_lossy_and_cannot_be_operated_on() {
        let (_t, d) = tmp();
        let raw = std::ffi::OsStr::from_bytes(b"bad\xffname");
        std::fs::write(std::path::Path::new(&d).join(raw), b"").unwrap();
        let listing = list(&d).unwrap();
        let lossy = &listing.entries[0];
        assert!(lossy.name_lossy);
        assert!(matches!(delete(&d, std::slice::from_ref(&lossy.name)), Err(CoreError::Config { .. })));
        assert_eq!(list(&d).unwrap().entries.len(), 1);
    }

    #[test]
    fn trash_moves_into_files_and_writes_trashinfo() {
        let (_t, d) = tmp();
        let (_tr, trash_root) = tmp();
        let trash_root = std::path::Path::new(&trash_root).join("Trash");
        std::fs::write(format!("{d}/a b.txt"), b"1").unwrap();
        trash(&trash_root, &d, &["a b.txt".into()]).unwrap();
        std::fs::write(format!("{d}/a b.txt"), b"2").unwrap();
        trash(&trash_root, &d, &["a b.txt".into()]).unwrap();
        assert!(list(&d).unwrap().entries.is_empty());
        assert_eq!(std::fs::read(trash_root.join("files/a b.txt")).unwrap(), b"1");
        assert_eq!(std::fs::read(trash_root.join("files/a b.txt.1")).unwrap(), b"2");
        let info = std::fs::read_to_string(trash_root.join("info/a b.txt.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath="), "{info}");
        assert!(info.contains("/a%20b.txt\n"), "{info}");
        assert!(info.contains("\nDeletionDate=20"), "{info}");
    }

    #[test]
    fn trash_never_replaces_an_orphan_in_files() {
        let (_t, d) = tmp();
        let (_tr, trash_root) = tmp();
        let trash_root = std::path::Path::new(&trash_root).join("Trash");
        std::fs::create_dir_all(trash_root.join("files")).unwrap();
        std::fs::write(trash_root.join("files/a"), b"orphan").unwrap();
        std::fs::write(format!("{d}/a"), b"new").unwrap();
        trash(&trash_root, &d, &["a".into()]).unwrap();
        assert_eq!(std::fs::read(trash_root.join("files/a")).unwrap(), b"orphan");
        assert_eq!(std::fs::read(trash_root.join("files/a.1")).unwrap(), b"new");
        assert!(trash_root.join("info/a.1.trashinfo").exists() && !trash_root.join("info/a.trashinfo").exists());
    }

    #[test]
    fn cross_device_errors_have_their_own_kind() {
        let error = io_error(io::Error::from_raw_os_error(libc::EXDEV), "/media/usb/x");
        assert!(matches!(error, CoreError::Fs { kind: FsKind::CrossDevice, .. }), "{error:?}");
    }

    #[test]
    fn home_prefers_local_start() {
        let home_dir = std::path::Path::new("/home/u");
        assert_eq!(home(Some("/srv"), home_dir), "/srv");
        assert_eq!(home(Some("~/proj"), home_dir), "/home/u/proj");
        assert_eq!(home(Some("  "), home_dir), "/home/u");
        assert_eq!(home(None, home_dir), "/home/u");
    }
}
