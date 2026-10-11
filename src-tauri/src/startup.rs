//! Process start-up: WebKit environment workarounds and logging.

use std::path::Path;

use linascp_core::store::Store;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder as RollingBuilder, RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::commands::settings::load_settings;

/// Applied only when the user enabled "compatibility rendering" (NVIDIA/Wayland DMABUF problems).
/// The 2026-10 spike showed WebGL runs fine without it and that it costs 10–13 % throughput.
pub const COMPAT_ENV: &[(&str, &str)] = &[("WEBKIT_DISABLE_DMABUF_RENDERER", "1")];

/// Daily log files kept in `~/.local/state/linascp/logs`.
pub const LOG_FILES_KEPT: usize = 7;

/// Which variables to set for the given switch; values the user already set are never overridden.
pub fn workaround_env(compat_rendering: bool, current: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    if !compat_rendering {
        return Vec::new();
    }
    COMPAT_ENV
        .iter()
        .filter(|&&(key, _)| current(key).is_none())
        .map(|&(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// Must run before the WebView is created and before any other thread is spawned.
pub fn apply_webkit_workarounds(store: &Store) {
    let compat = load_settings(store).map(|s| s.compat_rendering).unwrap_or(false);
    for (key, value) in workaround_env(compat, |key| std::env::var(key).ok()) {
        // SAFETY: called from `run()` before Tauri, logging or any other thread is started.
        unsafe { std::env::set_var(&key, &value) };
    }
}

/// `log_dir/linascp.YYYY-MM-DD.log`, rotated daily, `LOG_FILES_KEPT` files kept.
pub fn file_appender(log_dir: &Path) -> Result<RollingFileAppender, String> {
    std::fs::create_dir_all(log_dir).map_err(|e| e.to_string())?;
    RollingBuilder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("linascp")
        .filename_suffix("log")
        .max_log_files(LOG_FILES_KEPT)
        .build(log_dir)
        .map_err(|e| e.to_string())
}

/// Logs to stderr and to the rotating file. Filter via `LINASCP_LOG` (EnvFilter syntax).
/// The returned guard must live until the process ends (it flushes the file writer).
pub fn init_logging(log_dir: &Path) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_env("LINASCP_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,linascp_lib=debug,linascp_core=debug"));
    let (file_layer, guard) = match file_appender(log_dir) {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (Some(fmt::layer().with_ansi(false).with_writer(writer)), Some(guard))
        }
        Err(error) => {
            eprintln!("LinaSCP: file logging disabled: {error}");
            (None, None)
        }
    };
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(file_layer)
        .try_init();
    guard
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |key: &str| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn no_workaround_without_compat_rendering() {
        assert!(workaround_env(false, env_of(&[])).is_empty());
    }

    #[test]
    fn compat_rendering_disables_the_dmabuf_renderer() {
        assert_eq!(
            workaround_env(true, env_of(&[])),
            vec![("WEBKIT_DISABLE_DMABUF_RENDERER".to_string(), "1".to_string())]
        );
    }

    #[test]
    fn user_provided_values_are_never_overridden() {
        assert!(workaround_env(true, env_of(&[("WEBKIT_DISABLE_DMABUF_RENDERER", "0")])).is_empty());
    }

    #[test]
    fn file_appender_creates_the_log_directory_and_a_dated_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("state").join("logs");
        let appender = file_appender(&log_dir).unwrap();
        drop(appender);
        let names: Vec<String> = std::fs::read_dir(&log_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|n| n.starts_with("linascp.") && n.ends_with(".log")), "{names:?}");
    }
}
