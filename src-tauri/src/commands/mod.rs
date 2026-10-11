pub mod hosts;
pub mod profiles;
pub mod secrets;
pub mod settings;

use linascp_core::error::{CoreError, Result};

/// Runs blocking work (SQLite bulk work, keyring D-Bus calls, file IO) off the async runtime.
pub(crate) async fn blocking<T, F>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CoreError::Internal { message: format!("background task failed: {e}") })?
}
