//! PDF embedding and Base64 encoding/decoding utilities
//!
//! This module provides functionality to embed analysis results and custom instructions
//! into PDF metadata, as well as read them back.

use serde::{Serialize, Deserialize};
use lopdf::{Document, Object, StringFormat};

/// PDF embedded data structure
#[derive(Clone, Serialize, Deserialize)]
pub struct PdfEmbeddedData {
    pub result: String,
    pub instruction: Option<String>,
    pub date: String,
}

/// Embed analysis result and custom instruction into PDF metadata
pub fn embed_result_in_pdf_with_instruction(pdf_path: &str, result: &str, custom_instruction: &str) -> Result<(), String> {
    let mut doc = Document::load(pdf_path).map_err(|e| format!("PDF読み込みエラー: {}", e))?;

    // Get or create Info dictionary
    let info_id = if let Some(info_ref) = doc.trailer.get(b"Info").ok().and_then(|o| o.as_reference().ok()) {
        info_ref
    } else {
        // Create new Info dictionary
        let info_dict = lopdf::Dictionary::new();
        let info_id = doc.add_object(Object::Dictionary(info_dict));
        doc.trailer.set("Info", Object::Reference(info_id));
        info_id
    };

    // Add custom metadata
    if let Ok(Object::Dictionary(ref mut info)) = doc.get_object_mut(info_id) {
        // Store analysis result (base64 encoded to avoid encoding issues)
        let encoded = base64_encode(result);
        info.set("ShoruiCheckerResult", Object::String(encoded.into_bytes(), StringFormat::Literal));

        // Store custom instruction if provided
        if !custom_instruction.is_empty() {
            let encoded_instruction = base64_encode(custom_instruction);
            info.set("ShoruiCheckerInstruction", Object::String(encoded_instruction.into_bytes(), StringFormat::Literal));
        }

        // Store analysis timestamp
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        info.set("ShoruiCheckerDate", Object::String(timestamp.into_bytes(), StringFormat::Literal));

        // Store version
        info.set("ShoruiCheckerVersion", Object::String(b"1.0".to_vec(), StringFormat::Literal));
    }

    doc.save(pdf_path).map_err(|e| format!("PDF保存エラー: {}", e))?;
    Ok(())
}

/// Wrapper for backward compatibility (embeds result without custom instruction)
pub fn embed_result_in_pdf(pdf_path: &str, result: &str) -> Result<(), String> {
    embed_result_in_pdf_with_instruction(pdf_path, result, "")
}

/// Read embedded analysis result from PDF
/// Returns (result, date) tuple if found
pub fn read_result_from_pdf(pdf_path: &str) -> Option<(String, String)> {
    let data = read_embedded_data_from_pdf(pdf_path)?;
    Some((data.result, data.date))
}

/// Read all embedded data from PDF
pub fn read_embedded_data_from_pdf(pdf_path: &str) -> Option<PdfEmbeddedData> {
    let doc = Document::load(pdf_path).ok()?;

    let info_ref = doc.trailer.get(b"Info").ok()?.as_reference().ok()?;
    if let Ok(Object::Dictionary(info)) = doc.get_object(info_ref) {
        let result = info.get(b"ShoruiCheckerResult").ok()
            .and_then(|o| {
                if let Object::String(bytes, _) = o {
                    String::from_utf8(bytes.clone()).ok()
                        .and_then(|s| base64_decode(&s))
                } else {
                    None
                }
            })?;

        let instruction = info.get(b"ShoruiCheckerInstruction").ok()
            .and_then(|o| {
                if let Object::String(bytes, _) = o {
                    String::from_utf8(bytes.clone()).ok()
                        .and_then(|s| base64_decode(&s))
                } else {
                    None
                }
            });

        let date = info.get(b"ShoruiCheckerDate").ok()
            .and_then(|o| {
                if let Object::String(bytes, _) = o {
                    String::from_utf8(bytes.clone()).ok()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        return Some(PdfEmbeddedData { result, instruction, date });
    }

    None
}

/// Base64 encode a string
pub fn base64_encode(s: &str) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder = base64_writer(&mut buf);
        encoder.write_all(s.as_bytes()).unwrap();
    }
    String::from_utf8(buf).unwrap_or_default()
}

/// Create a Base64 writer
pub fn base64_writer(w: &mut Vec<u8>) -> impl std::io::Write + '_ {
    struct B64Writer<'a>(&'a mut Vec<u8>);
    impl<'a> std::io::Write for B64Writer<'a> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            for chunk in buf.chunks(3) {
                let b0 = chunk[0] as usize;
                let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
                let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
                self.0.push(ALPHABET[b0 >> 2]);
                self.0.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)]);
                if chunk.len() > 1 {
                    self.0.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)]);
                } else {
                    self.0.push(b'=');
                }
                if chunk.len() > 2 {
                    self.0.push(ALPHABET[b2 & 0x3f]);
                } else {
                    self.0.push(b'=');
                }
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    B64Writer(w)
}

/// Base64 decode a string
pub fn base64_decode(s: &str) -> Option<String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::new();
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'=').collect();

    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 { break; }
        let b0 = ALPHABET.iter().position(|&c| c == chunk[0])? as u8;
        let b1 = ALPHABET.iter().position(|&c| c == chunk[1])? as u8;
        result.push((b0 << 2) | (b1 >> 4));
        if chunk.len() > 2 && chunk[2] != b'=' {
            let b2 = ALPHABET.iter().position(|&c| c == chunk[2])? as u8;
            result.push((b1 << 4) | (b2 >> 2));
            if chunk.len() > 3 && chunk[3] != b'=' {
                let b3 = ALPHABET.iter().position(|&c| c == chunk[3])? as u8;
                result.push((b2 << 6) | b3);
            }
        }
    }

    String::from_utf8(result).ok()
}

/// Collect embedded data from all PDFs in a folder
/// PDFに解析結果を埋め込む（コマンド）
#[tauri::command]
pub fn embed_pdf_result(path: String, result: String) -> Result<(), String> {
    embed_result_in_pdf(&path, &result)
}

/// PDFから解析結果を読み取る（コマンド）
#[tauri::command]
pub fn read_pdf_result(path: String) -> Option<(String, String)> {
    read_result_from_pdf(&path)
}
