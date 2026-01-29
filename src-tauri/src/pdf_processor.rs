//! PDF text extraction module

use std::path::Path;

/// Extract text content from a PDF file
pub fn extract_text(file_path: &str) -> Result<String, String> {
    let path = Path::new(file_path);

    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    let bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| format!("Failed to extract text: {}", e))
}

/// Get basic info about a PDF file
pub fn get_pdf_info(file_path: &str) -> Result<PdfInfo, String> {
    let path = Path::new(file_path);

    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("Failed to get metadata: {}", e))?;

    let file_name = path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(PdfInfo {
        file_name,
        file_path: file_path.to_string(),
        size_bytes: metadata.len(),
    })
}

#[derive(Debug)]
pub struct PdfInfo {
    pub file_name: String,
    pub file_path: String,
    pub size_bytes: u64,
}
