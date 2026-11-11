use crate::config::{
    FolderColumnsConfig, FolderLogStyle, LogFilenameOptions, SingleFileRowsConfig,
    SurroundFolderMode,
};
use crate::dsp::TrackAnalysis;
use crate::formatter::single_log::format_single_file_log;
use crate::formatter::{
    folder_display_name, format_log_track_name, format_sample_rate, pad_left_visual, visual_width,
};
use std::path::{Path, PathBuf};

struct ColumnDef {
    header: String,
    extract: Box<dyn Fn(&TrackAnalysis) -> String>,
}

pub fn compute_official_album_dr(tracks: &[TrackAnalysis]) -> i32 {
    if tracks.is_empty() {
        return 0;
    }
    let sum_raw: f64 = tracks.iter().map(|t| t.dr_track_raw).sum();
    (sum_raw / (tracks.len() as f64)).round() as i32
}

pub fn format_folder_log(
    folder_path: &Path,
    tracks: &[TrackAnalysis],
    style: FolderLogStyle,
    cols_cfg: &FolderColumnsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
) -> String {
    match style {
        FolderLogStyle::TruePeaksTable => {
            format_truepeaks_table(folder_path, tracks, cols_cfg, name_opts, surround_mode)
        }
        FolderLogStyle::ClassicFooDr => format_classic_foo_dr(folder_path, tracks, name_opts),
        FolderLogStyle::CompactMastering => {
            format_compact_mastering(folder_path, tracks, name_opts)
        }
    }
}

fn format_duration_mmss(secs_f64: f64) -> String {
    let total_secs = secs_f64.max(0.0).round() as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{}:{:02}", mins, secs)
    }
}

fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.iter().any(|existing| existing == &item) {
        list.push(item);
    }
}

fn codec_display_name(ext: &str) -> String {
    match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "wav" => "PCM (WAV)".to_string(),
        "bwf" => "PCM (BWF)".to_string(),
        "aif" | "aiff" | "aifc" => "PCM (AIFF)".to_string(),
        "w64" => "PCM (Wave64)".to_string(),
        "flac" => "FLAC".to_string(),
        "alac" => "ALAC".to_string(),
        "m4a" | "mp4" => "ALAC / AAC (M4A)".to_string(),
        "mp3" => "MP3".to_string(),
        "aac" => "AAC".to_string(),
        "ogg" | "oga" => "Ogg Vorbis".to_string(),
        "caf" => "CAF".to_string(),
        other if other.is_empty() => "Unknown".to_string(),
        other => other.to_ascii_uppercase(),
    }
}

fn album_summary_metadata(tracks: &[TrackAnalysis]) -> (String, String, Option<String>, Option<u64>, String) {
    let mut sample_rates = Vec::new();
    let mut channel_counts = Vec::new();
    let mut bit_depths = Vec::new();
    let mut codecs = Vec::new();
    let mut total_bytes = 0u64;
    let mut total_secs = 0.0_f64;

    for t in tracks {
        push_unique(&mut sample_rates, t.sample_rate.to_string());
        push_unique(&mut channel_counts, t.channels.len().to_string());
        if let Some(wl) = t.word_length {
            push_unique(&mut bit_depths, wl.to_string());
        }
        push_unique(&mut codecs, codec_display_name(&t.file_format));
        total_bytes = total_bytes.saturating_add(t.file_size_bytes);
        total_secs += t.duration_secs();
    }

    let sr_str = if sample_rates.is_empty() {
        "-".to_string()
    } else {
        format!("{} Hz", sample_rates.join(" / "))
    };
    let ch_str = if channel_counts.is_empty() {
        "-".to_string()
    } else {
        channel_counts.join(" / ")
    };
    let bits_str = if bit_depths.is_empty() {
        None
    } else {
        Some(bit_depths.join(" / "))
    };
    let bitrate_kbps = if total_bytes > 0 && total_secs > 0.0 {
        Some(((total_bytes as f64) * 8.0 / total_secs / 1000.0).round() as u64)
    } else {
        None
    };
    let codec_str = if codecs.is_empty() {
        "-".to_string()
    } else {
        codecs.join(" / ")
    };

    (sr_str, ch_str, bits_str, bitrate_kbps, codec_str)
}

fn format_db_suffix(v: f64) -> String {
    if v.is_finite() {
        format!("{:.2} dB", v)
    } else {
        "-inf dB".to_string()
    }
}

fn format_db_plain(v: f64, signed: bool) -> String {
    if !v.is_finite() {
        "-inf".to_string()
    } else if signed {
        format!("{:+.2}", v)
    } else {
        format!("{:.2}", v)
    }
}

fn format_classic_foo_dr(
    folder_path: &Path,
    tracks: &[TrackAnalysis],
    name_opts: &LogFilenameOptions,
) -> String {
    let album_name = folder_display_name(folder_path, "Album");
    let rule_dash = "-".repeat(80);
    let rule_eq = "=".repeat(80);

    let mut lines: Vec<String> = Vec::with_capacity(tracks.len() + 18);
    lines.push(rule_dash.clone());
    lines.push(format!("Analyzed: {}", album_name));
    lines.push(rule_dash.clone());
    lines.push(String::new());
    lines.push("DR         Peak         RMS     Duration Track".to_string());
    lines.push(rule_dash.clone());

    for t in tracks {
        let dr_col = format!("DR{}", t.dr_track_rounded);
        let peak_col = format_db_suffix(t.max_sppm_joint);
        let rms_col = format_db_suffix(t.combined_rms_db());
        let dur_col = format_duration_mmss(t.duration_secs());
        let track_name = format_log_track_name(&t.file_stem, name_opts);
        lines.push(format!(
            "{:<9}{:>9}   {:>9} {:>9} {}",
            dr_col, peak_col, rms_col, dur_col, track_name
        ));
    }

    lines.push(rule_dash);
    lines.push(String::new());
    lines.push(format!("Number of tracks:  {}", tracks.len()));
    lines.push(format!(
        "Official DR value: DR{}",
        compute_official_album_dr(tracks)
    ));
    lines.push(String::new());

    let (sr_str, ch_str, bits_opt, bitrate_opt, codec_str) = album_summary_metadata(tracks);
    lines.push(format!("Samplerate:        {}", sr_str));
    lines.push(format!("Channels:          {}", ch_str));
    if let Some(bits_str) = bits_opt {
        lines.push(format!("Bits per sample:   {}", bits_str));
    }
    if let Some(kbps) = bitrate_opt {
        lines.push(format!("Bitrate:           {} kbps", kbps));
    }
    lines.push(format!("Codec:             {}", codec_str));
    lines.push(rule_eq);

    lines.join("\n")
}

fn format_compact_mastering(
    folder_path: &Path,
    tracks: &[TrackAnalysis],
    name_opts: &LogFilenameOptions,
) -> String {
    let album_name = folder_display_name(folder_path, "Album");
    let official_dr = compute_official_album_dr(tracks);
    let rule_dash = "-".repeat(80);
    let rule_eq = "=".repeat(80);

    let total_secs: f64 = tracks.iter().map(|t| t.duration_secs()).sum();
    let (sr_str, _ch_str, bits_opt, _bitrate_opt, codec_str) = album_summary_metadata(tracks);
    let fmt_summary = match bits_opt {
        Some(bits) => format!("{} {}-bit {}", sr_str, bits, codec_str),
        None => format!("{} {}", sr_str, codec_str),
    };

    // Duration-weighted integrated loudness across album tracks
    let mut sum_weight = 0.0_f64;
    let mut sum_lufs_power = 0.0_f64;
    let mut max_tpl = f64::NEG_INFINITY;
    let mut max_sppm = f64::NEG_INFINITY;
    for t in tracks {
        let dur = t.duration_secs().max(0.1);
        if t.integrated_lufs.is_finite() {
            sum_lufs_power += dur * 10.0_f64.powf(t.integrated_lufs / 10.0);
            sum_weight += dur;
        }
        if t.max_tpl_joint > max_tpl {
            max_tpl = t.max_tpl_joint;
        }
        if t.max_sppm_joint > max_sppm {
            max_sppm = t.max_sppm_joint;
        }
    }
    let avg_lufs = if sum_weight > 0.0 && sum_lufs_power > 0.0 {
        10.0 * (sum_lufs_power / sum_weight).log10()
    } else {
        f64::NEG_INFINITY
    };

    let mut lines: Vec<String> = Vec::with_capacity(tracks.len() + 10);
    lines.push(rule_eq);
    lines.push(format!("Album:   {}", album_name));
    lines.push(format!(
        "Summary: DR{} (Official) | {} {} | {} | {}",
        official_dr,
        tracks.len(),
        if tracks.len() == 1 { "track" } else { "tracks" },
        format_duration_mmss(total_secs),
        fmt_summary
    ));
    lines.push(format!(
        "Metrics: Avg {} LUFS | Max TPL {} dBTP | Max SPPM {} dBFS",
        format_db_plain(avg_lufs, false),
        format_db_plain(max_tpl, true),
        format_db_plain(max_sppm, false),
    ));
    lines.push(rule_dash.clone());
    lines.push(format!(
        "{:<6}{:>9}{:>10}{:>8}{:>7}{:>9}   {}",
        "DR", "Peak dB", "TPL dBTP", "LUFSi", "PSR", "Time", "Track"
    ));
    lines.push(rule_dash.clone());

    for t in tracks {
        lines.push(format!(
            "{:<6}{:>9}{:>10}{:>8}{:>7}{:>9}   {}",
            format!("DR{}", t.dr_track_rounded),
            format_db_plain(t.max_sppm_joint, false),
            format_db_plain(t.max_tpl_joint, true),
            format_db_plain(t.integrated_lufs, false),
            format_db_plain(t.min_psr, false),
            format_duration_mmss(t.duration_secs()),
            format_log_track_name(&t.file_stem, name_opts)
        ));
    }

    lines.push(rule_dash);
    lines.join("\n")
}

fn format_truepeaks_table(
    folder_path: &Path,
    tracks: &[TrackAnalysis],
    cols_cfg: &FolderColumnsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
) -> String {
    let max_channels = tracks
        .iter()
        .map(|t| t.channels.len())
        .max()
        .unwrap_or(2);
    let expand_surround =
        max_channels > 2 && surround_mode == SurroundFolderMode::ExpandPerChannel;
    let rep_channels = tracks
        .iter()
        .find(|t| t.channels.len() == max_channels)
        .map(|t| t.channels.clone())
        .unwrap_or_default();

    let mut cols: Vec<ColumnDef> = Vec::new();

    // Group 1: Info
    if cols_cfg.file_name {
        let name_opts_copy = *name_opts;
        cols.push(ColumnDef {
            header: "File Name".to_string(),
            extract: Box::new(move |t| format_log_track_name(&t.file_stem, &name_opts_copy)),
        });
    }
    if cols_cfg.file_format {
        cols.push(ColumnDef {
            header: "Format".to_string(),
            extract: Box::new(|t| t.file_format.clone()),
        });
    }
    if cols_cfg.sample_rate {
        cols.push(ColumnDef {
            header: "SR".to_string(),
            extract: Box::new(|t| format_sample_rate(t.sample_rate)),
        });
    }
    if cols_cfg.word_length {
        cols.push(ColumnDef {
            header: "Word Length".to_string(),
            extract: Box::new(|t| {
                t.word_length
                    .map(|w| w.to_string())
                    .unwrap_or_else(|| "-".to_string())
            }),
        });
    }
    if cols_cfg.bits_used {
        cols.push(ColumnDef {
            header: "Bits Used".to_string(),
            extract: Box::new(|t| {
                t.bits_used
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "-".to_string())
            }),
        });
    }

    // Group 2: DR
    if cols_cfg.dr_pmf {
        cols.push(ColumnDef {
            header: "DR (PMF)".to_string(),
            extract: Box::new(|t| t.dr_track_rounded.to_string()),
        });
    }
    if expand_surround && (cols_cfg.dr_left || cols_cfg.dr_right) {
        for (idx, ch) in rep_channels.iter().enumerate() {
            let lbl = format!("DR {}", ch.short_label());
            cols.push(ColumnDef {
                header: lbl,
                extract: Box::new(move |t| {
                    t.dr_channels
                        .get(idx)
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    } else {
        if cols_cfg.dr_left {
            cols.push(ColumnDef {
                header: "DR LEFT".to_string(),
                extract: Box::new(|t| {
                    t.dr_channels
                        .first()
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
        if cols_cfg.dr_right {
            cols.push(ColumnDef {
                header: "DR RIGHT".to_string(),
                extract: Box::new(|t| {
                    t.dr_channels
                        .get(1)
                        .or_else(|| t.dr_channels.first())
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    }

    // Group 3: True Peak Level
    if cols_cfg.max_tpl_itu {
        cols.push(ColumnDef {
            header: "Max. TPL".to_string(),
            extract: Box::new(|t| format!("{:+.2}", t.max_tpl_joint)),
        });
    }
    if expand_surround && (cols_cfg.max_tpl_left || cols_cfg.max_tpl_right) {
        for (idx, ch) in rep_channels.iter().enumerate() {
            let lbl = format!("Max. TPL {}", ch.short_label());
            cols.push(ColumnDef {
                header: lbl,
                extract: Box::new(move |t| {
                    t.max_tpl_channels
                        .get(idx)
                        .map(|v| format!("{:+.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    } else {
        if cols_cfg.max_tpl_left {
            cols.push(ColumnDef {
                header: "Max. TPL LEFT".to_string(),
                extract: Box::new(|t| {
                    t.max_tpl_channels
                        .first()
                        .map(|v| format!("{:+.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
        if cols_cfg.max_tpl_right {
            cols.push(ColumnDef {
                header: "Max. TPL RIGHT".to_string(),
                extract: Box::new(|t| {
                    t.max_tpl_channels
                        .get(1)
                        .or_else(|| t.max_tpl_channels.first())
                        .map(|v| format!("{:+.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    }

    // Group 4: LUFS
    if cols_cfg.lufsi {
        cols.push(ColumnDef {
            header: "LUFSi".to_string(),
            extract: Box::new(|t| format!("{:.2}", t.integrated_lufs)),
        });
    }
    if cols_cfg.lra {
        cols.push(ColumnDef {
            header: "LRA".to_string(),
            extract: Box::new(|t| format!("{:.2}", t.lra)),
        });
    }
    if cols_cfg.max_mom_itu {
        cols.push(ColumnDef {
            header: "Max. Mom.".to_string(),
            extract: Box::new(|t| format!("{:.2}", t.max_mom_joint)),
        });
    }
    if cols_cfg.max_short_term_itu {
        cols.push(ColumnDef {
            header: "Max. Short-Term".to_string(),
            extract: Box::new(|t| format!("{:.2}", t.max_short_term_joint)),
        });
    }

    // Group 5: Channel Specific LUFS
    if expand_surround && (cols_cfg.max_mom_left || cols_cfg.max_mom_right) {
        for (idx, ch) in rep_channels.iter().enumerate() {
            let lbl = format!("Max. Mom. {}", ch.short_label());
            cols.push(ColumnDef {
                header: lbl,
                extract: Box::new(move |t| {
                    t.max_mom_channels
                        .get(idx)
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    } else {
        if cols_cfg.max_mom_left {
            cols.push(ColumnDef {
                header: "Max. Mom. LEFT".to_string(),
                extract: Box::new(|t| {
                    t.max_mom_channels
                        .first()
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
        if cols_cfg.max_mom_right {
            cols.push(ColumnDef {
                header: "Max. Mom. RIGHT".to_string(),
                extract: Box::new(|t| {
                    t.max_mom_channels
                        .get(1)
                        .or_else(|| t.max_mom_channels.first())
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    }

    if expand_surround && (cols_cfg.max_short_term_left || cols_cfg.max_short_term_right) {
        for (idx, ch) in rep_channels.iter().enumerate() {
            let lbl = format!("Max. Short-Term {}", ch.short_label());
            cols.push(ColumnDef {
                header: lbl,
                extract: Box::new(move |t| {
                    t.max_short_term_channels
                        .get(idx)
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    } else {
        if cols_cfg.max_short_term_left {
            cols.push(ColumnDef {
                header: "Max. Short-Term LEFT".to_string(),
                extract: Box::new(|t| {
                    t.max_short_term_channels
                        .first()
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
        if cols_cfg.max_short_term_right {
            cols.push(ColumnDef {
                header: "Max. Short-Term RIGHT".to_string(),
                extract: Box::new(|t| {
                    t.max_short_term_channels
                        .get(1)
                        .or_else(|| t.max_short_term_channels.first())
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    }

    // Group 6: Old Skool
    if cols_cfg.max_sppm_joint {
        cols.push(ColumnDef {
            header: "Max. SPPM".to_string(),
            extract: Box::new(|t| format!("{:.2}", t.max_sppm_joint)),
        });
    }
    if expand_surround && (cols_cfg.max_sppm_left || cols_cfg.max_sppm_right) {
        for (idx, ch) in rep_channels.iter().enumerate() {
            let lbl = format!("Max. SPPM {}", ch.short_label());
            cols.push(ColumnDef {
                header: lbl,
                extract: Box::new(move |t| {
                    t.max_sppm_channels
                        .get(idx)
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    } else {
        if cols_cfg.max_sppm_left {
            cols.push(ColumnDef {
                header: "Max. SPPM LEFT".to_string(),
                extract: Box::new(|t| {
                    t.max_sppm_channels
                        .first()
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
        if cols_cfg.max_sppm_right {
            cols.push(ColumnDef {
                header: "Max. SPPM RIGHT".to_string(),
                extract: Box::new(|t| {
                    t.max_sppm_channels
                        .get(1)
                        .or_else(|| t.max_sppm_channels.first())
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    }

    if expand_surround && (cols_cfg.rms_left || cols_cfg.rms_right) {
        for (idx, ch) in rep_channels.iter().enumerate() {
            let lbl = format!("RMS {}", ch.short_label());
            cols.push(ColumnDef {
                header: lbl,
                extract: Box::new(move |t| {
                    t.rms_channels
                        .get(idx)
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    } else {
        if cols_cfg.rms_left {
            cols.push(ColumnDef {
                header: "RMS LEFT".to_string(),
                extract: Box::new(|t| {
                    t.rms_channels
                        .first()
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
        if cols_cfg.rms_right {
            cols.push(ColumnDef {
                header: "RMS RIGHT".to_string(),
                extract: Box::new(|t| {
                    t.rms_channels
                        .get(1)
                        .or_else(|| t.rms_channels.first())
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string())
                }),
            });
        }
    }

    // Group 7: Ratios
    if cols_cfg.plr {
        cols.push(ColumnDef {
            header: "PLR".to_string(),
            extract: Box::new(|t| format!("{:.2}", t.plr)),
        });
    }
    if cols_cfg.min_psr {
        cols.push(ColumnDef {
            header: "Min. PSR".to_string(),
            extract: Box::new(|t| format!("{:.2}", t.min_psr)),
        });
    }

    // Precompute cell strings and max visual widths per column
    let num_cols = cols.len();
    let mut col_widths: Vec<usize> = cols.iter().map(|c| visual_width(&c.header)).collect();
    let mut matrix: Vec<Vec<String>> = Vec::with_capacity(tracks.len());

    for track in tracks {
        let mut row = Vec::with_capacity(num_cols);
        for (c_idx, col) in cols.iter().enumerate() {
            let val = (col.extract)(track);
            let w = visual_width(&val);
            if w > col_widths[c_idx] {
                col_widths[c_idx] = w;
            }
            row.push(val);
        }
        matrix.push(row);
    }

    let album_name = folder_display_name(folder_path, "Album");

    let mut lines: Vec<String> = Vec::with_capacity(tracks.len() + 8);
    lines.push(String::new());
    lines.push(album_name);
    lines.push(String::new());

    // Header row
    let mut hdr_line = String::new();
    for (c_idx, col) in cols.iter().enumerate() {
        hdr_line.push_str(&pad_left_visual(&col.header, col_widths[c_idx]));
        hdr_line.push_str(" | ");
    }
    lines.push(hdr_line);
    lines.push(String::new());

    // Track rows
    for row in &matrix {
        let mut row_line = String::new();
        for (c_idx, cell) in row.iter().enumerate() {
            row_line.push_str(&pad_left_visual(cell, col_widths[c_idx]));
            row_line.push_str(" | ");
        }
        lines.push(row_line);
    }

    lines.push(String::new());
    lines.push(format!("Number of EP/Album Files: {}", tracks.len()));
    let official_dr = compute_official_album_dr(tracks);
    lines.push(format!("Official EP/Album DR: {}", official_dr));

    lines.join("\n")
}

#[derive(Debug, Clone)]
pub struct FolderTreeNode {
    pub path: PathBuf,
    pub display_name: String,
    pub tracks: Vec<TrackAnalysis>,
    pub children: Vec<FolderTreeNode>,
}

impl FolderTreeNode {
    pub fn total_track_count(&self) -> usize {
        self.tracks.len() + self.children.iter().map(|c| c.total_track_count()).sum::<usize>()
    }

    pub fn non_empty_folder_count(&self) -> usize {
        usize::from(!self.tracks.is_empty())
            + self
                .children
                .iter()
                .map(|c| c.non_empty_folder_count())
                .sum::<usize>()
    }

    pub fn sum_dr_raw(&self) -> f64 {
        let local: f64 = self.tracks.iter().map(|t| t.dr_track_raw).sum();
        local + self.children.iter().map(|c| c.sum_dr_raw()).sum::<f64>()
    }

    pub fn official_dr(&self) -> i32 {
        let n = self.total_track_count();
        if n == 0 {
            0
        } else {
            (self.sum_dr_raw() / (n as f64)).round() as i32
        }
    }
}

fn track_word(n: usize) -> &'static str {
    if n == 1 {
        "track"
    } else {
        "tracks"
    }
}

fn folder_word(n: usize) -> &'static str {
    if n == 1 {
        "folder"
    } else {
        "folders"
    }
}

fn collect_non_empty_sections<'a>(
    node: &'a FolderTreeNode,
    rel_path: &str,
    out: &mut Vec<(String, &'a FolderTreeNode)>,
) {
    if !node.tracks.is_empty() {
        out.push((rel_path.to_string(), node));
    }
    for child in &node.children {
        let next_rel = if rel_path.is_empty() {
            child.display_name.clone()
        } else {
            format!("{}/{}", rel_path, child.display_name)
        };
        collect_non_empty_sections(child, &next_rel, out);
    }
}

fn append_folder_sections(
    out: &mut Vec<String>,
    root: &FolderTreeNode,
    style: FolderLogStyle,
    cols_cfg: &FolderColumnsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
) {
    let rule_dash = "-".repeat(80);
    let mut sections = Vec::new();
    collect_non_empty_sections(root, &root.display_name, &mut sections);

    for (rel_path, node) in sections {
        let n = node.tracks.len();
        let local_dr = compute_official_album_dr(&node.tracks);
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(rule_dash.clone());
        out.push(format!(
            "Directory: {}  (DR{} | {} {})",
            rel_path,
            local_dr,
            n,
            track_word(n)
        ));
        out.push(rule_dash.clone());
        out.push(format_folder_log(
            &node.path,
            &node.tracks,
            style,
            cols_cfg,
            name_opts,
            surround_mode,
        ));
    }
}

pub fn format_recursive_folder_log(
    root: &FolderTreeNode,
    style: FolderLogStyle,
    cols_cfg: &FolderColumnsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
) -> String {
    let rule_eq = "=".repeat(80);
    let total_tracks = root.total_track_count();
    let folder_cnt = root.non_empty_folder_count();
    let overall_dr = root.official_dr();

    let mut out = Vec::new();
    append_folder_sections(&mut out, root, style, cols_cfg, name_opts, surround_mode);

    if !out.is_empty() {
        out.push(String::new());
    }
    out.push(rule_eq.clone());
    out.push(format!(
        "Scan Summary: {} {} | {} {} | Official Overall DR: {}",
        folder_cnt,
        folder_word(folder_cnt),
        total_tracks,
        track_word(total_tracks),
        overall_dr
    ));
    out.push(rule_eq);

    out.join("\n")
}

pub fn format_combined_multi_log(
    roots: &[FolderTreeNode],
    loose_tracks: &[TrackAnalysis],
    style: FolderLogStyle,
    cols_cfg: &FolderColumnsConfig,
    single_rows: &SingleFileRowsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
) -> String {
    let rule_dash = "-".repeat(80);
    let rule_eq = "=".repeat(80);

    let folder_cnt: usize = roots.iter().map(|r| r.non_empty_folder_count()).sum();
    let folder_tracks: usize = roots.iter().map(|r| r.total_track_count()).sum();
    let total_tracks = folder_tracks + loose_tracks.len();
    let sum_dr: f64 = roots.iter().map(|r| r.sum_dr_raw()).sum::<f64>()
        + loose_tracks.iter().map(|t| t.dr_track_raw).sum::<f64>();
    let overall_dr = if total_tracks == 0 {
        0
    } else {
        (sum_dr / (total_tracks as f64)).round() as i32
    };

    let mut out = Vec::new();
    out.push(format!(
        "Combined Batch Analysis: {} {} | {} {} | Official Overall DR: {}",
        folder_cnt,
        folder_word(folder_cnt),
        total_tracks,
        track_word(total_tracks),
        overall_dr
    ));
    out.push(rule_eq.clone());

    for root in roots {
        append_folder_sections(&mut out, root, style, cols_cfg, name_opts, surround_mode);
    }

    if !loose_tracks.is_empty() {
        out.push(String::new());
        out.push(rule_dash.clone());
        out.push(format!(
            "Individual Files ({} {})",
            loose_tracks.len(),
            track_word(loose_tracks.len())
        ));
        out.push(rule_dash);
        for t in loose_tracks {
            out.push(String::new());
            out.push(format_single_file_log(t, single_rows, name_opts));
        }
    }

    out.push(String::new());
    out.push(rule_eq.clone());
    out.push(format!(
        "Combined Summary: {} {} | {} {} | Official Overall DR: {}",
        folder_cnt,
        folder_word(folder_cnt),
        total_tracks,
        track_word(total_tracks),
        overall_dr
    ));
    out.push(rule_eq);

    out.join("\n")
}
