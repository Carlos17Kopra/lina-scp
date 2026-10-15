//! SFTP session over a russh connection (remote side of the commander).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::{StreamExt, stream};
use russh::client;
use russh_sftp::client::RawSftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{FileAttributes, StatusCode};
use tokio::task::AbortHandle;

use super::SessionInfo;
use super::classify::network_error;
use super::handler::ClientHandler;
use crate::error::{CoreError, FsKind, NetworkKind, Result};
use crate::sftp::{Entry, EntryKind, LinkTarget, Listing, join};
use crate::validate::validate_file_name;

/// In-flight `stat` requests while resolving symlinks of one listing.
const LINK_STAT_CONCURRENCY: usize = 32;

/// Called at most once when the connection of an SFTP session is gone.
pub type LostFn = Arc<dyn Fn(CoreError) + Send + Sync>;

struct Lost {
    fired: AtomicBool,
    on_lost: LostFn,
}

impl Lost {
    /// Reports `error` once; later calls (and calls after `close`) do nothing.
    fn fire(&self, error: CoreError) {
        if !self.fired.swap(true, Ordering::SeqCst) {
            (self.on_lost)(error);
        }
    }
}

pub struct SftpSession {
    raw: RawSftpSession,
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

impl SftpSession {
    pub(crate) fn start(
        raw: RawSftpSession,
        version: u32,
        handle: client::Handle<ClientHandler>,
        jump: Option<client::Handle<ClientHandler>>,
        info: SessionInfo,
        on_lost: LostFn,
    ) -> Self {
        let handle = Arc::new(handle);
        let lost = Arc::new(Lost { fired: AtomicBool::new(false), on_lost });
        // russh has no awaitable "closed": poll. Keepalive (host.keepalive_s) closes the handle on a dead link.
        // ponytail: 1 s polling per session; a handler `disconnected` hook if this ever matters.
        let watcher = {
            let (handle, lost) = (handle.clone(), lost.clone());
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
        };
        Self { raw, handle, jump, info, version, lost, watcher }
    }

    pub fn info(&self) -> &SessionInfo {
        &self.info
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// Maps the error and reports a lost connection (once) on any transport failure: a request
    /// timeout (frozen link without keepalive) or a dead SFTP channel leaves the SSH handle open,
    /// but the session is unusable either way; reconnecting is cheap.
    fn check<T>(&self, result: std::result::Result<T, SftpError>, path: &str) -> Result<T> {
        result.map_err(|e| {
            let error = map_sftp_error(e, path);
            if matches!(error, CoreError::Network { .. }) {
                self.lost.fire(error.clone());
            }
            error
        })
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
                name: file.filename,
                name_lossy: false,
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

impl Drop for SftpSession {
    fn drop(&mut self) {
        self.watcher.abort();
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
