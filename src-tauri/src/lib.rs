pub mod commands;
pub mod dto;
pub mod editor;
pub mod output;
pub mod startup;
pub mod state;
mod util;

use commands::{
    bookmarks, connect, hosts, profiles, secrets, sessions, settings, sftp, sftp_edit, sftp_transfers, snippets,
    transfer,
};

/// Where the generated TypeScript bindings live (committed to git).
pub const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src/ipc/bindings.ts");

/// All IPC commands with their specta type information.
pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            hosts::hosts_list,
            hosts::host_save,
            hosts::host_delete,
            hosts::groups_list,
            hosts::group_save,
            hosts::group_delete,
            profiles::profiles_list,
            profiles::profile_save,
            profiles::profile_delete,
            sftp_edit::edit_open,
            sftp_edit::local_edit,
            sftp_edit::open_with_choose,
            sftp_edit::edit_resolve,
            sftp_edit::edit_reopen,
            sftp_edit::edit_stop,
            bookmarks::bookmarks_list,
            bookmarks::bookmark_add,
            bookmarks::bookmark_remove,
            snippets::snippets_list,
            snippets::snippet_save,
            snippets::snippet_delete,
            secrets::secret_set,
            secrets::secret_delete,
            settings::settings_get,
            settings::settings_set,
            connect::connect_ssh,
            connect::connect_answer,
            connect::connect_cancel,
            connect::connect_test,
            sftp::connect_sftp,
            sftp::sftp_list,
            sftp::sftp_mkdir,
            sftp::sftp_create_file,
            sftp::sftp_rename,
            sftp::sftp_delete,
            sftp::sftp_close,
            sftp::local_list,
            sftp::local_home,
            sftp::local_mkdir,
            sftp::local_create_file,
            sftp::local_rename,
            sftp::local_delete,
            sftp::local_trash,
            sftp_transfers::transfer_enqueue,
            sftp_transfers::transfer_resolve,
            sftp_transfers::transfer_cancel,
            sftp_transfers::transfer_retry,
            sftp_transfers::transfer_clear,
            sftp_transfers::sftp_reconnect_now,
            sftp_transfers::sftp_stop_reconnect,
            sessions::session_open_local,
            sessions::session_write,
            sessions::session_resize,
            sessions::session_ack,
            sessions::session_close,
            sessions::session_reconnect_now,
            sessions::session_reconnect_stop,
            transfer::export_file,
            transfer::import_preview,
            transfer::import_apply,
            transfer::ssh_config_reload,
        ])
        .dangerously_cast_bigints_to_number()
}

pub fn export_bindings(builder: &tauri_specta::Builder<tauri::Wry>) -> Result<(), specta_typescript::Error> {
    builder.export(specta_typescript::Typescript::default(), BINDINGS_PATH)
}

pub fn run() {
    // Opening the store first is deliberate: the compat-rendering flag must be known
    // before any thread exists and before WebKit is initialised.
    let state = match state::AppState::init() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("LinaSCP: startup failed: {error}");
            std::process::exit(1);
        }
    };
    startup::apply_webkit_workarounds(&state.store);
    let _log_guard = startup::init_logging(&state.paths.log_dir());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "LinaSCP starting");

    let builder = specta_builder();
    #[cfg(debug_assertions)]
    if let Err(error) = export_bindings(&builder) {
        tracing::warn!(%error, "failed to export TypeScript bindings");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(state)
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running LinaSCP");
}

#[cfg(test)]
mod tests {
    #[test]
    fn exports_bindings() {
        super::export_bindings(&super::specta_builder()).expect("export bindings");
        let ts = std::fs::read_to_string(super::BINDINGS_PATH).unwrap();
        assert!(ts.contains("hostsList: () =>"));
        assert!(ts.contains("export type CoreError ="));
        assert!(ts.contains("export type Settings ="));
        assert!(ts.contains(
            "transferEnqueue: (sessionId: string, direction: Direction, sources: string[], targetDir: string)"
        ));
        assert!(ts.contains("export type TransferEvent ="));
        assert!(ts.contains("autoReconnect: boolean"));
        assert!(ts.contains("export type SecretOwner ="));
        assert!(ts.contains("sessionAck: (sessionId: string, bytes: number)"));
        assert!(ts.contains("output: Channel<ArrayBuffer>"));
        assert!(ts.contains("connectAnswer: (promptId: number, answer: PromptAnswer)"));
        assert!(ts.contains("export type SessionEventDto ="));
        assert!(ts.contains(
            "exportFile: (path: string, includeSecrets: boolean, includeKeyFiles: boolean, passphrase: string | null)"
        ));
        assert!(ts.contains("sshConfigReload: () =>"));
    }
}
