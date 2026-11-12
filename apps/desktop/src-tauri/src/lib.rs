mod commands;

use quark_core::{AppCore, CoreConfig};
use tauri::{Emitter, Manager};

pub struct AppState {
    pub core: AppCore,
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "quark_drive=info,quark_core=info,warn".into()),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| dirs::data_dir().unwrap_or_default().join("quark-drive"));
            let config = CoreConfig {
                data_dir,
                ..CoreConfig::default()
            };
            let core = tauri::async_runtime::block_on(AppCore::new(config))
                .map_err(|err| Box::<dyn std::error::Error>::from(err.to_string()))?;
            let state = AppState { core: core.clone() };
            app.manage(state);

            let mut events = core.subscribe();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = events.recv().await {
                    let _ = handle.emit("core://event", event);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_bootstrap,
            commands::start_qr_login,
            commands::poll_qr_login,
            commands::cancel_qr_login,
            commands::import_cookie,
            commands::logout,
            commands::refresh_account,
            commands::list_dir,
            commands::privacy_registered,
            commands::privacy_unlock,
            commands::privacy_list_dir,
            commands::privacy_lock,
            commands::privacy_move_files,
            commands::privacy_enqueue_upload,
            commands::privacy_enqueue_download,
            commands::cached_files,
            commands::mkdir,
            commands::rename,
            commands::move_files,
            commands::delete_files,
            commands::list_recycle,
            commands::restore_recycle,
            commands::delete_recycle,
            commands::clear_recycle,
            commands::enqueue_upload,
            commands::enqueue_download,
            commands::list_transfers,
            commands::pause_transfer,
            commands::resume_transfer,
            commands::cancel_transfer,
            commands::retry_transfer,
            commands::remove_transfer,
            commands::clear_finished_transfers,
            commands::get_settings,
            commands::update_settings,
            commands::pick_files,
            commands::pick_folder,
            commands::open_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
