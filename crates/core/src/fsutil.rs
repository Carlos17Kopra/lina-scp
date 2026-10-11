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
    let tmp = temp_path(path)?;
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
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
    fn create_private_dir_uses_0700() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a/b");
        create_private_dir(&dir).unwrap();
        assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    }
}
