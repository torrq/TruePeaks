pub mod folder_log;
pub mod single_log;

use crate::config::{CharsetMode, LineEndingMode};
use chrono::Local;
use encoding_rs::WINDOWS_1252;
use std::fs;
use std::path::Path;
use unicode_normalization::UnicodeNormalization;
use unicode_width::UnicodeWidthStr;

pub fn format_sample_rate(sr: u32) -> String {
    if sr % 1000 == 0 {
        format!("{}k", sr / 1000)
    } else {
        format!("{:.1}k", (sr as f64) / 1000.0)
    }
}

pub fn normalize_nfc(s: &str) -> String {
    s.nfc().collect::<String>()
}

pub fn folder_display_name(dir: &Path, fallback: &str) -> String {
    let canonical = fs::canonicalize(dir);
    let resolved = canonical.as_deref().unwrap_or(dir);
    resolved
        .file_name()
        .map(|s| normalize_nfc(&s.to_string_lossy()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let raw = resolved.to_string_lossy();
            let trimmed = raw.strip_prefix(r"\\?\").unwrap_or(&raw).trim();
            if trimmed.is_empty() {
                fallback.to_string()
            } else {
                normalize_nfc(trimmed)
            }
        })
}

pub fn visual_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn pad_left_visual(s: &str, target_width: usize) -> String {
    let w = visual_width(s);
    if w >= target_width {
        s.to_string()
    } else {
        format!("{}{}", " ".repeat(target_width - w), s)
    }
}

pub fn apply_line_endings(text: &str, mode: LineEndingMode) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    match mode {
        LineEndingMode::Lf => normalized,
        LineEndingMode::Crlf => normalized.replace('\n', "\r\n"),
        LineEndingMode::Cr => normalized.replace('\n', "\r"),
    }
}

pub fn encode_log_bytes(text: &str, charset: CharsetMode, line_ending: LineEndingMode) -> Vec<u8> {
    let with_le = apply_line_endings(text, line_ending);
    match charset {
        CharsetMode::Utf8 => with_le.into_bytes(),
        CharsetMode::Utf8Bom => {
            let mut out = vec![0xEF, 0xBB, 0xBF];
            out.extend_from_slice(with_le.as_bytes());
            out
        }
        CharsetMode::Utf16Le => {
            let mut out = vec![0xFF, 0xFE];
            for code_unit in with_le.encode_utf16() {
                out.extend_from_slice(&code_unit.to_le_bytes());
            }
            out
        }
        CharsetMode::Utf16Be => {
            let mut out = vec![0xFE, 0xFF];
            for code_unit in with_le.encode_utf16() {
                out.extend_from_slice(&code_unit.to_be_bytes());
            }
            out
        }
        CharsetMode::Windows1252 => {
            let (cow, _, _) = WINDOWS_1252.encode(&with_le);
            cow.into_owned()
        }
    }
}

pub fn write_log_file(
    path: &Path,
    text: &str,
    charset: CharsetMode,
    line_ending: LineEndingMode,
) -> Result<(), String> {
    let bytes = encode_log_bytes(text, charset, line_ending);
    fs::write(path, bytes).map_err(|e| format!("Failed to write log file {:?}: {}", path, e))
}

pub fn expand_filename_pattern(
    pattern: &str,
    folder_name: &str,
    file_name: Option<&str>,
    ext: Option<&str>,
    album_dr: Option<i32>,
) -> String {
    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H-%M-%S").to_string();

    let mut out = pattern.to_string();
    out = out.replace("{folder_name}", folder_name);
    if let Some(fn_str) = file_name {
        out = out.replace("{file_name}", fn_str);
    }
    if let Some(ext_str) = ext {
        let clean_ext = ext_str.trim_start_matches('.');
        out = out.replace("{ext}", clean_ext);
    }
    if let Some(dr) = album_dr {
        out = out.replace("{album_dr}", &dr.to_string());
    }
    out = out.replace("{date}", &date_str);
    out = out.replace("{time}", &time_str);
    if out.trim().is_empty() {
        if let Some(fn_str) = file_name {
            format!("{}_log.txt", fn_str)
        } else {
            "DR.txt".to_string()
        }
    } else {
        out
    }
}
