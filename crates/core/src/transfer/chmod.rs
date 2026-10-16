//! Changing permission bits on either side, optionally recursive (TP4a).

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

use super::side::{Child, NodeKind, Side};
use crate::error::{CoreError, FsKind, Result};
use crate::sftp::join;
use crate::validate::validate_file_name;

/// Bits to set and to clear (each within `0o777`); bits in neither stay as they are per entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModeChange {
    pub set: u32,
    pub clear: u32,
}

impl ModeChange {
    /// New permission bits for an entry with `mode`; special bits stay. `dir_x`: x wherever r is set.
    fn apply(self, mode: u32, dir_x: bool) -> u32 {
        let mut perms = (mode & 0o777 & !self.clear) | self.set;
        if dir_x {
            perms |= (perms & 0o444) >> 2;
        }
        (mode & 0o7000) | perms
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChmodFailure {
    pub path: String,
    pub error: CoreError,
}

/// Applies `change` to every path. Recursive: descends into directories without following symlinks, and
/// every directory also gets x wherever r is set. Per-entry errors are collected; only a lost connection
/// (`CoreError::Network`) aborts.
pub async fn chmod(
    side: &dyn Side,
    paths: &[String],
    change: ModeChange,
    recursive: bool,
) -> Result<Vec<ChmodFailure>> {
    if (change.set | change.clear) & !0o777 != 0 {
        return Err(CoreError::config("ungültige Rechte"));
    }
    let mut failures = Vec::new();
    for path in paths {
        visit(side, path.clone(), change, recursive, true, &mut failures).await?;
    }
    Ok(failures)
}

fn keep(error: CoreError, path: &str, failures: &mut Vec<ChmodFailure>) -> Result<()> {
    if matches!(error, CoreError::Network { .. }) {
        return Err(error);
    }
    failures.push(ChmodFailure { path: path.to_string(), error });
    Ok(())
}

async fn set_mode(side: &dyn Side, path: &str, mode: u32, failures: &mut Vec<ChmodFailure>) -> Result<()> {
    match side.set_mode(path, mode).await {
        Ok(()) => Ok(()),
        Err(error) => keep(error, path, failures),
    }
}

fn visit<'a>(
    side: &'a dyn Side,
    path: String,
    change: ModeChange,
    recursive: bool,
    top: bool,
    failures: &'a mut Vec<ChmodFailure>,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        let node = match side.stat(&path).await {
            Ok(Some(node)) => node,
            Ok(None) => return keep(CoreError::fs(FsKind::NotFound, &path, "not found"), &path, failures),
            Err(error) => return keep(error, &path, failures),
        };
        // A selected link changes its target (like chmod); below the selection links are never touched.
        if node.link && !top {
            return Ok(());
        }
        let dir_like = matches!(node.kind, NodeKind::Dir | NodeKind::DirLink);
        let mode = change.apply(node.mode, recursive && dir_like);
        if !(recursive && node.kind == NodeKind::Dir && !node.link) {
            return set_mode(side, &path, mode, failures).await;
        }
        // Children first, so revoking r/x on the directory does not lock out the descent; when the listing is
        // refused, granting on the directory first may be what makes it readable.
        match side.children(&path).await {
            Ok(children) => {
                descend(side, &path, children, change, failures).await?;
                set_mode(side, &path, mode, failures).await
            }
            Err(error @ CoreError::Network { .. }) => Err(error),
            Err(_) => {
                set_mode(side, &path, mode, failures).await?;
                match side.children(&path).await {
                    Ok(children) => descend(side, &path, children, change, failures).await,
                    Err(error) => keep(error, &path, failures),
                }
            }
        }
    })
}

async fn descend(
    side: &dyn Side,
    dir: &str,
    children: Vec<Child>,
    change: ModeChange,
    failures: &mut Vec<ChmodFailure>,
) -> Result<()> {
    for child in children {
        // ponytail: lossy and hostile names are skipped silently, like the transfer engine does.
        if child.lossy || validate_file_name(&child.name).is_err() {
            continue;
        }
        visit(side, join(dir, &child.name), change, true, false, failures).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::LocalSide;
    use crate::transfer::test_support::{FaultySide, p};
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn mode(path: &Path) -> u32 {
        std::fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
    }

    fn set(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// The change that makes the permission bits exactly `mode`.
    fn exact(mode: u32) -> ModeChange {
        ModeChange { set: mode, clear: 0o777 & !mode }
    }

    #[tokio::test]
    async fn masks_keep_unmentioned_bits_per_entry() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::write(&a, b"").unwrap();
        std::fs::write(&b, b"").unwrap();
        set(&a, 0o600);
        set(&b, 0o640);
        let change = ModeChange { set: 0o004, clear: 0o200 };
        let failures = chmod(&LocalSide, &[p(&a), p(&b)], change, false).await.unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!((mode(&a), mode(&b)), (0o404, 0o444));
    }

    #[tokio::test]
    async fn special_bits_stay() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("shared");
        std::fs::create_dir(&dir).unwrap();
        set(&dir, 0o1777);
        chmod(&LocalSide, &[p(&dir)], ModeChange { set: 0, clear: 0o002 }, false).await.unwrap();
        assert_eq!(mode(&dir), 0o1775);
    }

    #[tokio::test]
    async fn recursive_gives_dirs_x_where_r_and_files_the_exact_mode() {
        let d = tempfile::tempdir().unwrap();
        let top = d.path().join("d");
        std::fs::create_dir_all(top.join("sub")).unwrap();
        std::fs::write(top.join("f"), b"").unwrap();
        std::fs::write(top.join("sub/g"), b"").unwrap();
        for path in [&top, &top.join("sub")] {
            set(path, 0o700);
        }
        let failures = chmod(&LocalSide, &[p(&top)], exact(0o644), true).await.unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!((mode(&top), mode(&top.join("sub"))), (0o755, 0o755));
        assert_eq!((mode(&top.join("f")), mode(&top.join("sub/g"))), (0o644, 0o644));
    }

    #[tokio::test]
    async fn without_recursion_a_dir_gets_exactly_the_mode() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("empty");
        std::fs::create_dir(&dir).unwrap();
        chmod(&LocalSide, &[p(&dir)], exact(0o600), false).await.unwrap();
        assert_eq!(mode(&dir), 0o600);
    }

    #[tokio::test]
    async fn recursion_never_follows_or_changes_symlinks_but_a_selected_link_changes_its_target() {
        let d = tempfile::tempdir().unwrap();
        let (top, outside) = (d.path().join("d"), d.path().join("outside"));
        std::fs::create_dir_all(outside.join("inner")).unwrap();
        std::fs::create_dir(&top).unwrap();
        std::fs::write(outside.join("file"), b"").unwrap();
        set(&outside.join("file"), 0o600);
        set(&outside.join("inner"), 0o700);
        std::os::unix::fs::symlink(outside.join("file"), top.join("lf")).unwrap();
        std::os::unix::fs::symlink(&outside, top.join("ld")).unwrap();
        chmod(&LocalSide, &[p(&top)], exact(0o644), true).await.unwrap();
        assert_eq!((mode(&outside.join("file")), mode(&outside.join("inner"))), (0o600, 0o700));
        chmod(&LocalSide, &[p(&top.join("lf"))], exact(0o640), true).await.unwrap();
        assert_eq!(mode(&outside.join("file")), 0o640);
    }

    #[tokio::test]
    async fn revoking_access_on_a_dir_still_reaches_its_children() {
        let d = tempfile::tempdir().unwrap();
        let top = d.path().join("d");
        std::fs::create_dir(&top).unwrap();
        std::fs::write(top.join("f"), b"").unwrap();
        set(&top.join("f"), 0o600);
        let failures = chmod(&LocalSide, &[p(&top)], exact(0o000), true).await.unwrap();
        let top_mode = mode(&top);
        set(&top, 0o700);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!((top_mode, mode(&top.join("f"))), (0o000, 0o000));
    }

    #[tokio::test]
    async fn granting_access_on_an_unreadable_dir_reaches_its_children() {
        let d = tempfile::tempdir().unwrap();
        let top = d.path().join("d");
        std::fs::create_dir(&top).unwrap();
        std::fs::write(top.join("f"), b"").unwrap();
        set(&top.join("f"), 0o600);
        set(&top, 0o000);
        let failures = chmod(&LocalSide, &[p(&top)], exact(0o644), true).await.unwrap();
        set(&top, 0o700);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(mode(&top.join("f")), 0o644);
    }

    #[tokio::test]
    async fn failures_are_collected_and_the_rest_continues() {
        let d = tempfile::tempdir().unwrap();
        let (a, b, gone) = (d.path().join("a"), d.path().join("b"), d.path().join("gone"));
        std::fs::write(&a, b"").unwrap();
        std::fs::write(&b, b"").unwrap();
        let side = FaultySide::new();
        side.deny_set_mode(&p(&a));
        let failures = chmod(&*side, &[p(&a), p(&gone), p(&b)], exact(0o640), false).await.unwrap();
        assert_eq!(failures.iter().map(|f| f.path.clone()).collect::<Vec<_>>(), [p(&a), p(&gone)]);
        assert!(matches!(failures[1].error, CoreError::Fs { kind: FsKind::NotFound, .. }));
        assert_eq!(mode(&b), 0o640);
    }

    #[tokio::test]
    async fn a_lost_connection_aborts() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("d")).unwrap();
        let side = FaultySide::new();
        side.fail_children();
        let result = chmod(&*side, &[p(&d.path().join("d"))], exact(0o755), true).await;
        assert!(matches!(result, Err(CoreError::Network { .. })), "{result:?}");
    }

    #[tokio::test]
    async fn lossy_and_hostile_child_names_are_skipped() {
        use std::os::unix::ffi::OsStrExt;
        let d = tempfile::tempdir().unwrap();
        let top = d.path().join("d");
        std::fs::create_dir(&top).unwrap();
        let bad = top.join(std::ffi::OsStr::from_bytes(b"bad\xff"));
        std::fs::write(&bad, b"").unwrap();
        set(&bad, 0o600);
        let side = FaultySide::new();
        side.add_fake_children(&["../escape", ".."]);
        let failures = chmod(&*side, &[p(&top)], exact(0o644), true).await.unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(mode(&bad), 0o600);
    }

    #[tokio::test]
    async fn masks_outside_the_permission_bits_are_rejected() {
        let d = tempfile::tempdir().unwrap();
        for change in [ModeChange { set: 0o4000, clear: 0 }, ModeChange { set: 0, clear: 0o10000 }] {
            let result = chmod(&LocalSide, &[p(d.path())], change, false).await;
            assert!(matches!(result, Err(CoreError::Config { .. })), "{result:?}");
        }
    }
}
