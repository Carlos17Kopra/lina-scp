//! Directory listings shared by the local side (`local`) and SFTP (`connect::SftpSession`).

pub mod local;

use serde::{Deserialize, Serialize};

/// What a symlink points to; `None` in `EntryKind::Symlink` = dangling or unreadable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum LinkTarget {
    File,
    Dir,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Dir,
    Symlink { target: Option<LinkTarget> },
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    /// Unix seconds.
    pub mtime: Option<i64>,
    /// `st_mode` as reported (type and permission bits).
    pub mode: Option<u32>,
    /// User name, else the numeric uid.
    pub owner: Option<String>,
    /// Local only: the real name is not valid UTF-8; `name` is a lossy rendering and must not be used for operations.
    pub name_lossy: bool,
}

/// One directory: canonical absolute `path` and its entries without `.` and `..`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub path: String,
    pub entries: Vec<Entry>,
}

/// POSIX join of an absolute directory and one validated name.
pub fn join(dir: &str, name: &str) -> String {
    format!("{}/{name}", dir.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_handles_root_and_plain_dirs() {
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("/var/www", "html"), "/var/www/html");
        assert_eq!(join("/var/www/", "html"), "/var/www/html");
    }

    #[test]
    fn entries_serialize_camel_case_with_tagged_kind() {
        let entry = Entry {
            name: "l".into(),
            kind: EntryKind::Symlink { target: Some(LinkTarget::Dir) },
            size: 3,
            mtime: Some(1),
            mode: Some(0o120777),
            owner: Some("root".into()),
            name_lossy: false,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], serde_json::json!({ "type": "symlink", "target": "dir" }));
        assert_eq!(json["nameLossy"], false);
    }
}
