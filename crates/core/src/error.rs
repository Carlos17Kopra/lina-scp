use serde::{Deserialize, Serialize};

/// Classification of network failures, used by the UI to pick the error screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum NetworkKind {
    Timeout,
    Refused,
    Dns,
    Other,
}

/// Classification of file-system failures (local or SFTP), used for UI texts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum FsKind {
    PermissionDenied,
    NotFound,
    AlreadyExists,
    NotEmpty,
    Other,
}

/// The single error type of `linascp-core`.
///
/// Serialized as an internally tagged object with the tag field `type`
/// (not `kind`, because the `Network` variant already has a field named `kind`):
/// `{"type":"network","kind":"timeout","message":"..."}`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CoreError {
    #[error("network error ({kind:?}): {message}")]
    Network { kind: NetworkKind, message: String },
    #[error("host key rejected: {message}")]
    HostKeyRejected { message: String },
    #[error("authentication failed: {message}")]
    Auth { rejected_methods: Vec<String>, message: String },
    #[error("keyring error: {message}")]
    Keyring { message: String },
    #[error("store error: {message}")]
    Store { message: String },
    #[error("configuration error: {message}")]
    Config { message: String },
    #[error("i/o error: {message}")]
    Io { message: String },
    #[error("file error ({kind:?}) at {path}: {message}")]
    Fs { kind: FsKind, path: String, message: String },
    #[error("cancelled")]
    Cancelled,
    /// A reconnect attempt would have needed user input (password, passphrase, 2FA, host key).
    #[error("input required")]
    InputRequired,
    #[error("internal error: {message}")]
    Internal { message: String },
}

pub type Result<T> = std::result::Result<T, CoreError>;

impl CoreError {
    pub fn store(message: impl Into<String>) -> Self {
        Self::Store { message: message.into() }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::Config { message: message.into() }
    }

    pub fn keyring(message: impl Into<String>) -> Self {
        Self::Keyring { message: message.into() }
    }

    pub fn fs(kind: FsKind, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Fs { kind, path: path.into(), message: message.into() }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal { message: message.into() }
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(err: rusqlite::Error) -> Self {
        Self::store(err.to_string())
    }
}

impl From<std::io::Error> for CoreError {
    fn from(err: std::io::Error) -> Self {
        Self::Io { message: err.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fs_error_serializes_with_kind_and_path() {
        let json = serde_json::to_value(CoreError::fs(FsKind::AlreadyExists, "/tmp/a", "exists")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "type": "fs", "kind": "alreadyExists", "path": "/tmp/a", "message": "exists" })
        );
    }

    #[test]
    fn network_error_serializes_with_type_tag_and_kind_field() {
        let err = CoreError::Network { kind: NetworkKind::Timeout, message: "timed out".into() };
        let value = serde_json::to_value(&err).unwrap();
        assert_eq!(value, json!({"type": "network", "kind": "timeout", "message": "timed out"}));
    }

    #[test]
    fn auth_error_uses_camel_case_fields() {
        let err = CoreError::Auth { rejected_methods: vec!["password".into()], message: "denied".into() };
        let value = serde_json::to_value(&err).unwrap();
        assert_eq!(value, json!({"type": "auth", "rejectedMethods": ["password"], "message": "denied"}));
    }

    #[test]
    fn unit_variant_and_roundtrip() {
        assert_eq!(serde_json::to_value(CoreError::Cancelled).unwrap(), json!({"type": "cancelled"}));
        let err = CoreError::HostKeyRejected { message: "no".into() };
        let back: CoreError = serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(back, err);
    }

    #[test]
    fn input_required_serializes_as_a_unit_type() {
        assert_eq!(
            serde_json::to_value(CoreError::InputRequired).unwrap(),
            serde_json::json!({ "type": "inputRequired" })
        );
    }

    #[test]
    fn io_and_sqlite_errors_convert() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        assert!(matches!(CoreError::from(io), CoreError::Io { .. }));
        let sql = rusqlite::Error::QueryReturnedNoRows;
        assert!(matches!(CoreError::from(sql), CoreError::Store { .. }));
    }

    #[test]
    fn display_contains_message() {
        assert_eq!(CoreError::config("bad").to_string(), "configuration error: bad");
    }
}
