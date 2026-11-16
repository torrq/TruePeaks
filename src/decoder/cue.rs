use crate::decoder::is_supported_audio_file;
use std::fs;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone)]
pub struct CueTrackSpec {
    pub track_number: u32,
    pub title: String,
    pub performer: Option<String>,
    pub file_path: PathBuf,
    pub start_secs: f64,
    pub end_secs: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct CueSheet {
    pub title: Option<String>,
    pub performer: Option<String>,
    pub tracks: Vec<CueTrackSpec>,
}

pub fn is_cue_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.eq_ignore_ascii_case("cue"))
            .unwrap_or(false)
}

fn parse_cue_timestamp_secs(ts: &str) -> Option<f64> {
    let parts: Vec<&str> = ts.trim().split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let mins: u64 = parts[0].parse().ok()?;
    let secs: u64 = parts[1].parse().ok()?;
    let frames: u64 = parts[2].parse().ok()?;
    Some((mins * 60 + secs) as f64 + (frames as f64) / 75.0)
}

fn unquote_first_arg(rest: &str) -> &str {
    let trimmed = rest.trim();
    if let Some(after_open) = trimmed.strip_prefix('"') {
        if let Some(end_quote) = after_open.find('"') {
            return &after_open[..end_quote];
        }
    }
    trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches('"')
}

fn resolve_cue_audio_path(cue_dir: &Path, raw_ref: &str) -> PathBuf {
    let ref_path = Path::new(raw_ref);
    let candidate = cue_dir.join(ref_path);
    if candidate.is_file() {
        return candidate;
    }
    if let Some(file_name) = ref_path.file_name() {
        let local = cue_dir.join(file_name);
        if local.is_file() {
            return local;
        }
    }
    // Fallback: match same file stem with any supported audio extension in cue_dir
    if let Some(stem) = ref_path.file_stem().and_then(|s| s.to_str()) {
        if let Ok(entries) = fs::read_dir(cue_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if is_supported_audio_file(&p)
                    && p.file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.eq_ignore_ascii_case(stem))
                        .unwrap_or(false)
                {
                    return p;
                }
            }
        }
    }
    candidate
}

pub fn parse_cue_sheet(cue_path: &Path) -> Result<CueSheet, String> {
    let bytes = fs::read(cue_path)
        .map_err(|e| format!("Failed to read CUE sheet {}: {}", cue_path.display(), e))?;
    let raw_text = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        let u16s: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else {
        let (decoded, _, _) = encoding_rs::UTF_8.decode(&bytes);
        if decoded.contains('\u{FFFD}') {
            encoding_rs::WINDOWS_1252.decode(&bytes).0.into_owned()
        } else {
            decoded.into_owned()
        }
    };

    let cue_dir = cue_path.parent().unwrap_or_else(|| Path::new("."));
    let mut sheet_title: Option<String> = None;
    let mut sheet_performer: Option<String> = None;
    let mut current_file: Option<PathBuf> = None;

    struct PendingTrack {
        number: u32,
        title: Option<String>,
        performer: Option<String>,
        file_path: Option<PathBuf>,
        start_secs: Option<f64>,
    }

    let mut pending: Vec<PendingTrack> = Vec::new();
    let mut current_track: Option<PendingTrack> = None;

    for raw_line in raw_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("REM ") {
            continue;
        }
        let upper = line.to_ascii_uppercase();

        if upper.starts_with("FILE ") {
            let ref_name = unquote_first_arg(&line[5..]);
            current_file = Some(resolve_cue_audio_path(cue_dir, ref_name));
        } else if upper.starts_with("TRACK ") {
            if let Some(t) = current_track.take() {
                pending.push(t);
            }
            let mut parts = line[6..].split_whitespace();
            let num = parts.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
            let track_type = parts.next().unwrap_or("AUDIO").to_ascii_uppercase();
            if track_type == "AUDIO" {
                current_track = Some(PendingTrack {
                    number: num,
                    title: None,
                    performer: None,
                    file_path: current_file.clone(),
                    start_secs: None,
                });
            }
        } else if upper.starts_with("TITLE ") {
            let val = unquote_first_arg(&line[6..]).trim().nfc().collect::<String>();
            if let Some(ref mut t) = current_track {
                t.title = Some(val);
            } else {
                sheet_title = Some(val);
            }
        } else if upper.starts_with("PERFORMER ") {
            let val = unquote_first_arg(&line[10..]).trim().nfc().collect::<String>();
            if let Some(ref mut t) = current_track {
                t.performer = Some(val);
            } else {
                sheet_performer = Some(val);
            }
        } else if upper.starts_with("INDEX ") {
            let mut parts = line[6..].split_whitespace();
            let idx_code = parts.next().unwrap_or("");
            let ts = parts.next().unwrap_or("");
            if idx_code == "01" {
                if let (Some(ref mut t), Some(secs)) = (&mut current_track, parse_cue_timestamp_secs(ts)) {
                    if t.start_secs.is_none() {
                        t.start_secs = Some(secs);
                    }
                    if t.file_path.is_none() {
                        t.file_path = current_file.clone();
                    }
                }
            }
        }
    }
    if let Some(t) = current_track.take() {
        pending.push(t);
    }

    let mut tracks = Vec::with_capacity(pending.len());
    for (idx, p) in pending.iter().enumerate() {
        let Some(ref fp) = p.file_path else {
            continue;
        };
        let Some(start) = p.start_secs else {
            continue;
        };
        let end = pending.get(idx + 1).and_then(|next| {
            if next.file_path.as_ref() == Some(fp) {
                next.start_secs
            } else {
                None
            }
        });
        let title = p
            .title
            .clone()
            .unwrap_or_else(|| format!("Track {:02}", p.number));
        tracks.push(CueTrackSpec {
            track_number: p.number,
            title,
            performer: p.performer.clone().or_else(|| sheet_performer.clone()),
            file_path: fp.clone(),
            start_secs: start,
            end_secs: end,
        });
    }

    if tracks.is_empty() {
        return Err(format!(
            "No playable AUDIO tracks with INDEX 01 found in {}",
            cue_path.display()
        ));
    }

    Ok(CueSheet {
        title: sheet_title,
        performer: sheet_performer,
        tracks,
    })
}
