//! The local file system as a transfer side.

use std::fs::{File, OpenOptions, Permissions};
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use async_trait::async_trait;

use super::side::{Child, Node, NodeKind, Reader, Side, Writer};
use crate::error::{CoreError, Result};
use crate::sftp::local::io_error;

pub struct LocalSide;

async fn run<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| CoreError::internal(e.to_string()))?
}

fn node(meta: &std::fs::Metadata, kind: NodeKind) -> Node {
    Node { kind, size: meta.len(), mtime: Some(meta.mtime()), mode: meta.mode() }
}

fn stat_sync(path: &str) -> Result<Option<Node>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(e, path)),
    };
    let ft = meta.file_type();
    Ok(Some(if ft.is_symlink() {
        match std::fs::metadata(path) {
            Ok(target) if target.is_dir() => node(&target, NodeKind::DirLink),
            Ok(target) if target.is_file() => node(&target, NodeKind::File),
            Ok(target) => node(&target, NodeKind::Other),
            Err(_) => node(&meta, NodeKind::BrokenLink),
        }
    } else if ft.is_dir() {
        node(&meta, NodeKind::Dir)
    } else if ft.is_file() {
        node(&meta, NodeKind::File)
    } else {
        node(&meta, NodeKind::Other)
    }))
}

struct LocalFile {
    file: Arc<File>,
    path: String,
}

#[async_trait]
impl Reader for LocalFile {
    async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let (file, path) = (self.file.clone(), self.path.clone());
        run(move || {
            let mut buf = vec![0; len];
            let mut filled = 0;
            while filled < len {
                match file.read_at(&mut buf[filled..], offset + filled as u64) {
                    Ok(0) => break,
                    Ok(n) => filled += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(io_error(e, &path)),
                }
            }
            buf.truncate(filled);
            Ok(buf)
        })
        .await
    }

    async fn close(&self) {}
}

#[async_trait]
impl Writer for LocalFile {
    async fn write_at(&self, offset: u64, data: Vec<u8>) -> Result<()> {
        let (file, path) = (self.file.clone(), self.path.clone());
        run(move || file.write_all_at(&data, offset).map_err(|e| io_error(e, &path))).await
    }

    async fn close(&self) -> Result<()> {
        let (file, path) = (self.file.clone(), self.path.clone());
        run(move || file.sync_all().map_err(|e| io_error(e, &path))).await
    }
}

#[async_trait]
impl Side for LocalSide {
    async fn stat(&self, path: &str) -> Result<Option<Node>> {
        let path = path.to_string();
        run(move || stat_sync(&path)).await
    }

    async fn children(&self, path: &str) -> Result<Vec<Child>> {
        let path = path.to_string();
        run(move || {
            let mut out = Vec::new();
            for entry in std::fs::read_dir(&path).map_err(|e| io_error(e, &path))? {
                let name = entry.map_err(|e| io_error(e, &path))?.file_name();
                out.push(match name.to_str() {
                    Some(s) => Child { name: s.to_string(), lossy: false },
                    None => Child { name: name.to_string_lossy().into_owned(), lossy: true },
                });
            }
            Ok(out)
        })
        .await
    }

    async fn mkdir(&self, path: &str) -> Result<()> {
        let path = path.to_string();
        run(move || std::fs::create_dir(&path).map_err(|e| io_error(e, &path))).await
    }

    async fn open_read(&self, path: &str) -> Result<Box<dyn Reader>> {
        let path = path.to_string();
        run(move || {
            let file = File::open(&path).map_err(|e| io_error(e, &path))?;
            Ok(Box::new(LocalFile { file: Arc::new(file), path }) as Box<dyn Reader>)
        })
        .await
    }

    async fn open_write(&self, path: &str, resume: bool) -> Result<Box<dyn Writer>> {
        let path = path.to_string();
        run(move || {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(!resume)
                .mode(0o600)
                .open(&path)
                .map_err(|e| io_error(e, &path))?;
            Ok(Box::new(LocalFile { file: Arc::new(file), path }) as Box<dyn Writer>)
        })
        .await
    }

    async fn set_mode(&self, path: &str, mode: u32) -> Result<()> {
        let path = path.to_string();
        run(move || std::fs::set_permissions(&path, Permissions::from_mode(mode)).map_err(|e| io_error(e, &path))).await
    }

    async fn set_mtime(&self, path: &str, mtime: i64) -> Result<()> {
        let path = path.to_string();
        run(move || {
            let time = UNIX_EPOCH + Duration::from_secs(mtime.max(0) as u64);
            let file = OpenOptions::new().write(true).open(&path).map_err(|e| io_error(e, &path))?;
            file.set_modified(time).map_err(|e| io_error(e, &path))
        })
        .await
    }

    async fn rename_over(&self, from: &str, to: &str) -> Result<()> {
        let (from, to) = (from.to_string(), to.to_string());
        run(move || std::fs::rename(Path::new(&from), Path::new(&to)).map_err(|e| io_error(e, &to))).await
    }

    async fn remove(&self, path: &str) -> Result<()> {
        let path = path.to_string();
        run(move || std::fs::remove_file(&path).map_err(|e| io_error(e, &path))).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FsKind;
    use std::os::unix::fs::PermissionsExt;

    fn p(path: &std::path::Path) -> String {
        path.to_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn stat_classifies_files_dirs_and_links() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::write(root.join("f"), b"abc").unwrap();
        std::fs::create_dir(root.join("dir")).unwrap();
        std::os::unix::fs::symlink(root.join("f"), root.join("lf")).unwrap();
        std::os::unix::fs::symlink(root.join("dir"), root.join("ld")).unwrap();
        std::os::unix::fs::symlink(root.join("gone"), root.join("lb")).unwrap();
        let side = LocalSide;
        let f = side.stat(&p(&root.join("f"))).await.unwrap().unwrap();
        assert_eq!((f.kind, f.size), (NodeKind::File, 3));
        assert!(f.mtime.is_some());
        assert_eq!(side.stat(&p(&root.join("dir"))).await.unwrap().unwrap().kind, NodeKind::Dir);
        assert_eq!(side.stat(&p(&root.join("lf"))).await.unwrap().unwrap().kind, NodeKind::File);
        assert_eq!(side.stat(&p(&root.join("ld"))).await.unwrap().unwrap().kind, NodeKind::DirLink);
        assert_eq!(side.stat(&p(&root.join("lb"))).await.unwrap().unwrap().kind, NodeKind::BrokenLink);
        assert!(side.stat(&p(&root.join("none"))).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn write_read_mode_mtime_and_rename_over() {
        let d = tempfile::tempdir().unwrap();
        let part = p(&d.path().join("x.filepart"));
        let target = p(&d.path().join("x"));
        std::fs::write(&target, b"old").unwrap();
        let side = LocalSide;
        let w = side.open_write(&part, false).await.unwrap();
        w.write_at(3, b"def".to_vec()).await.unwrap();
        w.write_at(0, b"abc".to_vec()).await.unwrap();
        w.close().await.unwrap();
        assert_eq!(std::fs::metadata(&part).unwrap().permissions().mode() & 0o777, 0o600, "parts start private");
        let w = side.open_write(&part, true).await.unwrap();
        w.write_at(6, b"g".to_vec()).await.unwrap();
        w.close().await.unwrap();
        let r = side.open_read(&part).await.unwrap();
        assert_eq!(r.read_at(2, 10).await.unwrap(), b"cdefg");
        assert_eq!(r.read_at(7, 10).await.unwrap(), b"");
        r.close().await;
        side.set_mode(&part, 0o640).await.unwrap();
        side.set_mtime(&part, 1_000_000).await.unwrap();
        side.rename_over(&part, &target).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"abcdefg");
        let node = side.stat(&target).await.unwrap().unwrap();
        assert_eq!((node.mode & 0o777, node.mtime), (0o640, Some(1_000_000)));
        assert!(side.stat(&part).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn mkdir_children_remove_and_errors() {
        let d = tempfile::tempdir().unwrap();
        let side = LocalSide;
        let dir = p(&d.path().join("a"));
        side.mkdir(&dir).await.unwrap();
        assert!(matches!(side.mkdir(&dir).await, Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
        std::fs::write(d.path().join("a/f"), b"").unwrap();
        let bad = std::ffi::OsStr::from_bytes(b"bad\xff");
        std::fs::write(d.path().join("a").join(bad), b"").unwrap();
        let mut children = side.children(&dir).await.unwrap();
        children.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(children.len(), 2);
        assert!(children.iter().any(|c| c.name == "f" && !c.lossy));
        assert!(children.iter().any(|c| c.lossy));
        side.remove(&p(&d.path().join("a/f"))).await.unwrap();
        assert!(matches!(
            side.open_read(&p(&d.path().join("a/f"))).await,
            Err(CoreError::Fs { kind: FsKind::NotFound, .. })
        ));
    }

    use std::os::unix::ffi::OsStrExt;
}
