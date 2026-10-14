//! Error classification and log sanitizing for the connect pipeline.

use std::io;

use crate::error::{CoreError, NetworkKind};

/// Classify a socket-level error.
pub fn classify_io(err: &io::Error) -> NetworkKind {
    match err.kind() {
        io::ErrorKind::TimedOut => NetworkKind::Timeout,
        io::ErrorKind::ConnectionRefused => NetworkKind::Refused,
        _ => NetworkKind::Other,
    }
}

/// Any failure of name resolution is reported as DNS error.
pub fn classify_resolve_error(_err: &io::Error) -> NetworkKind {
    NetworkKind::Dns
}

/// Shorthand for `CoreError::Network`.
pub fn network_error(kind: NetworkKind, message: impl Into<String>) -> CoreError {
    CoreError::Network { kind, message: message.into() }
}

/// Map a russh transport error to a `CoreError`.
pub fn map_russh_error(err: russh::Error) -> CoreError {
    use russh::Error as E;
    let message = err.to_string();
    match err {
        E::IO(io_err) => network_error(classify_io(&io_err), io_err.to_string()),
        E::ConnectionTimeout | E::KeepaliveTimeout | E::InactivityTimeout | E::Elapsed(_) => {
            network_error(NetworkKind::Timeout, message)
        }
        E::UnknownKey => CoreError::HostKeyRejected { message },
        E::Disconnect | E::HUP | E::SendError | E::RecvError | E::ChannelOpenFailure(_) => {
            network_error(NetworkKind::Other, message)
        }
        _ => CoreError::Internal { message },
    }
}

const SECRET_KEYWORDS: [&str; 2] = ["password", "passphrase"];
const MASK: &str = "***";

/// Mask everything following a `password`/`passphrase` keyword
/// (case-insensitive), keeping the keyword and a `:`/`=`/space separator.
pub fn sanitize_log_line(line: &str) -> String {
    // ASCII lowercase keeps byte offsets identical to `line`.
    let lower = line.to_ascii_lowercase();
    let cut = SECRET_KEYWORDS.iter().filter_map(|kw| lower.find(kw).map(|pos| pos + kw.len())).min();
    let Some(mut end) = cut else {
        return line.to_string();
    };
    let bytes = line.as_bytes();
    while end < bytes.len() && matches!(bytes[end], b':' | b'=' | b' ' | b'\t') {
        end += 1;
    }
    if end >= line.len() {
        return line.to_string();
    }
    format!("{}{}", &line[..end], MASK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_io_kinds() {
        let t = io::Error::new(io::ErrorKind::TimedOut, "t");
        let r = io::Error::new(io::ErrorKind::ConnectionRefused, "r");
        let o = io::Error::new(io::ErrorKind::PermissionDenied, "o");
        assert_eq!(classify_io(&t), NetworkKind::Timeout);
        assert_eq!(classify_io(&r), NetworkKind::Refused);
        assert_eq!(classify_io(&o), NetworkKind::Other);
    }

    #[test]
    fn resolve_errors_are_dns() {
        let e = io::Error::other("failed to lookup address information");
        assert_eq!(classify_resolve_error(&e), NetworkKind::Dns);
    }

    #[test]
    fn network_error_builds_variant() {
        match network_error(NetworkKind::Refused, "nope") {
            CoreError::Network { kind, message } => {
                assert_eq!(kind, NetworkKind::Refused);
                assert_eq!(message, "nope");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn russh_errors_are_mapped() {
        let io_err = russh::Error::IO(io::Error::new(io::ErrorKind::ConnectionRefused, "x"));
        assert!(matches!(map_russh_error(io_err), CoreError::Network { kind: NetworkKind::Refused, .. }));
        assert!(matches!(
            map_russh_error(russh::Error::KeepaliveTimeout),
            CoreError::Network { kind: NetworkKind::Timeout, .. }
        ));
        assert!(matches!(map_russh_error(russh::Error::UnknownKey), CoreError::HostKeyRejected { .. }));
        assert!(matches!(
            map_russh_error(russh::Error::Disconnect),
            CoreError::Network { kind: NetworkKind::Other, .. }
        ));
        assert!(matches!(map_russh_error(russh::Error::Inconsistent), CoreError::Internal { .. }));
    }

    #[test]
    fn sanitize_masks_after_keywords() {
        assert_eq!(sanitize_log_line("password: hunter2"), "password: ***");
        assert_eq!(sanitize_log_line("Passphrase=abc def"), "Passphrase=***");
        assert_eq!(sanitize_log_line("debug1: sending PASSWORD secret"), "debug1: sending PASSWORD ***");
        assert_eq!(sanitize_log_line("x passphrase y password z"), "x passphrase ***");
    }

    #[test]
    fn sanitize_leaves_other_lines_alone() {
        assert_eq!(
            sanitize_log_line("debug1: Connecting to 10.0.0.1 port 22"),
            "debug1: Connecting to 10.0.0.1 port 22"
        );
        assert_eq!(sanitize_log_line("ends with password"), "ends with password");
        assert_eq!(sanitize_log_line("password:  "), "password:  ");
        assert_eq!(sanitize_log_line("Ünïcode password: ü"), "Ünïcode password: ***");
    }
}
