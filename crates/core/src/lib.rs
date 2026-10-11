//! LinaSCP core: storage, secrets, known_hosts, ssh-config, export and sessions.

pub mod error;
pub mod model;
pub mod paths;

pub use error::{CoreError, NetworkKind, Result};
pub use model::*;
pub use paths::AppPaths;
