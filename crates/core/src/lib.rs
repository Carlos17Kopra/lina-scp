//! LinaSCP core: storage, secrets, known_hosts, ssh-config, export and sessions.

pub mod connect;
pub mod error;
mod fsutil;
pub mod known_hosts;
pub mod model;
pub mod paths;
pub mod secrets;
pub mod session;
pub mod sshconfig;
pub mod store;

pub use error::{CoreError, NetworkKind, Result};
pub use known_hosts::{HostKeyInfo, HostKeyStatus, KnownHosts, fingerprints};
pub use model::*;
pub use paths::AppPaths;
pub use secrets::{KeyringSecretStore, MemorySecretStore, SecretKind, SecretOwner, SecretStore};
pub use sshconfig::{INCLUDE_LINE, Mirror, import_ssh_config, merge_hosts};
pub use store::Store;
