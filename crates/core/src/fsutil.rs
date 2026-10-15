//! Small filesystem helpers for private (0600/0700) files and atomic writes.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};

/// Creates `dir` (and missing parents) with mode 0700. Existing directories are left untouched.
pub(crate) fn create_private_dir(dir: &Path) -> Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    Ok(())
}

/// Atomically replaces `path` with `contents` (temp file + rename), mode 0600.
/// The parent directory must exist.
pub(crate) fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    write_private_if(path, contents, || Ok(true)).map(|_| ())
}

/// Like `write_private`, but only renames the temp file into place if `precondition` (checked
/// right before the rename) still holds. Returns whether `path` was replaced.
// ponytail: optimistic check, not a lock; a write between the check and the rename can still be lost.
pub(crate) fn write_private_if(
    path: &Path,
    contents: &[u8],
    precondition: impl FnOnce() -> Result<bool>,
) -> Result<bool> {
    let tmp = temp_path(path)?;
    let result = (|| -> Result<bool> {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        if !precondition()? {
            return Ok(false);
        }
        fs::rename(&tmp, path)?;
        Ok(true)
    })();
    if !matches!(result, Ok(true)) {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn temp_path(path: &Path) -> Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| CoreError::internal(format!("not a file path: {}", path.display())))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    Ok(path.with_file_name(tmp_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn write_private_creates_0600_file_and_replaces_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("f");
        write_private(&path, b"one").unwrap();
        write_private(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn write_private_if_skips_rename_when_precondition_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("f");
        fs::write(&path, b"old").unwrap();
        assert!(!write_private_if(&path, b"new", || Ok(false)).unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
        assert!(write_private_if(&path, b"new", || Ok(true)).unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn create_private_dir_uses_0700() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a/b");
        create_private_dir(&dir).unwrap();
        assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    }
}
