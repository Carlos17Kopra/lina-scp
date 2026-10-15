//! SFTP session over a russh connection (remote side of the commander).

use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::StatusCode;

use super::classify::network_error;
use crate::error::{CoreError, FsKind, NetworkKind};

/// SFTP status -> `Fs`; transport problems (timeout, closed channel) -> `Network`.
#[cfg_attr(not(test), allow(dead_code))] // used by SftpSession (next commit)
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
#[cfg_attr(not(test), allow(dead_code))]
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
