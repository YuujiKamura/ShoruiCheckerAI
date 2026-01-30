// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut headless = false;
    let mut pdf_path: Option<String> = None;

    for arg in args.iter().skip(1) {
        if arg == "--headless" || arg == "-h" {
            headless = true;
        } else if arg.to_lowercase().ends_with(".pdf") {
            pdf_path = Some(arg.clone());
        }
    }

    if headless {
        if let Some(path) = pdf_path {
            // ヘッドレスモード: GUIなしで解析して終了
            if let Err(e) = shoruichecker_lib::analyze_headless(&path) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        } else {
            eprintln!("Usage: shoruichecker --headless <file.pdf>");
            std::process::exit(1);
        }
    } else {
        // GUIモード
        if let Some(path) = pdf_path {
            std::env::set_var("ANALYZE_FILE", path);
        }
        shoruichecker_lib::run()
    }
}
