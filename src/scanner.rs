use crate::config::AppConfig;
use crate::decoder::{is_supported_audio_file, DecodeError};
use crate::dsp::{analyze_file, TrackAnalysis};
use crate::formatter::folder_log::{compute_official_album_dr, format_folder_log};
use crate::formatter::single_log::format_single_file_log;
use crate::formatter::{expand_filename_pattern, normalize_nfc, write_log_file};
use crossbeam_channel::Sender;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub enum ScanTarget {
    SingleFile(PathBuf),
    Folder(PathBuf),
}

#[derive(Debug, Clone)]
pub enum ScanEvent {
    Started {
        total_files: usize,
        target_label: String,
    },
    Progress {
        completed: usize,
        total: usize,
        last_file: String,
    },
    Completed {
        log_text: String,
        saved_path: Option<PathBuf>,
        elapsed_secs: f64,
        tracks_count: usize,
    },
    Cancelled,
    Error(String),
}

pub fn spawn_scan_job(
    target: ScanTarget,
    config: AppConfig,
    cancel_flag: Arc<AtomicBool>,
    tx: Sender<ScanEvent>,
) {
    std::thread::spawn(move || {
        let start_time = Instant::now();
        match target {
            ScanTarget::SingleFile(path) => {
                run_single_file_scan(&path, &config, &cancel_flag, &tx, start_time);
            }
            ScanTarget::Folder(dir) => {
                run_folder_scan(&dir, &config, &cancel_flag, &tx, start_time);
            }
        }
    });
}

fn run_single_file_scan(
    path: &Path,
    config: &AppConfig,
    cancel_flag: &Arc<AtomicBool>,
    tx: &Sender<ScanEvent>,
    start_time: Instant,
) {
    let label = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("File")
        .to_string();

    let _ = tx.send(ScanEvent::Started {
        total_files: 1,
        target_label: label.clone(),
    });

    match analyze_file(path, config.include_lfe_in_dr, cancel_flag) {
        Ok(track) => {
            if cancel_flag.load(Ordering::Relaxed) {
                let _ = tx.send(ScanEvent::Cancelled);
                return;
            }
            let _ = tx.send(ScanEvent::Progress {
                completed: 1,
                total: 1,
                last_file: track.file_stem.clone(),
            });

            let log_text = format_single_file_log(&track, &config.single_rows);
            let mut saved_path = None;

            if config.save_logs_to_disk {
                let parent_dir = path.parent().unwrap_or_else(|| Path::new("."));
                let folder_name = parent_dir
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(normalize_nfc)
                    .unwrap_or_else(|| "Audio".to_string());
                let out_filename = expand_filename_pattern(
                    &config.single_log_pattern,
                    &folder_name,
                    Some(&track.file_stem),
                    Some(&track.file_format),
                    Some(track.dr_track_rounded),
                );
                let out_path = parent_dir.join(out_filename);
                if let Err(e) =
                    write_log_file(&out_path, &log_text, config.charset, config.line_ending)
                {
                    let _ = tx.send(ScanEvent::Error(e));
                    return;
                }
                saved_path = Some(out_path);
            }

            let _ = tx.send(ScanEvent::Completed {
                log_text,
                saved_path,
                elapsed_secs: start_time.elapsed().as_secs_f64(),
                tracks_count: 1,
            });
        }
        Err(DecodeError::Cancelled) => {
            let _ = tx.send(ScanEvent::Cancelled);
        }
        Err(e) => {
            let _ = tx.send(ScanEvent::Error(format!(
                "Failed to analyze {:?}: {}",
                path.file_name().unwrap_or_default(),
                e
            )));
        }
    }
}

fn run_folder_scan(
    dir: &Path,
    config: &AppConfig,
    cancel_flag: &Arc<AtomicBool>,
    tx: &Sender<ScanEvent>,
    start_time: Instant,
) {
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            let _ = tx.send(ScanEvent::Error(format!(
                "Failed to read directory {:?}: {}",
                dir, e
            )));
            return;
        }
    };

    let mut audio_files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|d| d.path()))
        .filter(|p| is_supported_audio_file(p))
        .collect();

    audio_files.sort_by(|a, b| {
        let a_name = a
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let b_name = b
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        a_name.cmp(&b_name)
    });

    if audio_files.is_empty() {
        let _ = tx.send(ScanEvent::Error(
            "No supported audio files found in the selected folder.".to_string(),
        ));
        return;
    }

    let total = audio_files.len();
    let folder_name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .map(normalize_nfc)
        .unwrap_or_else(|| "Album".to_string());

    let _ = tx.send(ScanEvent::Started {
        total_files: total,
        target_label: folder_name.clone(),
    });

    let pool = match rayon::ThreadPoolBuilder::new()
        .num_threads(config.max_threads.max(1))
        .build()
    {
        Ok(p) => p,
        Err(e) => {
            let _ = tx.send(ScanEvent::Error(format!(
                "Failed to build worker thread pool: {}",
                e
            )));
            return;
        }
    };

    let completed_counter = Arc::new(AtomicUsize::new(0));
    let include_lfe = config.include_lfe_in_dr;

    let results: Vec<Result<TrackAnalysis, DecodeError>> = pool.install(|| {
        audio_files
            .par_iter()
            .map(|file_path| {
                if cancel_flag.load(Ordering::Relaxed) {
                    return Err(DecodeError::Cancelled);
                }
                let res = analyze_file(file_path, include_lfe, cancel_flag);
                if let Ok(ref track) = res {
                    let done = completed_counter.fetch_add(1, Ordering::Relaxed) + 1;
                    let _ = tx.send(ScanEvent::Progress {
                        completed: done,
                        total,
                        last_file: track.file_stem.clone(),
                    });
                }
                res
            })
            .collect()
    });

    if cancel_flag.load(Ordering::Relaxed) {
        let _ = tx.send(ScanEvent::Cancelled);
        return;
    }

    let mut tracks = Vec::with_capacity(total);
    for res in results {
        match res {
            Ok(t) => tracks.push(t),
            Err(DecodeError::Cancelled) => {
                let _ = tx.send(ScanEvent::Cancelled);
                return;
            }
            Err(e) => {
                let _ = tx.send(ScanEvent::Error(e.to_string()));
                return;
            }
        }
    }

    let log_text = format_folder_log(
        dir,
        &tracks,
        &config.folder_columns,
        config.surround_folder_mode,
    );

    let mut saved_path = None;
    if config.save_logs_to_disk {
        let album_dr = compute_official_album_dr(&tracks);
        let out_filename = expand_filename_pattern(
            &config.folder_log_pattern,
            &folder_name,
            None,
            None,
            Some(album_dr),
        );
        let out_path = dir.join(out_filename);
        if let Err(e) = write_log_file(&out_path, &log_text, config.charset, config.line_ending) {
            let _ = tx.send(ScanEvent::Error(e));
            return;
        }
        saved_path = Some(out_path);
    }

    let _ = tx.send(ScanEvent::Completed {
        log_text,
        saved_path,
        elapsed_secs: start_time.elapsed().as_secs_f64(),
        tracks_count: tracks.len(),
    });
}
