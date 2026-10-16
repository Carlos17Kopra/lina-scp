//! Opens files in the user's editor for the SFTP "Bearbeiten" action.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use gio::prelude::{AppInfoExt, FileExt};
use linascp_core::edit::Launcher;
use linascp_core::error::{CoreError, Result};
use linascp_core::store::{EDITOR_COMMAND, Store};

/// Program and arguments for the "Editor" setting, with the file appended.
// ponytail: split on whitespace, no quoting; enough for `code -w` or `kate`.
pub(crate) fn editor_argv(command: &str, path: &Path) -> Vec<OsString> {
    let mut argv: Vec<OsString> = command.split_whitespace().map(OsString::from).collect();
    argv.push(path.as_os_str().to_owned());
    argv
}

/// The "Editor" setting, read on every open; unset = the default application for `text/plain`
/// (never the file's own type: "Bearbeiten" must not run a program or open an image viewer).
pub struct SystemLauncher {
    store: Arc<Store>,
}

impl SystemLauncher {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }
}

impl Launcher for SystemLauncher {
    fn open(&self, path: &Path) -> Result<()> {
        let command = self.store.get_setting(EDITOR_COMMAND)?.filter(|c| !c.trim().is_empty());
        match command {
            Some(command) => {
                let argv = editor_argv(&command, path);
                let (program, args) = argv.split_first().ok_or_else(|| CoreError::config("empty editor command"))?;
                Command::new(program)
                    .args(args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| CoreError::config(format!("cannot start the editor: {e}")))?;
                Ok(())
            }
            None => {
                let app = gio::AppInfo::default_for_type("text/plain", false)
                    .ok_or_else(|| CoreError::config("no text editor found"))?;
                let uri = gio::File::for_path(path).uri();
                app.launch_uris(&[uri.as_str()], None::<&gio::AppLaunchContext>)
                    .map_err(|e| CoreError::config(format!("cannot start the editor: {e}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;

    #[test]
    fn editor_argv_appends_the_file() {
        let argv: Vec<OsString> = editor_argv("code  -w", Path::new("/t/a b.txt"));
        assert_eq!(argv, ["code", "-w", "/t/a b.txt"].map(OsString::from));
    }
}
