use std::fs;
use std::path::Path;
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
use crate::CREATE_NO_WINDOW;

pub fn gemini_cmd_path() -> String {
    std::env::var("APPDATA")
        .map(|p| format!("{}\\npm\\gemini.cmd", p))
        .unwrap_or_else(|_| "gemini".to_string())
}

pub fn run_gemini_with_prompt(
    temp_dir: &Path,
    prompt: &str,
    model: &str,
    pdfs: Option<&[String]>,
) -> Result<String, String> {
    let prompt_file = temp_dir.join("prompt.txt");
    fs::write(&prompt_file, prompt).map_err(|e| e.to_string())?;

    let gemini_path = gemini_cmd_path();

    let ps_script = if let Some(pdfs) = pdfs {
        let pdf_array = pdfs
            .iter()
            .map(|f| format!("    '{}'", f.replace("'", "''")))
            .collect::<Vec<_>>()
            .join(",\n");
        format!(
            r#"$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8
$pdfs = @(
{}
)
Get-Content -Raw -Encoding UTF8 'prompt.txt' | & '{}' -m {} -o text $pdfs
"#,
            pdf_array,
            gemini_path.replace("'", "''"),
            model
        )
    } else {
        format!(
            r#"$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8
Get-Content -Raw -Encoding UTF8 'prompt.txt' | & '{}' -m {} -o text
"#,
            gemini_path.replace("'", "''"),
            model
        )
    };

    let script_file = temp_dir.join("run.ps1");
    fs::write(&script_file, &ps_script).map_err(|e| e.to_string())?;

    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        &script_file.to_string_lossy(),
    ])
    .current_dir(temp_dir);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(clean_gemini_output(&result))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn clean_gemini_output(output: &str) -> String {
    output
        .lines()
        .filter(|line| {
            !line.contains("Loaded cached credentials")
                && !line.contains("Hook registry initialized")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
