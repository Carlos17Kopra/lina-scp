pub mod commands;
pub mod dto;
pub mod output;
pub mod state;
mod util;

use commands::{connect, hosts, profiles, secrets, sessions, settings};

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
            secrets::secret_set,
            secrets::secret_delete,
            settings::settings_get,
            settings::settings_set,
            connect::connect_ssh,
            connect::connect_answer,
            connect::connect_cancel,
            connect::connect_test,
            sessions::session_open_local,
            sessions::session_write,
            sessions::session_resize,
            sessions::session_ack,
            sessions::session_close,
        ])
        .dangerously_cast_bigints_to_number()
}

pub fn export_bindings(builder: &tauri_specta::Builder<tauri::Wry>) -> Result<(), specta_typescript::Error> {
    builder.export(specta_typescript::Typescript::default(), BINDINGS_PATH)
}

pub fn run() {
    let state = match state::AppState::init() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("LinaSCP: startup failed: {error}");
            std::process::exit(1);
        }
    };
    let builder = specta_builder();
    #[cfg(debug_assertions)]
    if let Err(error) = export_bindings(&builder) {
        eprintln!("LinaSCP: failed to export TypeScript bindings: {error}");
    }
    tauri::Builder::default()
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
        assert!(ts.contains("export type SecretOwner ="));
        assert!(ts.contains("sessionAck: (sessionId: string, bytes: number)"));
        assert!(ts.contains("output: Channel<ArrayBuffer>"));
        assert!(ts.contains("connectAnswer: (promptId: number, answer: PromptAnswer)"));
        assert!(ts.contains("export type SessionEventDto ="));
    }
}
