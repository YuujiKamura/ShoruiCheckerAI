use std::thread;
use std::time::Duration;


mod analysis;
mod code_review;
mod events;
mod error;
mod gemini;
mod gemini_cli;
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
    gui_shell::install_plugins(tauri::Builder::default())
        .setup(|app| {
            let _tray = gui_shell::setup_tray(&app.handle())?;

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
