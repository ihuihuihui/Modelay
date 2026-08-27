mod codex;
mod commands;
mod config;
mod error;
mod models;
mod paths;
mod platform;
mod secrets;
mod sessions;
mod storage;
mod usage;

use tauri::{Manager, WindowEvent};

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, _arguments, _cwd| {
                show_main_window(app);
            },
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_state,
            commands::get_widget_state,
            commands::save_channel,
            commands::delete_channel,
            commands::save_secret,
            commands::delete_secret,
            commands::list_models,
            commands::login_official,
            commands::switch_channel,
            commands::restart_chatgpt,
            commands::open_backup_folder,
            commands::get_usage,
            commands::set_widget_mode,
            commands::save_widget_position,
        ])
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            use tauri::menu::{Menu, MenuItem};
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

            let preferences = storage::initialize()?;
            let open_item = MenuItem::with_id(app, "open", "打开 Modelay", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 Modelay", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&open_item, &quit_item])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("Modelay")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            if let Some(window) = app.get_webview_window("usage") {
                platform::configure_usage_window(&window)?;
                if let Some(position) = preferences.widget_position {
                    window.set_position(tauri::Position::Physical(
                        tauri::PhysicalPosition::new(position.x, position.y),
                    ))?;
                }
                if preferences.dock_mode != "off" {
                    window.show()?;
                }
            }
            std::thread::spawn(storage::migrate_secrets_nonblocking);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Modelay");
    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            show_main_window(app);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app, event);
    });
}
