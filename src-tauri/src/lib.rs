use std::thread;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

mod analysis;
mod code_review;
mod events;
mod gemini;
mod guidelines;
mod history;
mod pdf_embed;
mod settings;
mod watcher;

#[cfg(target_os = "windows")]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x08000000;

pub use analysis::analyze_headless;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // Create system tray
            let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
            let show = MenuItem::with_id(app, "show", "表示", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
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
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Start watcher if folder is configured
            let settings = settings::load_settings();
            if let Some(folder) = settings.watch_folder.clone() {
                let app_handle = app.handle().clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(1));
                    let _ = watcher::start_watcher(app_handle, &folder);
                });
            }

            // Start code watcher if enabled and folder is configured
            if settings.code_review_enabled {
                if let Some(folder) = settings.code_watch_folder {
                    let app_handle = app.handle().clone();
                    thread::spawn(move || {
                        thread::sleep(Duration::from_secs(2));
                        let _ = code_review::start_code_watcher(app_handle, &folder);
                    });
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            analysis::analyze_pdfs,
            watcher::get_startup_file,
            watcher::get_watch_folder,
            watcher::set_watch_folder,
            watcher::stop_watching,
            gemini::open_gemini_auth,
            gemini::check_gemini_auth,
            settings::get_model,
            settings::set_model,
            history::get_all_history,
            pdf_embed::embed_pdf_result,
            pdf_embed::read_pdf_result,
            guidelines::generate_guidelines,
            code_review::get_code_watch_folder,
            code_review::is_code_review_enabled,
            code_review::set_code_watch_folder,
            code_review::set_code_review_enabled,
            code_review::stop_code_watching
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
