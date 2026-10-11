//! LinaSCP core: storage, secrets, known_hosts, ssh-config, export and sessions.

pub mod error;
mod fsutil;
pub mod known_hosts;
pub mod model;
pub mod paths;

pub use error::{CoreError, NetworkKind, Result};
pub use known_hosts::{HostKeyInfo, HostKeyStatus, KnownHosts, fingerprints};
pub use model::*;
pub use paths::AppPaths;
