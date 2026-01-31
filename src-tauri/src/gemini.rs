use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
use crate::CREATE_NO_WINDOW;

use crate::gemini_cli::gemini_cmd_path;

/// Open external terminal for Gemini authentication
#[tauri::command]
pub fn open_gemini_auth() -> Result<(), String> {
    let gemini_path = gemini_cmd_path();

    // Open new PowerShell window with gemini CLI
    Command::new("cmd")
        .args(["/c", "start", "powershell", "-NoExit", "-Command", &format!("& '{}'", gemini_path)])
        .spawn()
        .map_err(|e| format!("ターミナル起動エラー: {}", e))?;

    Ok(())
}

/// Check if Gemini CLI is authenticated
#[tauri::command]
pub fn check_gemini_auth() -> Result<bool, String> {
    let gemini_path = gemini_cmd_path();

    // Try running gemini with a simple command
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", &format!("& '{}' --version", gemini_path)]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd.output().map_err(|e| format!("確認エラー: {}", e))?;

    // If it succeeds, we're authenticated
    Ok(output.status.success())
}
