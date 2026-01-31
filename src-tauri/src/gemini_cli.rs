use std::fs;
use std::path::Path;
use std::path::PathBuf;
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

pub struct GeminiRequest<'a> {
    pub prompt: &'a str,
    pub model: &'a str,
    pub files: Option<&'a [String]>,
    pub output_format: &'a str,
}

impl<'a> GeminiRequest<'a> {
    pub fn text(prompt: &'a str, model: &'a str) -> Self {
        Self {
            prompt,
            model,
            files: None,
            output_format: "text",
        }
    }

    pub fn text_with_files(prompt: &'a str, model: &'a str, files: &'a [String]) -> Self {
        Self {
            prompt,
            model,
            files: Some(files),
            output_format: "text",
        }
    }

    pub fn json(prompt: &'a str, model: &'a str) -> Self {
        Self {
            prompt,
            model,
            files: None,
            output_format: "json",
        }
    }
}

pub fn run_gemini(temp_dir: &Path, request: &GeminiRequest<'_>) -> Result<String, String> {
    let prompt_file = temp_dir.join("prompt.txt");
    fs::write(&prompt_file, request.prompt).map_err(|e| e.to_string())?;

    let gemini_path = gemini_cmd_path();
    let ps_script = build_ps_script(&gemini_path, request);

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
        let status = output
            .status
            .code()
            .map(|c| format!("exit code {}", c))
            .unwrap_or_else(|| "terminated".to_string());
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let detail = if stdout.trim().is_empty() {
            format!("{}: {}", status, stderr)
        } else {
            format!("{}: {}\n{}", status, stderr, stdout)
        };
        Err(detail.trim().to_string())
    }
}

pub fn run_gemini_in_temp(prefix: &str, request: &GeminiRequest<'_>) -> Result<String, String> {
    let temp_dir = create_temp_dir(prefix)?;
    let result = run_gemini(&temp_dir, request);
    cleanup_temp_dir(&temp_dir);
    result
}

pub fn run_gemini_with_prompt(
    temp_dir: &Path,
    prompt: &str,
    model: &str,
    pdfs: Option<&[String]>,
) -> Result<String, String> {
    let request = if let Some(files) = pdfs {
        GeminiRequest::text_with_files(prompt, model, files)
    } else {
        GeminiRequest::text(prompt, model)
    };
    run_gemini(temp_dir, &request)
}

pub fn create_temp_dir(prefix: &str) -> Result<PathBuf, String> {
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let temp_dir = home_dir.join(prefix);
    fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;
    Ok(temp_dir)
}

pub fn cleanup_temp_dir(temp_dir: &Path) {
    let _ = fs::remove_dir_all(temp_dir);
}

fn build_ps_script(gemini_path: &str, request: &GeminiRequest<'_>) -> String {
    let gemini_path = gemini_path.replace("'", "''");
    let model = request.model;
    let output_format = request.output_format;
    if let Some(files) = request.files {
        let file_array = files
            .iter()
            .map(|f| format!("    '{}'", f.replace("'", "''")))
            .collect::<Vec<_>>()
            .join(",\n");
        format!(
            r#"$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8
$files = @(
{}
)
Get-Content -Raw -Encoding UTF8 'prompt.txt' | & '{}' -m {} -o {} $files
"#,
            file_array, gemini_path, model, output_format
        )
    } else {
        format!(
            r#"$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8
Get-Content -Raw -Encoding UTF8 'prompt.txt' | & '{}' -m {} -o {}
"#,
            gemini_path, model, output_format
        )
    }
}

pub fn clean_gemini_output(output: &str) -> String {
    output
        .lines()
        .filter(|line| {
            !line.contains("Loaded cached credentials")
                && !line.contains("Hook registry initialized")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
