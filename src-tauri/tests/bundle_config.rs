//! Packaging configuration: deb + AppImage, runtime dependencies and the desktop entry.

use std::path::{Path, PathBuf};

use serde_json::Value;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn tauri_conf() -> Value {
    let raw = std::fs::read_to_string(manifest_dir().join("tauri.conf.json")).expect("read tauri.conf.json");
    serde_json::from_str(&raw).expect("tauri.conf.json is valid JSON")
}

fn strings(value: &Value) -> Vec<&str> {
    value.as_array().expect("array").iter().map(|item| item.as_str().expect("string")).collect()
}

#[test]
fn bundles_deb_and_appimage() {
    let conf = tauri_conf();
    assert_eq!(strings(&conf["bundle"]["targets"]), ["deb", "appimage"]);
    assert_eq!(conf["productName"], "LinaSCP");
}

#[test]
fn deb_declares_the_runtime_libraries() {
    let conf = tauri_conf();
    // The bundler adds libwebkit2gtk-4.1-0 and libgtk-3-0 itself; listing them here would put them in twice.
    assert_eq!(strings(&conf["bundle"]["linux"]["deb"]["depends"]), ["libdbus-1-3"]);
}

#[test]
fn desktop_entry_lists_network_and_remote_access() {
    let conf = tauri_conf();
    let template = conf["bundle"]["linux"]["deb"]["desktopTemplate"].as_str().expect("desktopTemplate is set");
    let content = std::fs::read_to_string(manifest_dir().join(Path::new(template))).expect("read desktop template");
    let lines: Vec<&str> = content.lines().collect();
    assert!(lines.contains(&"[Desktop Entry]"));
    assert!(lines.contains(&"Categories=Network;RemoteAccess;"));
    assert!(lines.contains(&"Exec={{exec}}"));
    assert!(lines.contains(&"Icon={{icon}}"));
    assert!(lines.contains(&"Terminal=false"));
}
