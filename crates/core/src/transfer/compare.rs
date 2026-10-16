//! Recursive comparison of a local and a remote directory (TP4b).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use futures::{StreamExt, stream};
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::side::{Node, NodeKind, Side};
use crate::error::{CoreError, Result};
use crate::sftp::join;
use crate::validate::validate_file_name;

/// Concurrent `stat`s per directory listing.
const STAT_CONCURRENCY: usize = 32;
/// Modification times this close count as equal (FAT and some servers round to 1-2 s).
const MTIME_TOLERANCE_S: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DiffStatus {
    OnlyLocal,
    OnlyRemote,
    LocalNewer,
    RemoteNewer,
    /// Same modification time, different size.
    SizeDiffers,
    /// A file on one side, a directory on the other.
    TypeMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    pub dir: bool,
    pub size: u64,
    pub mtime: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Difference {
    /// Relative to both start directories, `/`-separated.
    pub rel: String,
    pub status: DiffStatus,
    pub local: Option<NodeInfo>,
    pub remote: Option<NodeInfo>,
}

/// `*` (any run) and `?` (one char) against a whole name.
pub(crate) fn matches(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

fn info(node: &Node) -> NodeInfo {
    NodeInfo { dir: node.kind == NodeKind::Dir, size: node.size, mtime: node.mtime }
}

/// Usable children of `dir` by name. Any error (also a refused listing) aborts the comparison: an unreadable
/// directory must never look empty.
async fn entries(side: &dyn Side, dir: String, exclude: &[String]) -> Result<BTreeMap<String, Node>> {
    let names: Vec<String> = side
        .children(&dir)
        .await?
        .into_iter()
        .filter(|c| !c.lossy && validate_file_name(&c.name).is_ok() && !exclude.iter().any(|p| matches(p, &c.name)))
        .map(|c| c.name)
        .collect();
    let nodes: Vec<Result<Option<Node>>> = stream::iter(names.clone())
        .map(|name| {
            let path = join(&dir, &name);
            async move { side.stat(&path).await }
        })
        .buffered(STAT_CONCURRENCY)
        .collect()
        .await;
    let mut out = BTreeMap::new();
    for (name, node) in names.into_iter().zip(nodes) {
        // `None`: vanished since the listing.
        if let Some(node) = node? {
            out.insert(name, node);
        }
    }
    Ok(out)
}

fn compare_files(local: &Node, remote: &Node) -> Option<DiffStatus> {
    let newer = match (local.mtime, remote.mtime) {
        (Some(l), Some(r)) if (l - r).abs() > MTIME_TOLERANCE_S => {
            Some(if l > r { DiffStatus::LocalNewer } else { DiffStatus::RemoteNewer })
        }
        _ => None,
    };
    newer.or((local.size != remote.size).then_some(DiffStatus::SizeDiffers))
}

/// Differences between `local_dir` and `remote_dir`, recursively. Equal files are left out; a directory on one side
/// only is one entry; sorted by path. Names matching an `exclude` pattern are skipped at every depth.
pub async fn compare(
    local: &dyn Side,
    remote: &dyn Side,
    local_dir: &str,
    remote_dir: &str,
    exclude: &[String],
    cancel: &CancellationToken,
) -> Result<Vec<Difference>> {
    let mut out = Vec::new();
    let mut dirs = VecDeque::from([String::new()]);
    while let Some(rel) = dirs.pop_front() {
        if cancel.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let at = |base: &str| if rel.is_empty() { base.to_string() } else { join(base, &rel) };
        let (l, r) =
            tokio::try_join!(entries(local, at(local_dir), exclude), entries(remote, at(remote_dir), exclude))?;
        let names: BTreeSet<&String> = l.keys().chain(r.keys()).collect();
        for name in names {
            let (a, b) = (l.get(name), r.get(name));
            // A dir link, broken link or special file on either side hides the name on both: it must never look
            // missing (and get deleted or overwritten) because the other side has a link there.
            if [a, b].into_iter().flatten().any(|n| !matches!(n.kind, NodeKind::File | NodeKind::Dir)) {
                continue;
            }
            let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let status = match (a, b) {
                (Some(_), None) => Some(DiffStatus::OnlyLocal),
                (None, Some(_)) => Some(DiffStatus::OnlyRemote),
                (Some(a), Some(b)) => match (a.kind, b.kind) {
                    (NodeKind::Dir, NodeKind::Dir) => {
                        dirs.push_back(child.clone());
                        None
                    }
                    (NodeKind::File, NodeKind::File) => compare_files(a, b),
                    _ => Some(DiffStatus::TypeMismatch),
                },
                (None, None) => None,
            };
            if let Some(status) = status {
                out.push(Difference { rel: child, status, local: a.map(info), remote: b.map(info) });
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::LocalSide;
    use crate::transfer::test_support::{FaultySide, p};
    use std::path::Path;
    use std::time::{Duration, UNIX_EPOCH};

    fn file(path: &Path, data: &[u8], mtime: u64) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(mtime))
            .unwrap();
    }

    struct Trees {
        _d: tempfile::TempDir,
        l: std::path::PathBuf,
        r: std::path::PathBuf,
    }

    fn trees() -> Trees {
        let d = tempfile::tempdir().unwrap();
        let (l, r) = (d.path().join("l"), d.path().join("r"));
        std::fs::create_dir_all(&l).unwrap();
        std::fs::create_dir_all(&r).unwrap();
        Trees { _d: d, l, r }
    }

    async fn run(t: &Trees, exclude: &[&str]) -> Result<Vec<(String, DiffStatus)>> {
        let exclude: Vec<String> = exclude.iter().map(|s| s.to_string()).collect();
        let diffs = compare(&LocalSide, &LocalSide, &p(&t.l), &p(&t.r), &exclude, &CancellationToken::new()).await?;
        Ok(diffs.into_iter().map(|d| (d.rel, d.status)).collect())
    }

    #[test]
    fn wildcards_match_whole_names() {
        for (pat, name, want) in [
            ("*.tmp", "a.tmp", true),
            ("*.tmp", "a.tmpx", false),
            ("node_modules", "node_modules", true),
            ("node_modules", "node_modules2", false),
            ("?.log", "a.log", true),
            ("?.log", "ab.log", false),
            ("*", "", true),
            ("a*b*c", "axxbyyc", true),
            ("a*b*c", "axxbyy", false),
        ] {
            assert_eq!(matches(pat, name), want, "{pat} {name}");
        }
    }

    #[tokio::test]
    async fn every_status_with_equal_files_left_out() {
        let t = trees();
        file(&t.l.join("same"), b"x", 1000);
        file(&t.r.join("same"), b"x", 1000);
        file(&t.l.join("tolerance"), b"x", 1000);
        file(&t.r.join("tolerance"), b"x", 1001);
        file(&t.l.join("only_l"), b"x", 1000);
        file(&t.r.join("only_r"), b"x", 1000);
        file(&t.l.join("newer_l"), b"x", 2000);
        file(&t.r.join("newer_l"), b"xyz", 1000);
        file(&t.l.join("newer_r"), b"x", 1000);
        file(&t.r.join("newer_r"), b"x", 2000);
        file(&t.l.join("size"), b"x", 1000);
        file(&t.r.join("size"), b"xy", 1000);
        file(&t.l.join("mismatch"), b"x", 1000);
        std::fs::create_dir(t.r.join("mismatch")).unwrap();
        use DiffStatus::*;
        assert_eq!(
            run(&t, &[]).await.unwrap(),
            [
                ("mismatch".into(), TypeMismatch),
                ("newer_l".into(), LocalNewer),
                ("newer_r".into(), RemoteNewer),
                ("only_l".into(), OnlyLocal),
                ("only_r".into(), OnlyRemote),
                ("size".into(), SizeDiffers),
            ]
        );
    }

    #[tokio::test]
    async fn descends_into_common_dirs_and_reports_one_sided_dirs_once() {
        let t = trees();
        file(&t.l.join("a/b/deep"), b"x", 1000);
        file(&t.r.join("a/b/deep"), b"x", 3000);
        file(&t.l.join("a/new/x"), b"x", 1000);
        file(&t.l.join("a/new/y"), b"x", 1000);
        let diffs = compare(&LocalSide, &LocalSide, &p(&t.l), &p(&t.r), &[], &CancellationToken::new()).await.unwrap();
        assert_eq!(diffs.len(), 2, "{diffs:?}");
        assert_eq!((diffs[0].rel.as_str(), diffs[0].status), ("a/b/deep", DiffStatus::RemoteNewer));
        assert_eq!(diffs[0].remote, Some(NodeInfo { dir: false, size: 1, mtime: Some(3000) }));
        assert_eq!((diffs[1].rel.as_str(), diffs[1].status), ("a/new", DiffStatus::OnlyLocal));
        assert!(diffs[1].local.unwrap().dir && diffs[1].remote.is_none());
    }

    #[tokio::test]
    async fn exclude_patterns_apply_at_every_depth() {
        let t = trees();
        file(&t.l.join(".git/HEAD"), b"x", 1000);
        file(&t.l.join("src/node_modules/m"), b"x", 1000);
        file(&t.r.join("src/x.tmp"), b"x", 1000);
        file(&t.l.join("src/keep"), b"x", 1000);
        std::fs::create_dir_all(t.r.join("src")).unwrap();
        let diffs = run(&t, &[".git", "node_modules", "*.tmp"]).await.unwrap();
        assert_eq!(diffs, [("src/keep".into(), DiffStatus::OnlyLocal)]);
    }

    #[tokio::test]
    async fn a_dir_link_or_special_file_on_one_side_hides_the_name_on_both() {
        let t = trees();
        std::fs::create_dir(t.l.join("target")).unwrap();
        std::os::unix::fs::symlink(t.l.join("target"), t.l.join("www")).unwrap();
        file(&t.r.join("www/index.html"), b"x", 1000);
        std::os::unix::fs::symlink(t.l.join("gone"), t.l.join("broken")).unwrap();
        file(&t.r.join("broken"), b"x", 1000);
        file(&t.l.join("target/f"), b"x", 1000);
        std::fs::create_dir_all(t.r.join("target")).unwrap();
        file(&t.r.join("target/f"), b"x", 1000);
        assert_eq!(run(&t, &[]).await.unwrap(), []);
    }

    #[tokio::test]
    async fn a_file_link_counts_as_its_target() {
        let t = trees();
        file(&t.l.join("real"), b"abc", 1000);
        std::os::unix::fs::symlink(t.l.join("real"), t.l.join("link")).unwrap();
        file(&t.r.join("link"), b"abc", 1000);
        assert_eq!(run(&t, &[]).await.unwrap(), [("real".into(), DiffStatus::OnlyLocal)]);
    }

    #[tokio::test]
    async fn lossy_and_hostile_names_are_left_out() {
        use std::os::unix::ffi::OsStrExt;
        let t = trees();
        file(&t.l.join(std::ffi::OsStr::from_bytes(b"bad\xff")), b"x", 1000);
        let remote = FaultySide::new();
        remote.add_fake_children(&["..", "a/b"]);
        let diffs = compare(&LocalSide, &*remote, &p(&t.l), &p(&t.r), &[], &CancellationToken::new()).await.unwrap();
        assert!(diffs.is_empty(), "{diffs:?}");
    }

    #[tokio::test]
    async fn an_unreadable_dir_aborts_instead_of_looking_empty() {
        use std::os::unix::fs::PermissionsExt;
        let t = trees();
        file(&t.l.join("d/f"), b"x", 1000);
        file(&t.r.join("d/f"), b"x", 1000);
        std::fs::set_permissions(t.r.join("d"), std::fs::Permissions::from_mode(0o000)).unwrap();
        let result = run(&t, &[]).await;
        std::fs::set_permissions(t.r.join("d"), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(result, Err(CoreError::Fs { .. })), "{result:?}");
    }

    #[tokio::test]
    async fn a_cancelled_compare_stops() {
        let t = trees();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = compare(&LocalSide, &LocalSide, &p(&t.l), &p(&t.r), &[], &cancel).await;
        assert_eq!(result, Err(CoreError::Cancelled));
    }
}
