//! Secret storage (passwords, key passphrases) behind the `SecretStore` trait.
//!
//! All implementations are synchronous. `KeyringSecretStore` talks to the Secret Service
//! over D-Bus and may block (e.g. while the keyring unlock prompt is shown), so async
//! callers must wrap calls in `tokio::task::spawn_blocking`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use keyring_core::api::CredentialStoreApi;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::model::Id;

/// Keyring service name for all LinaSCP entries.
pub const KEYRING_SERVICE: &str = "linascp";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SecretKind {
    Password,
    Passphrase,
}

/// Owner of a secret. Serialized adjacently tagged: `{"kind":"host","id":"<uuid>"}`
/// (internally tagged newtype variants holding a string are not representable in serde).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum SecretOwner {
    Host(Id),
    Profile(Id),
}

impl SecretOwner {
    pub fn id(&self) -> Id {
        match self {
            SecretOwner::Host(id) | SecretOwner::Profile(id) => *id,
        }
    }
}

pub trait SecretStore: Send + Sync {
    fn get(&self, owner: SecretOwner, kind: SecretKind) -> Result<Option<SecretString>>;
    fn set(&self, owner: SecretOwner, kind: SecretKind, value: &SecretString) -> Result<()>;
    /// Succeeds when the secret does not exist.
    fn delete(&self, owner: SecretOwner, kind: SecretKind) -> Result<()>;
}

/// Keyring account name: `"<host|profile>:<uuid>:<password|passphrase>"`.
pub fn account_name(owner: SecretOwner, kind: SecretKind) -> String {
    let (prefix, id) = match owner {
        SecretOwner::Host(id) => ("host", id),
        SecretOwner::Profile(id) => ("profile", id),
    };
    let kind = match kind {
        SecretKind::Password => "password",
        SecretKind::Passphrase => "passphrase",
    };
    format!("{prefix}:{id}:{kind}")
}

/// Inverse of [`account_name`]. Returns `None` for malformed names.
pub fn parse_account_name(name: &str) -> Option<(SecretOwner, SecretKind)> {
    let mut parts = name.split(':');
    let prefix = parts.next()?;
    let id = Id::parse_str(parts.next()?).ok()?;
    let kind = match parts.next()? {
        "password" => SecretKind::Password,
        "passphrase" => SecretKind::Passphrase,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    let owner = match prefix {
        "host" => SecretOwner::Host(id),
        "profile" => SecretOwner::Profile(id),
        _ => return None,
    };
    Some((owner, kind))
}

/// In-memory store for tests and as a fallback when no keyring is available.
#[derive(Default)]
pub struct MemorySecretStore {
    entries: Mutex<HashMap<String, String>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored secrets.
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, owner: SecretOwner, kind: SecretKind) -> Result<Option<SecretString>> {
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(entries.get(&account_name(owner, kind)).map(|v| SecretString::from(v.clone())))
    }

    fn set(&self, owner: SecretOwner, kind: SecretKind, value: &SecretString) -> Result<()> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.insert(account_name(owner, kind), value.expose_secret().to_string());
        Ok(())
    }

    fn delete(&self, owner: SecretOwner, kind: SecretKind) -> Result<()> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.remove(&account_name(owner, kind));
        Ok(())
    }
}

type DbusStore = dbus_secret_service_keyring_store::Store;

/// Secret Service (GNOME Keyring / KWallet) backed store via `keyring-core`.
/// The D-Bus connection is opened lazily on first use and retried after failures,
/// so construction never fails even when no keyring daemon is running.
#[derive(Default)]
pub struct KeyringSecretStore {
    store: Mutex<Option<Arc<DbusStore>>>,
}

impl KeyringSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn entry(&self, owner: SecretOwner, kind: SecretKind) -> Result<keyring_core::Entry> {
        let store = {
            let mut guard = self.store.lock().unwrap_or_else(PoisonError::into_inner);
            match guard.as_ref() {
                Some(store) => Arc::clone(store),
                None => {
                    let store = DbusStore::new().map_err(keyring_err)?;
                    *guard = Some(Arc::clone(&store));
                    store
                }
            }
        };
        store.build(KEYRING_SERVICE, &account_name(owner, kind), None).map_err(keyring_err)
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, owner: SecretOwner, kind: SecretKind) -> Result<Option<SecretString>> {
        match self.entry(owner, kind)?.get_password() {
            Ok(value) => Ok(Some(SecretString::from(value))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(err) => Err(keyring_err(err)),
        }
    }

    fn set(&self, owner: SecretOwner, kind: SecretKind, value: &SecretString) -> Result<()> {
        self.entry(owner, kind)?.set_password(value.expose_secret()).map_err(keyring_err)
    }

    fn delete(&self, owner: SecretOwner, kind: SecretKind) -> Result<()> {
        match self.entry(owner, kind)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(err) => Err(keyring_err(err)),
        }
    }
}

fn keyring_err(err: keyring_core::Error) -> CoreError {
    CoreError::keyring(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    #[test]
    fn account_name_format() {
        let id = Id::parse_str("6f9619ff-8b86-d011-b42d-00c04fc964ff").unwrap();
        assert_eq!(
            account_name(SecretOwner::Host(id), SecretKind::Password),
            "host:6f9619ff-8b86-d011-b42d-00c04fc964ff:password"
        );
        assert_eq!(
            account_name(SecretOwner::Profile(id), SecretKind::Passphrase),
            "profile:6f9619ff-8b86-d011-b42d-00c04fc964ff:passphrase"
        );
    }

    #[test]
    fn parse_account_name_roundtrip_and_rejects_garbage() {
        let id = Id::new_v4();
        for owner in [SecretOwner::Host(id), SecretOwner::Profile(id)] {
            for kind in [SecretKind::Password, SecretKind::Passphrase] {
                assert_eq!(parse_account_name(&account_name(owner, kind)), Some((owner, kind)));
            }
        }
        assert_eq!(parse_account_name("host:nope:password"), None);
        assert_eq!(parse_account_name(&format!("user:{id}:password")), None);
        assert_eq!(parse_account_name(&format!("host:{id}:pin")), None);
        assert_eq!(parse_account_name(&format!("host:{id}:password:x")), None);
    }

    #[test]
    fn secret_owner_serializes_adjacently_tagged() {
        let id = Id::nil();
        let v = serde_json::to_value(SecretOwner::Profile(id)).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "profile", "id": id.to_string()}));
        let back: SecretOwner = serde_json::from_value(v).unwrap();
        assert_eq!(back, SecretOwner::Profile(id));
    }

    #[test]
    fn memory_store_set_get_delete() {
        let store = MemorySecretStore::new();
        let owner = SecretOwner::Host(Id::new_v4());
        assert!(store.get(owner, SecretKind::Password).unwrap().is_none());
        store.set(owner, SecretKind::Password, &secret("hunter2")).unwrap();
        store.set(owner, SecretKind::Passphrase, &secret("pp")).unwrap();
        assert_eq!(store.get(owner, SecretKind::Password).unwrap().unwrap().expose_secret(), "hunter2");
        assert_eq!(store.len(), 2);
        store.delete(owner, SecretKind::Password).unwrap();
        store.delete(owner, SecretKind::Password).unwrap();
        assert!(store.get(owner, SecretKind::Password).unwrap().is_none());
        assert_eq!(store.get(owner, SecretKind::Passphrase).unwrap().unwrap().expose_secret(), "pp");
    }

    #[test]
    #[ignore = "needs a D-Bus session with an unlocked Secret Service keyring"]
    fn keyring_store_roundtrip() {
        let store = KeyringSecretStore::new();
        let owner = SecretOwner::Host(Id::new_v4());
        store.set(owner, SecretKind::Password, &secret("keyring-test")).unwrap();
        assert_eq!(store.get(owner, SecretKind::Password).unwrap().unwrap().expose_secret(), "keyring-test");
        store.delete(owner, SecretKind::Password).unwrap();
        assert!(store.get(owner, SecretKind::Password).unwrap().is_none());
        store.delete(owner, SecretKind::Password).unwrap();
    }
}
