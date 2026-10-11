//! LinaSCP core: storage, secrets, known_hosts, ssh-config, export and sessions.

pub mod error;
pub mod model;
pub mod paths;
pub mod secrets;

pub use error::{CoreError, NetworkKind, Result};
pub use model::*;
pub use paths::AppPaths;
pub use secrets::{KeyringSecretStore, MemorySecretStore, SecretKind, SecretOwner, SecretStore};
