//! Opens files in the user's editor for the SFTP "Bearbeiten" action.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use gio::prelude::{AppInfoExt, FileExt};
use linascp_core::edit::Launcher;
use linascp_core::error::{CoreError, Result};
use linascp_core::store::{EDITOR_COMMAND, Store};

use crate::dto::{AppChoice, OpenApp, OpenWithApps};

/// Program and arguments for the "Editor" setting, with the file appended.
// ponytail: split on whitespace, no quoting; enough for `code -w` or `kate`.
pub(crate) fn editor_argv(command: &str, path: &Path) -> Vec<OsString> {
    let mut argv: Vec<OsString> = command.split_whitespace().map(OsString::from).collect();
    argv.push(path.as_os_str().to_owned());
    argv
}

fn content_type(name: &str) -> String {
    gio::content_type_guess(Some(name), &[]).0.to_string()
}

fn remembered_key(name: &str) -> String {
    format!("open_with.{}", content_type(name))
}

fn validate_app_id(id: &str) -> Result<()> {
    let stem = id.strip_suffix(".desktop").unwrap_or_default();
    let ok = !stem.is_empty()
        && stem.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        && !stem.starts_with('.');
    if ok { Ok(()) } else { Err(CoreError::config("not a desktop id")) }
}

/// The app for `how` and the file `name`; `None` = the editor. A remembered app is stored per
/// file type in LinaSCP's settings, never in the system's `mimeapps.list`.
pub(crate) fn resolve_app(store: &Store, name: &str, how: &OpenApp) -> Result<Option<String>> {
    match how {
        OpenApp::Editor => Ok(None),
        OpenApp::Default => Ok(store.get_setting(&remembered_key(name))?.filter(|id| !id.is_empty())),
        OpenApp::App { id, remember } => {
            validate_app_id(id)?;
            let key = remembered_key(name);
            if *remember {
                store.set_setting(&key, id)?;
            } else if store.get_setting(&key)?.as_deref() == Some(id) {
                store.set_setting(&key, "")?;
            }
            Ok(Some(id.clone()))
        }
    }
}

/// Apps for the file's type plus the text editors, for "Öffnen mit".
pub(crate) fn apps_for(store: &Store, name: &str) -> Result<OpenWithApps> {
    let mime = content_type(name);
    let mut apps: Vec<AppChoice> = Vec::new();
    for info in gio::AppInfo::all_for_type(&mime).into_iter().chain(gio::AppInfo::all_for_type("text/plain")) {
        let Some(id) = info.id().map(|id| id.to_string()) else { continue };
        if info.should_show() && validate_app_id(&id).is_ok() && !apps.iter().any(|a| a.id == id) {
            apps.push(AppChoice { id, name: info.display_name().to_string() });
        }
    }
    let remembered = resolve_app(store, name, &OpenApp::Default)?;
    Ok(OpenWithApps { type_name: gio::content_type_get_description(&mime).to_string(), apps, remembered })
}

fn launch(app: &gio::AppInfo, path: &Path) -> Result<()> {
    let uri = gio::File::for_path(path).uri();
    app.launch_uris(&[uri.as_str()], None::<&gio::AppLaunchContext>)
        .map_err(|e| CoreError::config(format!("cannot start the editor: {e}")))
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
    fn open(&self, path: &Path, app: Option<&str>) -> Result<()> {
        if let Some(id) = app {
            // Only installed apps: the id is looked up, never run as a command.
            let info = gio::AppInfo::all()
                .into_iter()
                .find(|a| a.id().is_some_and(|i| i == id))
                .ok_or_else(|| CoreError::config("the app is not installed"))?;
            return launch(&info, path);
        }
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
                launch(&app, path)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::OpenApp;
    use std::ffi::OsString;
    use std::path::Path;

    fn store() -> (tempfile::TempDir, Arc<Store>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&dir.path().join("t.db")).unwrap());
        (dir, store)
    }

    fn app(id: &str, remember: bool) -> OpenApp {
        OpenApp::App { id: id.into(), remember }
    }

    #[test]
    fn the_editor_ignores_a_remembered_app_and_default_uses_it() {
        let (_d, store) = store();
        assert_eq!(resolve_app(&store, "a.png", &OpenApp::Default).unwrap(), None);
        assert_eq!(
            resolve_app(&store, "a.png", &app("viewer.desktop", true)).unwrap().as_deref(),
            Some("viewer.desktop")
        );
        assert_eq!(resolve_app(&store, "b.png", &OpenApp::Default).unwrap().as_deref(), Some("viewer.desktop"));
        assert_eq!(resolve_app(&store, "b.png", &OpenApp::Editor).unwrap(), None);
        assert_eq!(resolve_app(&store, "c.txt", &OpenApp::Default).unwrap(), None, "per file type");
    }

    #[test]
    fn unticking_the_switch_forgets_only_the_remembered_app() {
        let (_d, store) = store();
        resolve_app(&store, "a.png", &app("viewer.desktop", true)).unwrap();
        assert_eq!(resolve_app(&store, "a.png", &app("gimp.desktop", false)).unwrap().as_deref(), Some("gimp.desktop"));
        assert_eq!(resolve_app(&store, "a.png", &OpenApp::Default).unwrap().as_deref(), Some("viewer.desktop"));
        resolve_app(&store, "a.png", &app("viewer.desktop", false)).unwrap();
        assert_eq!(resolve_app(&store, "a.png", &OpenApp::Default).unwrap(), None);
    }

    #[test]
    fn app_ids_must_be_desktop_ids() {
        let (_d, store) = store();
        for bad in ["", "../x.desktop", "/usr/bin/sh", "sh", "a b.desktop"] {
            assert!(matches!(resolve_app(&store, "a.png", &app(bad, true)), Err(CoreError::Config { .. })), "{bad}");
        }
        assert_eq!(resolve_app(&store, "a.png", &OpenApp::Default).unwrap(), None, "nothing stored");
    }

    #[test]
    fn editor_argv_appends_the_file() {
        let argv: Vec<OsString> = editor_argv("code  -w", Path::new("/t/a b.txt"));
        assert_eq!(argv, ["code", "-w", "/t/a b.txt"].map(OsString::from));
    }
}
