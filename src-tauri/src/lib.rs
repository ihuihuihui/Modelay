mod codex;
mod commands;
mod config;
mod error;
mod handoff;
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
            commands::get_thread_health,
            commands::create_thread_handoff,
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
            let tray_icon = tauri::image::Image::new_owned(
                include_bytes!("../icons/tray-white.rgba").to_vec(),
                32,
                32,
            );
            let tray = TrayIconBuilder::new()
                .tooltip("Modelay")
                .icon(tray_icon)
                // macOS treats template images as monochrome glyphs and applies the
                // correct menu-bar tint. The bundled asset is already a transparent
                // white star, so Windows/Linux keep the same minimal appearance.
                .icon_as_template(cfg!(target_os = "macos"))
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_main_window(app),
                    "quit" => {
                        codex::reset_rpc();
                        app.exit(0);
                    }
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
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Modelay");
    app.run(|app, event| {
        if let tauri::RunEvent::Exit = event {
            codex::reset_rpc();
        }
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } = event
        {
            show_main_window(app);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app, event);
    });
}
