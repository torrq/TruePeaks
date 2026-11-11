use crate::config::AppConfig;
use crate::decoder::{is_supported_audio_file, DecodeError};
use crate::dsp::{analyze_file, TrackAnalysis};
use crate::formatter::folder_log::{
    compute_official_album_dr, format_combined_multi_log, format_folder_log,
    format_recursive_folder_log, FolderTreeNode,
};
use crate::formatter::single_log::format_single_file_log;
use crate::formatter::{expand_filename_pattern, folder_display_name, write_log_file};
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
    Multi {
        dirs: Vec<PathBuf>,
        files: Vec<PathBuf>,
    },
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
        saved_paths: Vec<PathBuf>,
        elapsed_secs: f64,
        tracks_count: usize,
        folders_count: usize,
    },
    Cancelled,
    Error(String),
}

struct DirBlueprint {
    path: PathBuf,
    display_name: String,
    files: Vec<PathBuf>,
    children: Vec<DirBlueprint>,
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
            ScanTarget::Multi { dirs, files } => {
                run_multi_scan(&dirs, &files, &config, &cancel_flag, &tx, start_time);
            }
        }
    });
}

fn sort_paths_by_name(paths: &mut [PathBuf]) {
    paths.sort_by(|a, b| {
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
}

fn collect_dir_blueprint(dir: &Path, recurse: bool) -> Result<Option<DirBlueprint>, String> {
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {:?}: {}", dir, e))?;

    let mut files = Vec::new();
    let mut subdirs = Vec::new();

    for entry in entries.flatten() {
        let p = entry.path();
        if is_supported_audio_file(&p) {
            files.push(p);
        } else if recurse {
            let is_non_symlink_dir = fs::symlink_metadata(&p)
                .map(|m| m.is_dir() && !m.file_type().is_symlink())
                .unwrap_or(false);
            if is_non_symlink_dir {
                subdirs.push(p);
            }
        }
    }

    sort_paths_by_name(&mut files);
    sort_paths_by_name(&mut subdirs);

    let mut children = Vec::new();
    for sub in subdirs {
        if let Some(child_bp) = collect_dir_blueprint(&sub, true)? {
            children.push(child_bp);
        }
    }

    if files.is_empty() && children.is_empty() {
        return Ok(None);
    }

    Ok(Some(DirBlueprint {
        path: dir.to_path_buf(),
        display_name: folder_display_name(dir, "Album"),
        files,
        children,
    }))
}

fn collect_blueprint_paths(bp: &DirBlueprint, out: &mut Vec<PathBuf>) {
    out.extend(bp.files.iter().cloned());
    for child in &bp.children {
        collect_blueprint_paths(child, out);
    }
}

fn hydrate_blueprint(
    bp: DirBlueprint,
    iter: &mut impl Iterator<Item = TrackAnalysis>,
) -> FolderTreeNode {
    let mut tracks = Vec::with_capacity(bp.files.len());
    for _ in 0..bp.files.len() {
        if let Some(t) = iter.next() {
            tracks.push(t);
        }
    }
    let children = bp
        .children
        .into_iter()
        .map(|c| hydrate_blueprint(c, iter))
        .collect();
    FolderTreeNode {
        path: bp.path,
        display_name: bp.display_name,
        tracks,
        children,
    }
}

fn run_parallel_analysis(
    file_paths: &[PathBuf],
    config: &AppConfig,
    cancel_flag: &Arc<AtomicBool>,
    tx: &Sender<ScanEvent>,
) -> Result<Vec<TrackAnalysis>, Option<String>> {
    let total = file_paths.len();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.max_threads.max(1))
        .build()
        .map_err(|e| Some(format!("Failed to build worker thread pool: {}", e)))?;

    let completed_counter = Arc::new(AtomicUsize::new(0));
    let include_lfe = config.include_lfe_in_dr;

    let results: Vec<Result<TrackAnalysis, DecodeError>> = pool.install(|| {
        file_paths
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
        return Err(None);
    }

    let mut tracks = Vec::with_capacity(total);
    for res in results {
        match res {
            Ok(t) => tracks.push(t),
            Err(DecodeError::Cancelled) => return Err(None),
            Err(e) => return Err(Some(e.to_string())),
        }
    }
    Ok(tracks)
}

fn format_folder_node(node: &FolderTreeNode, config: &AppConfig) -> (String, i32) {
    if config.recurse_subfolders {
        let txt = format_recursive_folder_log(
            node,
            config.folder_log_style,
            &config.folder_columns,
            &config.log_filenames,
            config.surround_folder_mode,
        );
        (txt, node.official_dr())
    } else {
        let txt = format_folder_log(
            &node.path,
            &node.tracks,
            config.folder_log_style,
            &config.folder_columns,
            &config.log_filenames,
            config.surround_folder_mode,
        );
        (txt, compute_official_album_dr(&node.tracks))
    }
}

fn save_single_track_log(
    file_path: &Path,
    track: &TrackAnalysis,
    log_text: &str,
    config: &AppConfig,
) -> Result<PathBuf, String> {
    let parent_dir = file_path.parent().unwrap_or_else(|| Path::new("."));
    let folder_name = folder_display_name(parent_dir, "Audio");
    let out_filename = expand_filename_pattern(
        &config.single_log_pattern,
        &folder_name,
        Some(&track.file_stem),
        Some(&track.file_format),
        Some(track.dr_track_rounded),
    );
    let out_path = parent_dir.join(out_filename);
    write_log_file(
        &out_path,
        log_text,
        config.charset,
        config.line_ending,
        config.overwrite_existing_logs,
    )
}

fn save_folder_log(
    dir: &Path,
    folder_name: &str,
    album_dr: Option<i32>,
    log_text: &str,
    config: &AppConfig,
) -> Result<PathBuf, String> {
    let out_filename = expand_filename_pattern(
        &config.folder_log_pattern,
        folder_name,
        None,
        None,
        album_dr,
    );
    let out_path = dir.join(out_filename);
    write_log_file(
        &out_path,
        log_text,
        config.charset,
        config.line_ending,
        config.overwrite_existing_logs,
    )
}

fn common_save_directory(roots: &[FolderTreeNode], files: &[PathBuf]) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for r in roots {
        if let Some(p) = r.path.parent() {
            dirs.push(p.to_path_buf());
        } else {
            dirs.push(r.path.clone());
        }
    }
    for f in files {
        if let Some(p) = f.parent() {
            dirs.push(p.to_path_buf());
        }
    }
    if dirs.is_empty() {
        return None;
    }

    let first_comps: Vec<_> = dirs[0].components().collect();
    let mut common_len = first_comps.len();
    for d in &dirs[1..] {
        let comps: Vec<_> = d.components().collect();
        let shared = first_comps
            .iter()
            .zip(comps.iter())
            .take_while(|(a, b)| a == b)
            .count();
        common_len = common_len.min(shared);
    }

    let mut ancestor = PathBuf::new();
    for c in &first_comps[..common_len] {
        ancestor.push(c.as_os_str());
    }
    if ancestor.parent().is_some() && ancestor.is_dir() {
        return Some(ancestor);
    }

    // Fall back safely if inputs span separate drives/roots
    roots
        .first()
        .map(|r| r.path.clone())
        .or_else(|| files.first().and_then(|f| f.parent().map(|p| p.to_path_buf())))
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
        target_label: label,
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

            let log_text =
                format_single_file_log(&track, &config.single_rows, &config.log_filenames);
            let mut saved_paths = Vec::new();

            if config.save_logs_to_disk {
                match save_single_track_log(path, &track, &log_text, config) {
                    Ok(out_path) => saved_paths.push(out_path),
                    Err(e) => {
                        let _ = tx.send(ScanEvent::Error(e));
                        return;
                    }
                }
            }

            let _ = tx.send(ScanEvent::Completed {
                log_text,
                saved_paths,
                elapsed_secs: start_time.elapsed().as_secs_f64(),
                tracks_count: 1,
                folders_count: 0,
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
    let bp = match collect_dir_blueprint(dir, config.recurse_subfolders) {
        Ok(Some(bp)) => bp,
        Ok(None) => {
            let msg = if config.recurse_subfolders {
                "No supported audio files found in the selected folder or its subfolders."
            } else {
                "No supported audio files found in the selected folder."
            };
            let _ = tx.send(ScanEvent::Error(msg.to_string()));
            return;
        }
        Err(e) => {
            let _ = tx.send(ScanEvent::Error(e));
            return;
        }
    };

    let mut all_paths = Vec::new();
    collect_blueprint_paths(&bp, &mut all_paths);
    let total = all_paths.len();
    let folder_name = bp.display_name.clone();

    let _ = tx.send(ScanEvent::Started {
        total_files: total,
        target_label: folder_name.clone(),
    });

    let tracks = match run_parallel_analysis(&all_paths, config, cancel_flag, tx) {
        Ok(t) => t,
        Err(None) => {
            let _ = tx.send(ScanEvent::Cancelled);
            return;
        }
        Err(Some(e)) => {
            let _ = tx.send(ScanEvent::Error(e));
            return;
        }
    };

    let mut iter = tracks.into_iter();
    let root = hydrate_blueprint(bp, &mut iter);
    let (log_text, album_dr) = format_folder_node(&root, config);

    let mut saved_paths = Vec::new();
    if config.save_logs_to_disk {
        match save_folder_log(dir, &folder_name, Some(album_dr), &log_text, config) {
            Ok(out_path) => saved_paths.push(out_path),
            Err(e) => {
                let _ = tx.send(ScanEvent::Error(e));
                return;
            }
        }
    }

    let _ = tx.send(ScanEvent::Completed {
        log_text,
        saved_paths,
        elapsed_secs: start_time.elapsed().as_secs_f64(),
        tracks_count: total,
        folders_count: 1,
    });
}

fn run_multi_scan(
    dirs: &[PathBuf],
    files: &[PathBuf],
    config: &AppConfig,
    cancel_flag: &Arc<AtomicBool>,
    tx: &Sender<ScanEvent>,
    start_time: Instant,
) {
    let mut blueprints = Vec::new();
    for d in dirs {
        match collect_dir_blueprint(d, config.recurse_subfolders) {
            Ok(Some(bp)) => blueprints.push(bp),
            Ok(None) => {}
            Err(e) => {
                let _ = tx.send(ScanEvent::Error(e));
                return;
            }
        }
    }

    let mut sorted_files = files.to_vec();
    sort_paths_by_name(&mut sorted_files);

    let mut all_paths = Vec::new();
    for bp in &blueprints {
        collect_blueprint_paths(bp, &mut all_paths);
    }
    all_paths.extend(sorted_files.iter().cloned());

    if all_paths.is_empty() {
        let _ = tx.send(ScanEvent::Error(
            "No supported audio files found in the dropped items.".to_string(),
        ));
        return;
    }

    let total = all_paths.len();
    let folders_count = blueprints.len();
    let label = if config.combine_dropped_folders {
        format!("Combined ({} folders)", folders_count)
    } else if folders_count > 0 {
        format!("{} folders", folders_count)
    } else {
        format!("{} files", sorted_files.len())
    };

    let _ = tx.send(ScanEvent::Started {
        total_files: total,
        target_label: label,
    });

    let analyzed = match run_parallel_analysis(&all_paths, config, cancel_flag, tx) {
        Ok(t) => t,
        Err(None) => {
            let _ = tx.send(ScanEvent::Cancelled);
            return;
        }
        Err(Some(e)) => {
            let _ = tx.send(ScanEvent::Error(e));
            return;
        }
    };

    let mut iter = analyzed.into_iter();
    let roots: Vec<FolderTreeNode> = blueprints
        .into_iter()
        .map(|bp| hydrate_blueprint(bp, &mut iter))
        .collect();
    let loose_tracks: Vec<TrackAnalysis> = iter.collect();

    let mut saved_paths = Vec::new();
    let log_text = if config.combine_dropped_folders {
        let combined = format_combined_multi_log(
            &roots,
            &loose_tracks,
            config.folder_log_style,
            &config.folder_columns,
            &config.single_rows,
            &config.log_filenames,
            config.surround_folder_mode,
        );

        if config.save_logs_to_disk {
            let sum_dr: f64 = roots.iter().map(|r| r.sum_dr_raw()).sum::<f64>()
                + loose_tracks.iter().map(|t| t.dr_track_raw).sum::<f64>();
            let overall_dr = (sum_dr / (total as f64)).round() as i32;

            if let Some(save_dir) = common_save_directory(&roots, &sorted_files) {
                let save_name = folder_display_name(&save_dir, "Combined");
                match save_folder_log(&save_dir, &save_name, Some(overall_dr), &combined, config) {
                    Ok(out_path) => saved_paths.push(out_path),
                    Err(e) => {
                        let _ = tx.send(ScanEvent::Error(e));
                        return;
                    }
                }
            }
        }
        combined
    } else {
        // Default: separate log per dropped folder (and per dropped audio file)
        let rule_eq = "=".repeat(80);
        let mut sections = Vec::new();

        for root in &roots {
            let (section_log, album_dr) = format_folder_node(root, config);
            let mut banner = format!("[Folder: {}]", root.display_name);

            if config.save_logs_to_disk {
                match save_folder_log(
                    &root.path,
                    &root.display_name,
                    Some(album_dr),
                    &section_log,
                    config,
                ) {
                    Ok(out_path) => {
                        banner = format!("[Saved: {}]", out_path.display());
                        saved_paths.push(out_path);
                    }
                    Err(e) => {
                        let _ = tx.send(ScanEvent::Error(e));
                        return;
                    }
                }
            }

            if roots.len() + loose_tracks.len() > 1 {
                sections.push(format!("{}\n{}\n{}", banner, rule_eq, section_log));
            } else {
                sections.push(section_log);
            }
        }

        for (file_path, track) in sorted_files.iter().zip(loose_tracks.iter()) {
            let s_log = format_single_file_log(track, &config.single_rows, &config.log_filenames);
            let mut banner = format!("[File: {}]", track.file_stem);

            if config.save_logs_to_disk {
                match save_single_track_log(file_path, track, &s_log, config) {
                    Ok(out_path) => {
                        banner = format!("[Saved: {}]", out_path.display());
                        saved_paths.push(out_path);
                    }
                    Err(e) => {
                        let _ = tx.send(ScanEvent::Error(e));
                        return;
                    }
                }
            }

            if roots.len() + loose_tracks.len() > 1 {
                sections.push(format!("{}\n{}\n{}", banner, rule_eq, s_log));
            } else {
                sections.push(s_log);
            }
        }

        sections.join("\n\n")
    };

    let _ = tx.send(ScanEvent::Completed {
        log_text,
        saved_paths,
        elapsed_secs: start_time.elapsed().as_secs_f64(),
        tracks_count: total,
        folders_count,
    });
}
