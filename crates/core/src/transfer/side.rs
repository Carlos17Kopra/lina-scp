//! What the transfer engine needs from either end of a transfer.

use async_trait::async_trait;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A regular file, or a symlink to one (then size, mtime and mode are the target's).
    File,
    Dir,
    /// A symlink to a directory: never followed.
    DirLink,
    BrokenLink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub size: u64,
    pub mtime: Option<i64>,
    /// `st_mode` (type and permission bits); `None` when the server did not send them.
    pub mode: Option<u32>,
    /// The path itself is a symlink (`kind` describes its target).
    pub link: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Child {
    pub name: String,
    /// The real name is not valid UTF-8; `name` must not be used for operations.
    pub lossy: bool,
}

#[async_trait]
pub trait Side: Send + Sync {
    /// `None` when nothing exists at `path`.
    async fn stat(&self, path: &str) -> Result<Option<Node>>;
    /// Raw names without `.` and `..`.
    async fn children(&self, path: &str) -> Result<Vec<Child>>;
    /// `Fs { AlreadyExists }` when something exists at `path`.
    async fn mkdir(&self, path: &str) -> Result<()>;
    async fn open_read(&self, path: &str) -> Result<Box<dyn Reader>>;
    /// `resume`: keep the existing content (no truncate). New files start with mode 0600.
    async fn open_write(&self, path: &str, resume: bool) -> Result<Box<dyn Writer>>;
    async fn set_mode(&self, path: &str, mode: u32) -> Result<()>;
    async fn set_mtime(&self, path: &str, mtime: i64) -> Result<()>;
    /// Atomically replaces `to` if it exists.
    async fn rename_over(&self, from: &str, to: &str) -> Result<()>;
    async fn remove(&self, path: &str) -> Result<()>;
    /// Canonical absolute path with every symlink resolved; fails when nothing exists there.
    async fn realpath(&self, path: &str) -> Result<String>;
}

#[async_trait]
pub trait Reader: Send + Sync {
    /// Up to `len` bytes at `offset`; fewer only at the end of the file.
    async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>>;
    async fn close(&self);
}

#[async_trait]
pub trait Writer: Send + Sync {
    async fn write_at(&self, offset: u64, data: Vec<u8>) -> Result<()>;
    async fn close(&self) -> Result<()>;
}
