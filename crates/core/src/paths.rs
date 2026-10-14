use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};

/// Expand a leading `~/` (or a bare `~`) against `home`; other paths are returned unchanged.
pub fn expand_tilde(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home.to_path_buf(),
        None => PathBuf::from(path),
    }
}

/// All filesystem locations used by LinaSCP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub ssh_dir: PathBuf,
    pub home: PathBuf,
}

impl AppPaths {
    /// Resolves the real locations from `HOME`, `XDG_DATA_HOME` and `XDG_STATE_HOME`.
    pub fn from_env() -> Result<Self> {
        Self::resolve(std::env::var_os("HOME"), std::env::var_os("XDG_DATA_HOME"), std::env::var_os("XDG_STATE_HOME"))
    }

    /// Test layout: everything lives below `root`.
    pub fn for_root(root: &Path) -> Self {
        let home = root.join("home");
        Self { data_dir: root.join("data"), state_dir: root.join("state"), ssh_dir: home.join(".ssh"), home }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("linascp.db")
    }

    pub fn known_hosts(&self) -> PathBuf {
        self.ssh_dir.join("known_hosts")
    }

    pub fn ssh_config(&self) -> PathBuf {
        self.ssh_dir.join("config")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }

    pub fn imported_keys_dir(&self) -> PathBuf {
        self.ssh_dir.join("linascp")
    }

    /// Creates `data_dir` and `log_dir` if they do not exist yet.
    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(self.log_dir())?;
        Ok(())
    }

    fn resolve(home: Option<OsString>, xdg_data: Option<OsString>, xdg_state: Option<OsString>) -> Result<Self> {
        let home =
            home.filter(|h| !h.is_empty()).map(PathBuf::from).ok_or_else(|| CoreError::config("HOME is not set"))?;
        let data_base = absolute_or(xdg_data, home.join(".local/share"));
        let state_base = absolute_or(xdg_state, home.join(".local/state"));
        Ok(Self {
            data_dir: data_base.join("linascp"),
            state_dir: state_base.join("linascp"),
            ssh_dir: home.join(".ssh"),
            home,
        })
    }
}

/// XDG base directory spec: relative or empty values must be ignored.
fn absolute_or(value: Option<OsString>, fallback: PathBuf) -> PathBuf {
    value.map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_uses_home() {
        let home = Path::new("/home/u");
        assert_eq!(expand_tilde("~/.ssh/id", home), PathBuf::from("/home/u/.ssh/id"));
        assert_eq!(expand_tilde("~", home), PathBuf::from("/home/u"));
        assert_eq!(expand_tilde("/etc/key", home), PathBuf::from("/etc/key"));
        assert_eq!(expand_tilde("~other/key", home), PathBuf::from("~other/key"));
    }

    #[test]
    fn for_root_places_everything_below_root() {
        let p = AppPaths::for_root(Path::new("/tmp/x"));
        assert_eq!(p.home, PathBuf::from("/tmp/x/home"));
        assert_eq!(p.db_path(), PathBuf::from("/tmp/x/data/linascp.db"));
        assert_eq!(p.known_hosts(), PathBuf::from("/tmp/x/home/.ssh/known_hosts"));
        assert_eq!(p.ssh_config(), PathBuf::from("/tmp/x/home/.ssh/config"));
        assert_eq!(p.log_dir(), PathBuf::from("/tmp/x/state/logs"));
        assert_eq!(p.imported_keys_dir(), PathBuf::from("/tmp/x/home/.ssh/linascp"));
    }

    #[test]
    fn resolve_uses_xdg_defaults() {
        let p = AppPaths::resolve(Some("/home/u".into()), None, None).unwrap();
        assert_eq!(p.data_dir, PathBuf::from("/home/u/.local/share/linascp"));
        assert_eq!(p.state_dir, PathBuf::from("/home/u/.local/state/linascp"));
        assert_eq!(p.ssh_dir, PathBuf::from("/home/u/.ssh"));
    }

    #[test]
    fn resolve_honours_absolute_xdg_vars_and_ignores_relative_ones() {
        let p = AppPaths::resolve(Some("/home/u".into()), Some("/xdg/data".into()), Some("rel".into())).unwrap();
        assert_eq!(p.data_dir, PathBuf::from("/xdg/data/linascp"));
        assert_eq!(p.state_dir, PathBuf::from("/home/u/.local/state/linascp"));
    }

    #[test]
    fn resolve_without_home_is_config_error() {
        let err = AppPaths::resolve(None, None, None).unwrap_err();
        assert!(matches!(err, CoreError::Config { .. }));
    }

    #[test]
    fn ensure_dirs_creates_data_and_log_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let p = AppPaths::for_root(tmp.path());
        p.ensure_dirs().unwrap();
        assert!(p.data_dir.is_dir());
        assert!(p.log_dir().is_dir());
    }
}
