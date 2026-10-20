//! SFTP session over a russh connection (remote side of the commander).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::{StreamExt, stream};
use russh::client;
use russh_sftp::client::RawSftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{FileAttributes, OpenFlags, Packet, StatusCode};
use tokio::task::AbortHandle;

use super::SessionInfo;
use super::classify::network_error;
use super::handler::ClientHandler;
use crate::error::{CoreError, FsKind, NetworkKind, Result};
use crate::sftp::{Entry, EntryKind, LinkTarget, Listing, join};
use crate::transfer::{Child, Node, NodeKind, Reader, Side, Writer};
use crate::validate::validate_file_name;

/// In-flight `stat` requests while resolving symlinks of one listing.
const LINK_STAT_CONCURRENCY: usize = 32;

/// Called at most once when the connection of an SFTP or tunnel session is gone.
pub type LostFn = Arc<dyn Fn(CoreError) + Send + Sync>;

pub(crate) struct Lost {
    pub(crate) fired: AtomicBool,
    on_lost: LostFn,
}

impl Lost {
    pub(crate) fn new(on_lost: LostFn) -> Arc<Self> {
        Arc::new(Self { fired: AtomicBool::new(false), on_lost })
    }

    /// Reports `error` once; later calls (and calls after `close`) do nothing.
    pub(crate) fn fire(&self, error: CoreError) {
        if !self.fired.swap(true, Ordering::SeqCst) {
            (self.on_lost)(error);
        }
    }
}

pub struct SftpSession {
    raw: Arc<RawSftpSession>,
    posix_rename: bool,
    handle: Arc<client::Handle<ClientHandler>>,
    jump: Option<client::Handle<ClientHandler>>,
    info: SessionInfo,
    version: u32,
    lost: Arc<Lost>,
    watcher: AbortHandle,
}

fn lost_error() -> CoreError {
    network_error(NetworkKind::Other, "connection lost")
}

/// Fires `lost` once the connection closes; abort the returned handle on `close`.
pub(crate) fn watch_closed(handle: Arc<client::Handle<ClientHandler>>, lost: Arc<Lost>) -> AbortHandle {
    // russh has no awaitable "closed": poll. Keepalive (host.keepalive_s) closes the handle on a dead link.
    // ponytail: 1 s polling per session; a handler `disconnected` hook if this ever matters.
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if handle.is_closed() {
                lost.fire(lost_error());
                break;
            }
        }
    })
    .abort_handle()
}

impl SftpSession {
    pub(crate) fn start(
        raw: RawSftpSession,
        version: u32,
        posix_rename: bool,
        handle: client::Handle<ClientHandler>,
        jump: Option<client::Handle<ClientHandler>>,
        info: SessionInfo,
        on_lost: LostFn,
    ) -> Self {
        let handle = Arc::new(handle);
        let lost = Lost::new(on_lost);
        let watcher = watch_closed(handle.clone(), lost.clone());
        Self { raw: Arc::new(raw), posix_rename, handle, jump, info, version, lost, watcher }
    }

    pub fn info(&self) -> &SessionInfo {
        &self.info
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// The server offers `posix-rename@openssh.com` (atomic overwrite).
    pub fn posix_rename(&self) -> bool {
        self.posix_rename
    }

    fn check<T>(&self, result: std::result::Result<T, SftpError>, path: &str) -> Result<T> {
        checked(&self.lost, result, path)
    }

    fn ensure_alive(&self) -> Result<()> {
        if self.lost.fired.load(Ordering::SeqCst) { Err(lost_error()) } else { Ok(()) }
    }

    pub async fn home(&self) -> Result<String> {
        self.canonical(".").await
    }

    async fn canonical(&self, path: &str) -> Result<String> {
        self.ensure_alive()?;
        let name = self.check(self.raw.realpath(path).await, path)?;
        name.files.into_iter().next().map(|f| f.filename).ok_or_else(|| CoreError::internal("empty realpath reply"))
    }

    /// Target kind of a symlink at `path`; `None` for non-links and dangling links.
    async fn link_target(&self, path: Option<String>) -> Option<LinkTarget> {
        let attrs = self.raw.stat(path?).await.ok()?.attrs;
        Some(if attrs.is_dir() {
            LinkTarget::Dir
        } else if attrs.is_regular() {
            LinkTarget::File
        } else {
            LinkTarget::Other
        })
    }

    pub async fn list(&self, path: &str) -> Result<Listing> {
        let path = self.canonical(path).await?;
        let handle = self.check(self.raw.opendir(path.as_str()).await, &path)?.handle;
        let mut files = Vec::new();
        loop {
            match self.raw.readdir(handle.as_str()).await {
                Ok(name) => files.extend(name.files),
                Err(SftpError::Status(status)) if status.status_code == StatusCode::Eof => break,
                Err(e) => {
                    let _ = self.raw.close(handle.as_str()).await;
                    return self.check(Err(e), &path);
                }
            }
        }
        let _ = self.raw.close(handle.as_str()).await;
        let files: Vec<_> = files.into_iter().filter(|f| f.filename != "." && f.filename != "..").collect();
        // One `stat` per link, pipelined: sequential round trips make link-heavy dirs (/etc/alternatives) crawl.
        let links: Vec<Option<String>> =
            files.iter().map(|f| f.attrs.is_symlink().then(|| join(&path, &f.filename))).collect();
        let targets: Vec<Option<LinkTarget>> =
            stream::iter(links).map(|link| self.link_target(link)).buffered(LINK_STAT_CONCURRENCY).collect().await;
        let mut entries = Vec::with_capacity(files.len());
        for (file, target) in files.into_iter().zip(targets) {
            let attrs: &FileAttributes = &file.attrs;
            let kind = if attrs.is_symlink() {
                EntryKind::Symlink { target }
            } else if attrs.is_dir() {
                EntryKind::Dir
            } else if attrs.is_regular() {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            let owner = attrs
                .user
                .clone()
                .or_else(|| owner_from_longname(&file.longname))
                .or_else(|| attrs.uid.map(|uid| uid.to_string()));
            entries.push(Entry {
                kind,
                size: attrs.size.unwrap_or(0),
                mtime: attrs.mtime.map(i64::from),
                mode: attrs.permissions,
                owner,
                name_lossy: file.filename.contains('\u{FFFD}'),
                name: file.filename,
            });
        }
        Ok(Listing { path, entries })
    }

    async fn exists(&self, path: &str) -> bool {
        self.raw.lstat(path).await.is_ok()
    }

    pub async fn mkdir(&self, dir: &str, name: &str) -> Result<()> {
        validate_file_name(name)?;
        self.ensure_alive()?;
        let path = join(dir, name);
        match self.raw.mkdir(path.as_str(), FileAttributes::default()).await {
            Ok(_) => Ok(()),
            // OpenSSH answers a generic Failure for an existing name.
            Err(SftpError::Status(s)) if s.status_code == StatusCode::Failure && self.exists(&path).await => {
                Err(CoreError::fs(FsKind::AlreadyExists, &path, s.error_message))
            }
            Err(e) => self.check(Err(e), &path),
        }
    }

    /// SFTP v3 `rename` refuses an existing target; the `lstat` before gives a clear `AlreadyExists`.
    /// Creates an empty file (`CREATE | EXCLUDE`): an existing name is never touched.
    pub async fn create_file(&self, dir: &str, name: &str) -> Result<()> {
        validate_file_name(name)?;
        self.ensure_alive()?;
        let path = join(dir, name);
        let flags = OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE;
        let attrs = FileAttributes { permissions: Some(0o644), ..FileAttributes::default() };
        match self.raw.open(path.as_str(), flags, attrs).await {
            Ok(handle) => self.check(self.raw.close(handle.handle).await, &path).map(drop),
            // OpenSSH answers a generic Failure for an existing name.
            Err(SftpError::Status(s)) if s.status_code == StatusCode::Failure && self.exists(&path).await => {
                Err(CoreError::fs(FsKind::AlreadyExists, &path, s.error_message))
            }
            Err(e) => self.check(Err(e), &path),
        }
    }

    pub async fn rename(&self, dir: &str, from: &str, to: &str) -> Result<()> {
        validate_file_name(from)?;
        validate_file_name(to)?;
        self.ensure_alive()?;
        let (old, new) = (join(dir, from), join(dir, to));
        if self.exists(&new).await {
            return Err(CoreError::fs(FsKind::AlreadyExists, &new, "target exists"));
        }
        match self.raw.rename(old.as_str(), new.as_str()).await {
            Ok(_) => Ok(()),
            Err(SftpError::Status(s)) if s.status_code == StatusCode::Failure && self.exists(&new).await => {
                Err(CoreError::fs(FsKind::AlreadyExists, &new, s.error_message))
            }
            Err(e) => self.check(Err(e), &old),
        }
    }

    /// Permanent, recursive, bottom-up. Types come from `lstat`, so links are removed, never followed.
    pub async fn delete(&self, dir: &str, names: &[String]) -> Result<()> {
        for name in names {
            validate_file_name(name)?;
        }
        self.ensure_alive()?;
        // (path, children already pushed: remove the now empty dir)
        let mut stack: Vec<(String, bool)> = names.iter().map(|n| (join(dir, n), false)).rev().collect();
        while let Some((path, expanded)) = stack.pop() {
            if expanded {
                self.check(self.raw.rmdir(path.as_str()).await, &path)?;
                continue;
            }
            let attrs = self.check(self.raw.lstat(path.as_str()).await, &path)?.attrs;
            if attrs.is_dir() && !attrs.is_symlink() {
                let children = self.list_names(&path).await?;
                stack.push((path.clone(), true));
                stack.extend(children.into_iter().map(|c| (join(&path, &c), false)));
            } else {
                self.check(self.raw.remove(path.as_str()).await, &path)?;
            }
        }
        Ok(())
    }

    /// Raw child names of `path` (no canonicalisation, no link resolution).
    async fn list_names(&self, path: &str) -> Result<Vec<String>> {
        let handle = self.check(self.raw.opendir(path).await, path)?.handle;
        let mut names = Vec::new();
        loop {
            match self.raw.readdir(handle.as_str()).await {
                Ok(name) => names.extend(name.files.into_iter().map(|f| f.filename).filter(|n| n != "." && n != "..")),
                Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => break,
                Err(e) => {
                    let _ = self.raw.close(handle.as_str()).await;
                    return self.check(Err(e), path);
                }
            }
        }
        let _ = self.raw.close(handle.as_str()).await;
        Ok(names)
    }

    /// Ends the session without a `Lost` event.
    pub async fn close(&self) {
        self.lost.fired.store(true, Ordering::SeqCst);
        self.watcher.abort();
        let _ = self.raw.close_session();
        let _ = self.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        if let Some(jump) = &self.jump {
            let _ = jump.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        }
    }
}

#[async_trait::async_trait]
impl super::sftp_reconnect::Connection for SftpSession {
    fn info(&self) -> SessionInfo {
        self.info.clone()
    }
    async fn close(&self) {
        SftpSession::close(self).await;
    }
}

impl Drop for SftpSession {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}

/// Maps the error and reports a lost connection (once) on any transport failure: a request
/// timeout (frozen link without keepalive) or a dead SFTP channel leaves the SSH handle open,
/// but the session is unusable either way; reconnecting is cheap.
fn checked<T>(lost: &Lost, result: std::result::Result<T, SftpError>, path: &str) -> Result<T> {
    result.map_err(|e| {
        let error = map_sftp_error(e, path);
        if matches!(error, CoreError::Network { .. }) {
            lost.fire(error.clone());
        }
        error
    })
}

/// An open remote file (read or write) of a transfer.
struct RemoteFile {
    raw: Arc<RawSftpSession>,
    lost: Arc<Lost>,
    handle: String,
    path: String,
}

#[async_trait::async_trait]
impl Reader for RemoteFile {
    async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(len);
        // A server may answer short before the end: keep reading until `len` or EOF.
        while out.len() < len {
            let want = (len - out.len()) as u32;
            match self.raw.read(self.handle.as_str(), offset + out.len() as u64, want).await {
                Ok(data) if data.data.is_empty() => break,
                Ok(data) => out.extend_from_slice(&data.data),
                Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => break,
                Err(e) => return checked(&self.lost, Err(e), &self.path),
            }
        }
        Ok(out)
    }

    async fn close(&self) {
        let _ = self.raw.close(self.handle.as_str()).await;
    }
}

#[async_trait::async_trait]
impl Writer for RemoteFile {
    async fn write_at(&self, offset: u64, data: Vec<u8>) -> Result<()> {
        checked(&self.lost, self.raw.write(self.handle.as_str(), offset, data).await, &self.path).map(|_| ())
    }

    async fn close(&self) -> Result<()> {
        checked(&self.lost, self.raw.close(self.handle.as_str()).await, &self.path).map(|_| ())
    }
}

fn node_of(attrs: &FileAttributes, kind: NodeKind) -> Node {
    Node {
        kind,
        size: attrs.size.unwrap_or(0),
        mtime: attrs.mtime.map(i64::from),
        mode: attrs.permissions,
        link: false,
    }
}

/// SSH wire string: u32 length + bytes.
fn ssh_string(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

#[async_trait::async_trait]
impl Side for SftpSession {
    async fn stat(&self, path: &str) -> Result<Option<Node>> {
        self.ensure_alive()?;
        let attrs = match self.raw.lstat(path).await {
            Ok(a) => a.attrs,
            Err(SftpError::Status(s)) if s.status_code == StatusCode::NoSuchFile => return Ok(None),
            Err(e) => return self.check(Err(e), path),
        };
        if !attrs.is_symlink() {
            let kind = if attrs.is_dir() {
                NodeKind::Dir
            } else if attrs.is_regular() {
                NodeKind::File
            } else {
                NodeKind::Other
            };
            return Ok(Some(node_of(&attrs, kind)));
        }
        let target = match self.raw.stat(path).await {
            Ok(t) if t.attrs.is_dir() => node_of(&t.attrs, NodeKind::DirLink),
            Ok(t) if t.attrs.is_regular() => node_of(&t.attrs, NodeKind::File),
            Ok(t) => node_of(&t.attrs, NodeKind::Other),
            Err(SftpError::Status(_)) => node_of(&attrs, NodeKind::BrokenLink),
            Err(e) => return self.check(Err(e), path),
        };
        Ok(Some(Node { link: true, ..target }))
    }

    async fn children(&self, path: &str) -> Result<Vec<Child>> {
        self.ensure_alive()?;
        Ok(self.list_names(path).await?.into_iter().map(|name| Child { name, lossy: false }).collect())
    }

    async fn mkdir(&self, path: &str) -> Result<()> {
        self.ensure_alive()?;
        match self.raw.mkdir(path, FileAttributes::default()).await {
            Ok(_) => Ok(()),
            Err(SftpError::Status(s)) if s.status_code == StatusCode::Failure && self.exists(path).await => {
                Err(CoreError::fs(FsKind::AlreadyExists, path, s.error_message))
            }
            Err(e) => self.check(Err(e), path),
        }
    }

    async fn open_read(&self, path: &str) -> Result<Box<dyn Reader>> {
        self.ensure_alive()?;
        let handle = self.check(self.raw.open(path, OpenFlags::READ, FileAttributes::default()).await, path)?.handle;
        Ok(Box::new(RemoteFile { raw: self.raw.clone(), lost: self.lost.clone(), handle, path: path.to_string() }))
    }

    async fn open_write(&self, path: &str, resume: bool) -> Result<Box<dyn Writer>> {
        self.ensure_alive()?;
        let mut flags = OpenFlags::WRITE | OpenFlags::CREATE;
        if !resume {
            flags |= OpenFlags::TRUNCATE;
        }
        let attrs = FileAttributes { permissions: Some(0o600), ..FileAttributes::default() };
        let handle = self.check(self.raw.open(path, flags, attrs).await, path)?.handle;
        Ok(Box::new(RemoteFile { raw: self.raw.clone(), lost: self.lost.clone(), handle, path: path.to_string() }))
    }

    async fn set_mode(&self, path: &str, mode: u32) -> Result<()> {
        let attrs = FileAttributes { permissions: Some(mode), ..FileAttributes::default() };
        self.check(self.raw.setstat(path, attrs).await, path).map(|_| ())
    }

    async fn set_mtime(&self, path: &str, mtime: i64) -> Result<()> {
        // SFTP v3 sets atime and mtime together.
        let t = mtime.clamp(0, i64::from(u32::MAX)) as u32;
        let attrs = FileAttributes { atime: Some(t), mtime: Some(t), ..FileAttributes::default() };
        self.check(self.raw.setstat(path, attrs).await, path).map(|_| ())
    }

    async fn rename_over(&self, from: &str, to: &str) -> Result<()> {
        self.ensure_alive()?;
        if self.posix_rename {
            let mut data = Vec::new();
            ssh_string(&mut data, from);
            ssh_string(&mut data, to);
            return match self.raw.extended("posix-rename@openssh.com", data).await {
                Ok(Packet::Status(s)) if s.status_code == StatusCode::Ok => Ok(()),
                Ok(Packet::Status(s)) => Err(map_sftp_error(SftpError::Status(s), to)),
                Ok(_) => Err(CoreError::internal("unexpected reply to posix-rename")),
                Err(e) => self.check(Err(e), to),
            };
        }
        // ponytail: not atomic without the extension; a crash between both steps leaves only the part.
        match self.raw.remove(to).await {
            Ok(_) => {}
            Err(SftpError::Status(s)) if s.status_code == StatusCode::NoSuchFile => {}
            Err(e) => return self.check(Err(e), to),
        }
        self.check(self.raw.rename(from, to).await, from).map(|_| ())
    }

    async fn remove(&self, path: &str) -> Result<()> {
        self.check(self.raw.remove(path).await, path).map(|_| ())
    }

    async fn realpath(&self, path: &str) -> Result<String> {
        self.canonical(path).await
    }
}

/// SFTP status -> `Fs`; transport problems (timeout, closed channel) -> `Network`.
pub(crate) fn map_sftp_error(error: SftpError, path: &str) -> CoreError {
    match error {
        SftpError::Status(status) => {
            let kind = match status.status_code {
                StatusCode::NoSuchFile => FsKind::NotFound,
                StatusCode::PermissionDenied => FsKind::PermissionDenied,
                _ => FsKind::Other,
            };
            CoreError::fs(kind, path, status.error_message)
        }
        SftpError::Timeout => network_error(NetworkKind::Timeout, "SFTP request timed out"),
        SftpError::IO(message) | SftpError::UnexpectedBehavior(message) => network_error(NetworkKind::Other, message),
        other => CoreError::internal(other.to_string()),
    }
}

/// `ls -l` style longname: the owner is the third whitespace-separated column.
pub(crate) fn owner_from_longname(longname: &str) -> Option<String> {
    longname.split_whitespace().nth(2).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{FsKind, NetworkKind};
    use russh_sftp::client::error::Error as SftpError;
    use russh_sftp::protocol::{Status, StatusCode};

    fn status(code: StatusCode) -> SftpError {
        SftpError::Status(Status { id: 1, status_code: code, error_message: "m".into(), language_tag: "en".into() })
    }

    #[test]
    fn status_codes_map_to_fs_kinds() {
        let kind = |e| match map_sftp_error(e, "/p") {
            CoreError::Fs { kind, path, .. } => {
                assert_eq!(path, "/p");
                kind
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(kind(status(StatusCode::NoSuchFile)), FsKind::NotFound);
        assert_eq!(kind(status(StatusCode::PermissionDenied)), FsKind::PermissionDenied);
        assert_eq!(kind(status(StatusCode::Failure)), FsKind::Other);
    }

    #[test]
    fn transport_failures_are_network_errors() {
        assert!(matches!(
            map_sftp_error(SftpError::Timeout, "/p"),
            CoreError::Network { kind: NetworkKind::Timeout, .. }
        ));
        assert!(matches!(
            map_sftp_error(SftpError::UnexpectedBehavior("session closed".into()), "/p"),
            CoreError::Network { kind: NetworkKind::Other, .. }
        ));
        assert!(matches!(map_sftp_error(SftpError::IO("broken pipe".into()), "/p"), CoreError::Network { .. }));
    }

    #[test]
    fn owner_is_the_third_longname_column() {
        assert_eq!(
            owner_from_longname("-rw-r--r--    1 www-data www-data     1234 Oct  8 10:41 index.php").as_deref(),
            Some("www-data")
        );
        assert_eq!(owner_from_longname(""), None);
        assert_eq!(owner_from_longname("-rw-r--r-- 1"), None);
    }
}
