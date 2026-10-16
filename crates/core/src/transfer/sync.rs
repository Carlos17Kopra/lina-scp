//! Carrying out a synchronization planned from a compare (TP4b).

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::chmod::PathFailure;
use super::side::{NodeKind, Side};
use super::{BatchId, Direction, FileStamp, TransferQueue};
use crate::error::{CoreError, Result};
use crate::sftp::join;
use crate::validate::validate_file_name;

/// One planned entry: its path relative to the start directories, and the target as the compare saw it
/// (`None` for a directory, or when the target did not exist).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncItem {
    pub rel: String,
    pub expected: Option<FileStamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncDeleteReport {
    pub deleted: u32,
    /// Changed or gone since the compare: left alone.
    pub skipped: u32,
    pub failed: u32,
    pub first: Option<PathFailure>,
}

/// `rel` comes from the frontend: relative, `/`-separated, every segment a valid file name.
fn split_rel(rel: &str) -> Result<(&str, &str)> {
    for segment in rel.split('/') {
        validate_file_name(segment)?;
    }
    Ok(rel.rsplit_once('/').unwrap_or(("", rel)))
}

fn under(base: &str, rel: &str) -> String {
    if rel.is_empty() { base.to_string() } else { join(base, rel) }
}

/// Queues the transfers, one batch per target directory. `exclude` also applies inside transferred directories.
pub async fn enqueue_sync(
    queue: &TransferQueue,
    direction: Direction,
    local_dir: &str,
    remote_dir: &str,
    items: Vec<SyncItem>,
    exclude: &[String],
) -> Result<Vec<BatchId>> {
    let (src_base, dst_base) = match direction {
        Direction::Upload => (local_dir, remote_dir),
        Direction::Download => (remote_dir, local_dir),
    };
    let mut groups: BTreeMap<String, Vec<(String, Option<FileStamp>)>> = BTreeMap::new();
    for item in items {
        let (parent, name) = split_rel(&item.rel)?;
        groups.entry(parent.to_string()).or_default().push((name.to_string(), item.expected));
    }
    let mut batches = Vec::new();
    for (parent, entries) in groups {
        let target_dir = under(dst_base, &parent);
        let source_dir = under(src_base, &parent);
        let expected: HashMap<String, FileStamp> =
            entries.iter().filter_map(|(name, stamp)| stamp.map(|s| (join(&target_dir, name), s))).collect();
        let sources = entries.iter().map(|(name, _)| join(&source_dir, name)).collect();
        batches.push(queue.enqueue_expecting(direction, sources, target_dir, expected, exclude.to_vec()).await?);
    }
    Ok(batches)
}

/// Deletes each item below `base` that is still as the compare saw it: a file with the expected stamp, a
/// directory that is still one. `delete(dir, name)` does the removal (remote: permanent, local: trash).
pub async fn delete_unchanged<F>(
    side: &dyn Side,
    base: &str,
    items: Vec<SyncItem>,
    delete: impl Fn(String, String) -> F,
) -> Result<SyncDeleteReport>
where
    F: Future<Output = Result<()>>,
{
    let mut report = SyncDeleteReport { deleted: 0, skipped: 0, failed: 0, first: None };
    for item in items {
        let (parent, name) = split_rel(&item.rel)?;
        let path = join(base, &item.rel);
        let unchanged = match side.stat(&path).await {
            Ok(Some(node)) => match item.expected {
                Some(stamp) => node.kind == NodeKind::File && stamp == FileStamp { size: node.size, mtime: node.mtime },
                None => node.kind == NodeKind::Dir && !node.link,
            },
            Ok(None) => false,
            Err(error @ CoreError::Network { .. }) => return Err(error),
            Err(error) => {
                report.failed += 1;
                report.first.get_or_insert(PathFailure { path, error });
                continue;
            }
        };
        if !unchanged {
            report.skipped += 1;
            continue;
        }
        match delete(under(base, parent), name.to_string()).await {
            Ok(()) => report.deleted += 1,
            Err(error @ CoreError::Network { .. }) => return Err(error),
            Err(error) => {
                report.failed += 1;
                report.first.get_or_insert(PathFailure { path, error });
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::test_support::{Recorder, p};
    use crate::transfer::{JobState, LocalSide, TransferOptions};
    use std::os::unix::fs::MetadataExt;
    use std::sync::Arc;

    fn stamp(path: &std::path::Path) -> FileStamp {
        let meta = std::fs::metadata(path).unwrap();
        FileStamp { size: meta.len(), mtime: Some(meta.mtime()) }
    }

    fn item(rel: &str, expected: Option<FileStamp>) -> SyncItem {
        SyncItem { rel: rel.into(), expected }
    }

    #[test]
    fn relative_paths_are_checked_segment_by_segment() {
        assert_eq!(split_rel("a/b/c").unwrap(), ("a/b", "c"));
        assert_eq!(split_rel("c").unwrap(), ("", "c"));
        for bad in ["", "/etc", "a//b", "a/../b", "..", "a/", "a/\u{0}"] {
            assert!(split_rel(bad).is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn enqueue_sync_makes_one_batch_per_target_dir_and_overwrites_what_the_compare_saw() {
        let d = tempfile::tempdir().unwrap();
        let (l, r) = (d.path().join("l"), d.path().join("r"));
        for dir in [l.join("sub"), r.join("sub")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(l.join("top"), b"new").unwrap();
        std::fs::write(l.join("sub/f"), b"new").unwrap();
        std::fs::write(r.join("sub/f"), b"old").unwrap();
        let rec = Recorder::default();
        let options = TransferOptions { parallel: 2, upload_mode: None, bandwidth: None };
        let queue = TransferQueue::new(Arc::new(LocalSide), options, rec.emit());
        queue.set_remote(Some(Arc::new(LocalSide)));
        let items = vec![item("top", None), item("sub/f", Some(stamp(&r.join("sub/f"))))];
        let batches = enqueue_sync(&queue, Direction::Upload, &p(&l), &p(&r), items, &[]).await.unwrap();
        assert_eq!(batches.len(), 2);
        rec.wait_all_final().await;
        assert!(rec.jobs().values().all(|j| j.state == JobState::Done), "{:?}", rec.jobs());
        assert_eq!(std::fs::read(r.join("top")).unwrap(), b"new");
        assert_eq!(std::fs::read(r.join("sub/f")).unwrap(), b"new");
        let bad = enqueue_sync(&queue, Direction::Upload, &p(&l), &p(&r), vec![item("../x", None)], &[]).await;
        assert!(matches!(bad, Err(CoreError::Config { .. })));
    }

    #[tokio::test]
    async fn excluded_names_stay_behind_inside_a_transferred_dir() {
        let d = tempfile::tempdir().unwrap();
        let (l, r) = (d.path().join("l"), d.path().join("r"));
        std::fs::create_dir_all(l.join("newapp/node_modules/m")).unwrap();
        std::fs::create_dir_all(&r).unwrap();
        std::fs::write(l.join("newapp/a"), b"a").unwrap();
        std::fs::write(l.join("newapp/node_modules/m/x"), b"x").unwrap();
        let rec = Recorder::default();
        let options = TransferOptions { parallel: 2, upload_mode: None, bandwidth: None };
        let queue = TransferQueue::new(Arc::new(LocalSide), options, rec.emit());
        queue.set_remote(Some(Arc::new(LocalSide)));
        let exclude = vec!["node_modules".to_string()];
        enqueue_sync(&queue, Direction::Upload, &p(&l), &p(&r), vec![item("newapp", None)], &exclude).await.unwrap();
        rec.wait_all_final().await;
        assert!(r.join("newapp/a").exists());
        assert!(!r.join("newapp/node_modules").exists());
    }

    #[tokio::test]
    async fn delete_unchanged_skips_whatever_changed_since_the_compare() {
        let d = tempfile::tempdir().unwrap();
        let base = d.path();
        std::fs::write(base.join("same"), b"x").unwrap();
        std::fs::write(base.join("changed"), b"x").unwrap();
        std::fs::create_dir_all(base.join("dir/inner")).unwrap();
        std::fs::write(base.join("now_dir"), b"x").unwrap();
        let (same, changed, now_dir) =
            (stamp(&base.join("same")), stamp(&base.join("changed")), stamp(&base.join("now_dir")));
        std::fs::write(base.join("changed"), b"edited meanwhile").unwrap();
        std::fs::remove_file(base.join("now_dir")).unwrap();
        std::fs::create_dir(base.join("now_dir")).unwrap();
        let items = vec![
            item("same", Some(same)),
            item("changed", Some(changed)),
            item("dir", None),
            item("now_dir", Some(now_dir)),
            item("gone", None),
        ];
        let base_s = p(base);
        let report = delete_unchanged(&LocalSide, &base_s, items, |dir: String, name: String| async move {
            crate::sftp::local::delete(&dir, &[name])
        })
        .await
        .unwrap();
        assert_eq!((report.deleted, report.skipped, report.failed), (2, 3, 0));
        assert!(!base.join("same").exists() && !base.join("dir").exists());
        assert!(base.join("changed").exists() && base.join("now_dir").is_dir());
    }
}
