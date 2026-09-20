pub mod commands;
pub mod ilink;
pub mod sender;
pub mod server;
pub mod state;
pub mod store;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Windows 上是 %APPDATA%\com.liuli.weixin-clawbot-webhook\
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let state = state::AppState::new(store::Store::new(dir));
            app.manage(state.clone());
            tauri::async_runtime::spawn(async move {
                if let Err(e) = commands::start_server(&state).await {
                    eprintln!("webhook 服务启动失败：{e}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::login_start,
            commands::login_status,
            commands::list_accounts,
            commands::rename_account,
            commands::remove_account,
            commands::set_default_account,
            commands::get_config,
            commands::set_port,
            commands::reset_api_key,
            commands::server_start,
            commands::server_stop,
            commands::list_logs,
            commands::send_test,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
