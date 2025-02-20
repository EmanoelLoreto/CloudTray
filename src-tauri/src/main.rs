// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod drive;
mod config;

use tauri::{
    Manager, SystemTray, SystemTrayEvent, SystemTrayMenu
};

#[cfg(target_os = "macos")]
use tauri::ActivationPolicy;

#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

use window_shadows::set_shadow;

use tauri_plugin_positioner::{WindowExt, Position};

use std::sync::Mutex;
use tauri::State;

struct GoogleCredentials {
    client_id: Mutex<String>,
    client_secret: Mutex<String>,
}

#[tauri::command]
fn set_google_credentials(
    credentials: State<GoogleCredentials>,
    client_id: String,
    client_secret: String,
) {
    *credentials.client_id.lock().unwrap() = client_id;
    *credentials.client_secret.lock().unwrap() = client_secret;
}

fn main() {
    let google_credentials = GoogleCredentials {
        client_id: Mutex::new(String::new()),
        client_secret: Mutex::new(String::new()),
    };

    #[cfg(target_os = "macos")]
    let system_tray = SystemTray::new()
        .with_menu(SystemTrayMenu::new())
        .with_title("CloudTray")
        .with_icon_as_template(true);

    #[cfg(not(target_os = "macos"))]
    let system_tray = SystemTray::new()
        .with_menu(SystemTrayMenu::new())
        .with_tooltip("CloudTray");

    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_oauth::init())
        .system_tray(system_tray)
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(ActivationPolicy::Accessory);

            let _handle = app.handle();
            let window = app.get_window("tray-window").unwrap();
            let window_clone = window.clone();
            let window_clone_clone = window.clone();

            app.listen_global("quit", |_| {
                std::process::exit(0);
            });

            app.listen_global("close", move |_| {
                window_clone.hide().unwrap();
            });

            app.listen_global("open", move |_| {
                window_clone_clone.show().unwrap();
            });

            #[cfg(target_os = "macos")]
            {
                apply_vibrancy(
                    &window, 
                    NSVisualEffectMaterial::Menu, 
                    Some(NSVisualEffectState::Active), 
                    Some(6.0))
                    .expect("Unsupported platform! 'apply_vibrancy' is only supported on macOS");
            }

            #[cfg(target_os = "windows")]
            {
                window_vibrancy::clear_blur(&window).expect("Unsupported platform! 'apply_blur' is only supported on Windows");
            }

            #[cfg(any(windows, target_os = "macos"))]
            set_shadow(&window, true).unwrap();
            
            Ok(())
        })
        .on_system_tray_event(|app, event| {
            tauri_plugin_positioner::on_tray_event(app, &event);
            match event {
                SystemTrayEvent::LeftClick { position: _, size: _, .. } => {
                    let window = app.get_window("tray-window").unwrap();
                    #[cfg(target_os = "windows")]
                    {
                        if let Ok(()) = window.move_window(Position::TrayCenter) {
                            if let Ok(current_pos) = window.outer_position() {
                                let new_pos = tauri::PhysicalPosition {
                                    x: current_pos.x,
                                    y: current_pos.y - 10,
                                };
                                let _ = window.set_position(tauri::Position::Physical(new_pos));
                            }
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        let _ = window.move_window(Position::TrayCenter);
                    }
                    if window.is_visible().unwrap() {
                        window.hide().unwrap();
                    } else {
                        window.show().unwrap();
                        window.set_focus().unwrap();
                    }
                }
                SystemTrayEvent::RightClick { position: _, size: _, .. } => {
                    let window = app.get_window("tray-window").unwrap();
                    #[cfg(target_os = "windows")]
                    {
                        if let Ok(()) = window.move_window(Position::TrayCenter) {
                            if let Ok(current_pos) = window.outer_position() {
                                let new_pos = tauri::PhysicalPosition {
                                    x: current_pos.x,
                                    y: current_pos.y - 10,
                                };
                                let _ = window.set_position(tauri::Position::Physical(new_pos));
                            }
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        let _ = window.move_window(Position::TrayCenter);
                    }
                    if window.is_visible().unwrap() {
                        window.hide().unwrap();
                    } else {
                        window.show().unwrap();
                        window.set_focus().unwrap();
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_google_credentials,
            auth::start_oauth_server, 
            auth::exchange_auth_code, 
            auth::save_tokens,
            auth::get_tokens,
            auth::logout,
            drive::upload_file, 
            drive::get_or_create_app_folder,
            drive::upload_file_path,
            drive::list_recent_files,
            drive::delete_file,
            config::load_or_create_config,
            config::save_config,
        ])
        .manage(google_credentials)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}