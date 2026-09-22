pub mod commands;
pub mod ilink;
pub mod sender;
pub mod server;
pub mod state;
pub mod store;
pub mod wecom;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("微信 ClawBot Webhook")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Windows 上是 %APPDATA%\com.liuli.weixin-clawbot-webhook\
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let state = state::AppState::new(store::Store::new(dir));
            app.manage(state.clone());
            setup_tray(app)?;
            tauri::async_runtime::spawn(async move {
                if let Err(e) = commands::start_server(&state).await {
                    eprintln!("webhook 服务启动失败：{e}");
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // 点关闭按钮只隐藏到托盘，不退出
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
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
            commands::clear_logs,
            commands::send_test,
            wecom::commands::wecom_list_apps,
            wecom::commands::wecom_add_app,
            wecom::commands::wecom_update_app,
            wecom::commands::wecom_remove_app,
            wecom::commands::wecom_set_default,
            wecom::commands::wecom_send_test,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
