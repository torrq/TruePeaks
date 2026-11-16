use crate::config::{
    FolderColumnId, FolderColumnsConfig, FolderLogStyle, LogFilenameOptions, SingleFileRowsConfig,
    SurroundFolderMode,
};
use crate::decoder::SpeakerChannel;
use crate::dsp::TrackAnalysis;
use crate::formatter::single_log::format_single_file_log;
use crate::formatter::{
    folder_display_name, format_log_track_name, format_sample_rate, pad_left_visual, visual_width,
};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColumnAlign {
    Left,
    Right,
}

struct ColumnDef {
    id: FolderColumnId,
    header: String,
    align: ColumnAlign,
    min_width: usize,
    extract: Box<dyn Fn(&TrackAnalysis) -> String>,
}

fn pad_right_visual(s: &str, target_width: usize) -> String {
    let w = visual_width(s);
    if w >= target_width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(target_width - w))
    }
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
    format_folder_log_with_header(
        folder_path,
        tracks,
        style,
        cols_cfg,
        name_opts,
        surround_mode,
        true,
    )
}

fn format_folder_log_with_header(
    folder_path: &Path,
    tracks: &[TrackAnalysis],
    style: FolderLogStyle,
    cols_cfg: &FolderColumnsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
    include_header: bool,
) -> String {
    match style {
        FolderLogStyle::TruePeaksTable => format_truepeaks_table(
            folder_path,
            tracks,
            cols_cfg,
            name_opts,
            surround_mode,
            include_header,
        ),
        FolderLogStyle::ClassicFooDr => format_classic_foo_dr(
            folder_path,
            tracks,
            cols_cfg,
            name_opts,
            surround_mode,
            include_header,
        ),
        FolderLogStyle::CompactMastering => format_compact_mastering(
            folder_path,
            tracks,
            cols_cfg,
            name_opts,
            surround_mode,
            include_header,
        ),
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

fn format_unit_suffix(v: f64, unit: &str, signed: bool) -> String {
    if !v.is_finite() {
        format!("-inf {}", unit)
    } else if signed {
        format!("{:+.2} {}", v, unit)
    } else {
        format!("{:.2} {}", v, unit)
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

#[derive(Clone, Copy)]
enum SurroundGroup {
    Dr = 0,
    MaxTpl = 1,
    MaxMom = 2,
    MaxShortTerm = 3,
    MaxSppm = 4,
    Rms = 5,
}

fn surround_group_for(col: FolderColumnId) -> Option<SurroundGroup> {
    match col {
        FolderColumnId::DrLeft | FolderColumnId::DrRight => Some(SurroundGroup::Dr),
        FolderColumnId::MaxTplLeft | FolderColumnId::MaxTplRight => Some(SurroundGroup::MaxTpl),
        FolderColumnId::MaxMomLeft | FolderColumnId::MaxMomRight => Some(SurroundGroup::MaxMom),
        FolderColumnId::MaxShortTermLeft | FolderColumnId::MaxShortTermRight => {
            Some(SurroundGroup::MaxShortTerm)
        }
        FolderColumnId::MaxSppmLeft | FolderColumnId::MaxSppmRight => Some(SurroundGroup::MaxSppm),
        FolderColumnId::RmsLeft | FolderColumnId::RmsRight => Some(SurroundGroup::Rms),
        _ => None,
    }
}

fn build_columns_for_style(
    style: FolderLogStyle,
    cols_cfg: &FolderColumnsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
    tracks: &[TrackAnalysis],
) -> Vec<ColumnDef> {
    let max_channels = tracks
        .iter()
        .map(|t| t.channels.len())
        .max()
        .unwrap_or(2);
    let expand_surround =
        max_channels > 2 && surround_mode == SurroundFolderMode::ExpandPerChannel;
    let rep_channels: Vec<SpeakerChannel> = tracks
        .iter()
        .find(|t| t.channels.len() == max_channels)
        .map(|t| t.channels.clone())
        .unwrap_or_default();

    let mut active_ids = cols_cfg.active_ordered();
    if active_ids.is_empty() {
        active_ids = FolderColumnsConfig::for_style(style).active_ordered();
    }

    let mut expanded_groups = [false; 6];
    let mut cols: Vec<ColumnDef> = Vec::with_capacity(active_ids.len() + 6);

    for col_id in active_ids {
        if expand_surround {
            if let Some(grp) = surround_group_for(col_id) {
                let g_idx = grp as usize;
                if expanded_groups[g_idx] {
                    continue;
                }
                expanded_groups[g_idx] = true;
                for (idx, ch) in rep_channels.iter().enumerate() {
                    let short = ch.short_label();
                    match (style, grp) {
                        (FolderLogStyle::TruePeaksTable, SurroundGroup::Dr) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("DR {}", short),
                                align: ColumnAlign::Right,
                                min_width: 0,
                                extract: Box::new(move |t| {
                                    t.dr_channels
                                        .get(idx)
                                        .map(|v| format!("{:.2}", v))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::ClassicFooDr, SurroundGroup::Dr) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("DR {}", short),
                                align: ColumnAlign::Right,
                                min_width: 8,
                                extract: Box::new(move |t| {
                                    t.dr_channels
                                        .get(idx)
                                        .map(|v| format_db_suffix(*v))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::CompactMastering, SurroundGroup::Dr) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("DR {}", short),
                                align: ColumnAlign::Right,
                                min_width: 7,
                                extract: Box::new(move |t| {
                                    t.dr_channels
                                        .get(idx)
                                        .map(|v| format_db_plain(*v, false))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::TruePeaksTable, SurroundGroup::MaxTpl) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("Max. TPL {}", short),
                                align: ColumnAlign::Right,
                                min_width: 0,
                                extract: Box::new(move |t| {
                                    t.max_tpl_channels
                                        .get(idx)
                                        .map(|v| format!("{:+.2}", v))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::ClassicFooDr, SurroundGroup::MaxTpl) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("TPL {}", short),
                                align: ColumnAlign::Right,
                                min_width: 9,
                                extract: Box::new(move |t| {
                                    t.max_tpl_channels
                                        .get(idx)
                                        .map(|v| format_unit_suffix(*v, "dBTP", true))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::CompactMastering, SurroundGroup::MaxTpl) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("TPL {}", short),
                                align: ColumnAlign::Right,
                                min_width: 8,
                                extract: Box::new(move |t| {
                                    t.max_tpl_channels
                                        .get(idx)
                                        .map(|v| format_db_plain(*v, true))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::TruePeaksTable, SurroundGroup::MaxMom) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("Max. Mom. {}", short),
                                align: ColumnAlign::Right,
                                min_width: 0,
                                extract: Box::new(move |t| {
                                    t.max_mom_channels
                                        .get(idx)
                                        .map(|v| format!("{:.2}", v))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::ClassicFooDr, SurroundGroup::MaxMom) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("Mom. {}", short),
                                align: ColumnAlign::Right,
                                min_width: 10,
                                extract: Box::new(move |t| {
                                    t.max_mom_channels
                                        .get(idx)
                                        .map(|v| format_unit_suffix(*v, "LUFS", false))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::CompactMastering, SurroundGroup::MaxMom) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("Mom. {}", short),
                                align: ColumnAlign::Right,
                                min_width: 8,
                                extract: Box::new(move |t| {
                                    t.max_mom_channels
                                        .get(idx)
                                        .map(|v| format_db_plain(*v, false))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::TruePeaksTable, SurroundGroup::MaxShortTerm) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("Max. Short-Term {}", short),
                                align: ColumnAlign::Right,
                                min_width: 0,
                                extract: Box::new(move |t| {
                                    t.max_short_term_channels
                                        .get(idx)
                                        .map(|v| format!("{:.2}", v))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::ClassicFooDr, SurroundGroup::MaxShortTerm) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("ST {}", short),
                                align: ColumnAlign::Right,
                                min_width: 10,
                                extract: Box::new(move |t| {
                                    t.max_short_term_channels
                                        .get(idx)
                                        .map(|v| format_unit_suffix(*v, "LUFS", false))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::CompactMastering, SurroundGroup::MaxShortTerm) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("ST {}", short),
                                align: ColumnAlign::Right,
                                min_width: 8,
                                extract: Box::new(move |t| {
                                    t.max_short_term_channels
                                        .get(idx)
                                        .map(|v| format_db_plain(*v, false))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::TruePeaksTable, SurroundGroup::MaxSppm) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("Max. SPPM {}", short),
                                align: ColumnAlign::Right,
                                min_width: 0,
                                extract: Box::new(move |t| {
                                    t.max_sppm_channels
                                        .get(idx)
                                        .map(|v| format!("{:.2}", v))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::ClassicFooDr, SurroundGroup::MaxSppm) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("Peak {}", short),
                                align: ColumnAlign::Right,
                                min_width: 9,
                                extract: Box::new(move |t| {
                                    t.max_sppm_channels
                                        .get(idx)
                                        .map(|v| format_db_suffix(*v))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::CompactMastering, SurroundGroup::MaxSppm) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("Peak {}", short),
                                align: ColumnAlign::Right,
                                min_width: 8,
                                extract: Box::new(move |t| {
                                    t.max_sppm_channels
                                        .get(idx)
                                        .map(|v| format_db_plain(*v, false))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::TruePeaksTable, SurroundGroup::Rms) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("RMS {}", short),
                                align: ColumnAlign::Right,
                                min_width: 0,
                                extract: Box::new(move |t| {
                                    t.rms_channels
                                        .get(idx)
                                        .map(|v| format!("{:.2}", v))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::ClassicFooDr, SurroundGroup::Rms) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("RMS {}", short),
                                align: ColumnAlign::Right,
                                min_width: 9,
                                extract: Box::new(move |t| {
                                    t.rms_channels
                                        .get(idx)
                                        .map(|v| format_db_suffix(*v))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                        (FolderLogStyle::CompactMastering, SurroundGroup::Rms) => {
                            cols.push(ColumnDef {
                                id: col_id,
                                header: format!("RMS {}", short),
                                align: ColumnAlign::Right,
                                min_width: 8,
                                extract: Box::new(move |t| {
                                    t.rms_channels
                                        .get(idx)
                                        .map(|v| format_db_plain(*v, false))
                                        .unwrap_or_else(|| "-".to_string())
                                }),
                            });
                        }
                    }
                }
                continue;
            }
        }

        cols.push(build_single_column_def(style, col_id, name_opts));
    }

    cols
}

fn build_single_column_def(
    style: FolderLogStyle,
    col_id: FolderColumnId,
    name_opts: &LogFilenameOptions,
) -> ColumnDef {
    let name_opts_copy = *name_opts;
    match (style, col_id) {
        // File Name / Track
        (FolderLogStyle::TruePeaksTable, FolderColumnId::FileName) => ColumnDef {
            id: col_id,
            header: "File Name".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(move |t| format_log_track_name(&t.file_stem, &name_opts_copy)),
        },
        (_, FolderColumnId::FileName) => ColumnDef {
            id: col_id,
            header: "Track".to_string(),
            align: ColumnAlign::Left,
            min_width: 5,
            extract: Box::new(move |t| format_log_track_name(&t.file_stem, &name_opts_copy)),
        },

        // Duration / Time
        (FolderLogStyle::CompactMastering, FolderColumnId::Duration) => ColumnDef {
            id: col_id,
            header: "Time".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| format_duration_mmss(t.duration_secs())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::Duration) => ColumnDef {
            id: col_id,
            header: "Duration".to_string(),
            align: ColumnAlign::Right,
            min_width: 8,
            extract: Box::new(|t| format_duration_mmss(t.duration_secs())),
        },
        (FolderLogStyle::TruePeaksTable, FolderColumnId::Duration) => ColumnDef {
            id: col_id,
            header: "Duration".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| format_duration_mmss(t.duration_secs())),
        },

        // Format
        (_, FolderColumnId::FileFormat) => ColumnDef {
            id: col_id,
            header: "Format".to_string(),
            align: ColumnAlign::Right,
            min_width: if style == FolderLogStyle::TruePeaksTable { 0 } else { 6 },
            extract: Box::new(|t| t.file_format.clone()),
        },

        // Sample Rate
        (_, FolderColumnId::SampleRate) => ColumnDef {
            id: col_id,
            header: "SR".to_string(),
            align: ColumnAlign::Right,
            min_width: if style == FolderLogStyle::TruePeaksTable { 0 } else { 6 },
            extract: Box::new(|t| format_sample_rate(t.sample_rate)),
        },

        // Word Length
        (FolderLogStyle::TruePeaksTable, FolderColumnId::WordLength) => ColumnDef {
            id: col_id,
            header: "Word Length".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.word_length.map(|w| w.to_string()).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::WordLength) => ColumnDef {
            id: col_id,
            header: "Word".to_string(),
            align: ColumnAlign::Right,
            min_width: 6,
            extract: Box::new(|t| t.word_length.map(|w| format!("{}-bit", w)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::WordLength) => ColumnDef {
            id: col_id,
            header: "Word".to_string(),
            align: ColumnAlign::Right,
            min_width: 6,
            extract: Box::new(|t| t.word_length.map(|w| w.to_string()).unwrap_or_else(|| "-".to_string())),
        },

        // Bits Used
        (FolderLogStyle::TruePeaksTable, FolderColumnId::BitsUsed) => ColumnDef {
            id: col_id,
            header: "Bits Used".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.bits_used.map(|b| b.to_string()).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::BitsUsed) => ColumnDef {
            id: col_id,
            header: "Bits Used".to_string(),
            align: ColumnAlign::Right,
            min_width: 9,
            extract: Box::new(|t| t.bits_used.map(|b| format!("{}-bit", b)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::BitsUsed) => ColumnDef {
            id: col_id,
            header: "Bits".to_string(),
            align: ColumnAlign::Right,
            min_width: 6,
            extract: Box::new(|t| t.bits_used.map(|b| b.to_string()).unwrap_or_else(|| "-".to_string())),
        },

        // DR (PMF)
        (FolderLogStyle::TruePeaksTable, FolderColumnId::DrPmf) => ColumnDef {
            id: col_id,
            header: "DR (PMF)".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.dr_track_rounded.to_string()),
        },
        (_, FolderColumnId::DrPmf) => ColumnDef {
            id: col_id,
            header: "DR".to_string(),
            align: ColumnAlign::Left,
            min_width: 6,
            extract: Box::new(|t| format!("DR{}", t.dr_track_rounded)),
        },

        // DR LEFT / RIGHT
        (FolderLogStyle::TruePeaksTable, FolderColumnId::DrLeft) => ColumnDef {
            id: col_id,
            header: "DR LEFT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.dr_channels.first().map(|v| format!("{:.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::DrLeft) => ColumnDef {
            id: col_id,
            header: "DR L".to_string(),
            align: ColumnAlign::Right,
            min_width: 8,
            extract: Box::new(|t| t.dr_channels.first().map(|v| format_db_suffix(*v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::DrLeft) => ColumnDef {
            id: col_id,
            header: "DR L".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| t.dr_channels.first().map(|v| format_db_plain(*v, false)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::TruePeaksTable, FolderColumnId::DrRight) => ColumnDef {
            id: col_id,
            header: "DR RIGHT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.dr_channels.get(1).or_else(|| t.dr_channels.first()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::DrRight) => ColumnDef {
            id: col_id,
            header: "DR R".to_string(),
            align: ColumnAlign::Right,
            min_width: 8,
            extract: Box::new(|t| t.dr_channels.get(1).or_else(|| t.dr_channels.first()).map(|v| format_db_suffix(*v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::DrRight) => ColumnDef {
            id: col_id,
            header: "DR R".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| t.dr_channels.get(1).or_else(|| t.dr_channels.first()).map(|v| format_db_plain(*v, false)).unwrap_or_else(|| "-".to_string())),
        },

        // Max. TPL (ITU) & LEFT / RIGHT
        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxTplItu) => ColumnDef {
            id: col_id,
            header: "Max. TPL".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| format!("{:+.2}", t.max_tpl_joint)),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxTplItu) => ColumnDef {
            id: col_id,
            header: "TPL".to_string(),
            align: ColumnAlign::Right,
            min_width: 9,
            extract: Box::new(|t| format_unit_suffix(t.max_tpl_joint, "dBTP", true)),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxTplItu) => ColumnDef {
            id: col_id,
            header: "TPL dBTP".to_string(),
            align: ColumnAlign::Right,
            min_width: 9,
            extract: Box::new(|t| format_db_plain(t.max_tpl_joint, true)),
        },
        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxTplLeft) => ColumnDef {
            id: col_id,
            header: "Max. TPL LEFT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.max_tpl_channels.first().map(|v| format!("{:+.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxTplLeft) => ColumnDef {
            id: col_id,
            header: "TPL L".to_string(),
            align: ColumnAlign::Right,
            min_width: 9,
            extract: Box::new(|t| t.max_tpl_channels.first().map(|v| format_unit_suffix(*v, "dBTP", true)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxTplLeft) => ColumnDef {
            id: col_id,
            header: "TPL L".to_string(),
            align: ColumnAlign::Right,
            min_width: 8,
            extract: Box::new(|t| t.max_tpl_channels.first().map(|v| format_db_plain(*v, true)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxTplRight) => ColumnDef {
            id: col_id,
            header: "Max. TPL RIGHT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.max_tpl_channels.get(1).or_else(|| t.max_tpl_channels.first()).map(|v| format!("{:+.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxTplRight) => ColumnDef {
            id: col_id,
            header: "TPL R".to_string(),
            align: ColumnAlign::Right,
            min_width: 9,
            extract: Box::new(|t| t.max_tpl_channels.get(1).or_else(|| t.max_tpl_channels.first()).map(|v| format_unit_suffix(*v, "dBTP", true)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxTplRight) => ColumnDef {
            id: col_id,
            header: "TPL R".to_string(),
            align: ColumnAlign::Right,
            min_width: 8,
            extract: Box::new(|t| t.max_tpl_channels.get(1).or_else(|| t.max_tpl_channels.first()).map(|v| format_db_plain(*v, true)).unwrap_or_else(|| "-".to_string())),
        },

        // LUFS group
        (FolderLogStyle::TruePeaksTable, FolderColumnId::Lufsi) => ColumnDef {
            id: col_id,
            header: "LUFSi".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| format!("{:.2}", t.integrated_lufs)),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::Lufsi) => ColumnDef {
            id: col_id,
            header: "LUFSi".to_string(),
            align: ColumnAlign::Right,
            min_width: 10,
            extract: Box::new(|t| format_unit_suffix(t.integrated_lufs, "LUFS", false)),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::Lufsi) => ColumnDef {
            id: col_id,
            header: "LUFSi".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| format_db_plain(t.integrated_lufs, false)),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::Lra) => ColumnDef {
            id: col_id,
            header: "LRA".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| format!("{:.2}", t.lra)),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::Lra) => ColumnDef {
            id: col_id,
            header: "LRA".to_string(),
            align: ColumnAlign::Right,
            min_width: 8,
            extract: Box::new(|t| format_unit_suffix(t.lra, "LU", false)),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::Lra) => ColumnDef {
            id: col_id,
            header: "LRA".to_string(),
            align: ColumnAlign::Right,
            min_width: 6,
            extract: Box::new(|t| format_db_plain(t.lra, false)),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxMomItu) => ColumnDef {
            id: col_id,
            header: "Max. Mom.".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| format!("{:.2}", t.max_mom_joint)),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxMomItu) => ColumnDef {
            id: col_id,
            header: "Mom.".to_string(),
            align: ColumnAlign::Right,
            min_width: 10,
            extract: Box::new(|t| format_unit_suffix(t.max_mom_joint, "LUFS", false)),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxMomItu) => ColumnDef {
            id: col_id,
            header: "Mom.".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| format_db_plain(t.max_mom_joint, false)),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxShortTermItu) => ColumnDef {
            id: col_id,
            header: "Max. Short-Term".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| format!("{:.2}", t.max_short_term_joint)),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxShortTermItu) => ColumnDef {
            id: col_id,
            header: "Short-Term".to_string(),
            align: ColumnAlign::Right,
            min_width: 10,
            extract: Box::new(|t| format_unit_suffix(t.max_short_term_joint, "LUFS", false)),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxShortTermItu) => ColumnDef {
            id: col_id,
            header: "Short".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| format_db_plain(t.max_short_term_joint, false)),
        },

        // Channel Specific LUFS
        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxMomLeft) => ColumnDef {
            id: col_id,
            header: "Max. Mom. LEFT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.max_mom_channels.first().map(|v| format!("{:.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxMomLeft) => ColumnDef {
            id: col_id,
            header: "Mom. L".to_string(),
            align: ColumnAlign::Right,
            min_width: 10,
            extract: Box::new(|t| t.max_mom_channels.first().map(|v| format_unit_suffix(*v, "LUFS", false)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxMomLeft) => ColumnDef {
            id: col_id,
            header: "Mom. L".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| t.max_mom_channels.first().map(|v| format_db_plain(*v, false)).unwrap_or_else(|| "-".to_string())),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxMomRight) => ColumnDef {
            id: col_id,
            header: "Max. Mom. RIGHT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.max_mom_channels.get(1).or_else(|| t.max_mom_channels.first()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxMomRight) => ColumnDef {
            id: col_id,
            header: "Mom. R".to_string(),
            align: ColumnAlign::Right,
            min_width: 10,
            extract: Box::new(|t| t.max_mom_channels.get(1).or_else(|| t.max_mom_channels.first()).map(|v| format_unit_suffix(*v, "LUFS", false)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxMomRight) => ColumnDef {
            id: col_id,
            header: "Mom. R".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| t.max_mom_channels.get(1).or_else(|| t.max_mom_channels.first()).map(|v| format_db_plain(*v, false)).unwrap_or_else(|| "-".to_string())),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxShortTermLeft) => ColumnDef {
            id: col_id,
            header: "Max. Short-Term LEFT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.max_short_term_channels.first().map(|v| format!("{:.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxShortTermLeft) => ColumnDef {
            id: col_id,
            header: "ST L".to_string(),
            align: ColumnAlign::Right,
            min_width: 10,
            extract: Box::new(|t| t.max_short_term_channels.first().map(|v| format_unit_suffix(*v, "LUFS", false)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxShortTermLeft) => ColumnDef {
            id: col_id,
            header: "ST L".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| t.max_short_term_channels.first().map(|v| format_db_plain(*v, false)).unwrap_or_else(|| "-".to_string())),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxShortTermRight) => ColumnDef {
            id: col_id,
            header: "Max. Short-Term RIGHT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.max_short_term_channels.get(1).or_else(|| t.max_short_term_channels.first()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxShortTermRight) => ColumnDef {
            id: col_id,
            header: "ST R".to_string(),
            align: ColumnAlign::Right,
            min_width: 10,
            extract: Box::new(|t| t.max_short_term_channels.get(1).or_else(|| t.max_short_term_channels.first()).map(|v| format_unit_suffix(*v, "LUFS", false)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxShortTermRight) => ColumnDef {
            id: col_id,
            header: "ST R".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| t.max_short_term_channels.get(1).or_else(|| t.max_short_term_channels.first()).map(|v| format_db_plain(*v, false)).unwrap_or_else(|| "-".to_string())),
        },

        // Old Skool (Max. SPPM & RMS)
        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxSppmJoint) => ColumnDef {
            id: col_id,
            header: "Max. SPPM".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| format!("{:.2}", t.max_sppm_joint)),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxSppmJoint) => ColumnDef {
            id: col_id,
            header: "Peak".to_string(),
            align: ColumnAlign::Right,
            min_width: 9,
            extract: Box::new(|t| format_db_suffix(t.max_sppm_joint)),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxSppmJoint) => ColumnDef {
            id: col_id,
            header: "Peak dB".to_string(),
            align: ColumnAlign::Right,
            min_width: 8,
            extract: Box::new(|t| format_db_plain(t.max_sppm_joint, false)),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxSppmLeft) => ColumnDef {
            id: col_id,
            header: "Max. SPPM LEFT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.max_sppm_channels.first().map(|v| format!("{:.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxSppmLeft) => ColumnDef {
            id: col_id,
            header: "Peak L".to_string(),
            align: ColumnAlign::Right,
            min_width: 9,
            extract: Box::new(|t| t.max_sppm_channels.first().map(|v| format_db_suffix(*v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxSppmLeft) => ColumnDef {
            id: col_id,
            header: "Peak L".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| t.max_sppm_channels.first().map(|v| format_db_plain(*v, false)).unwrap_or_else(|| "-".to_string())),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::MaxSppmRight) => ColumnDef {
            id: col_id,
            header: "Max. SPPM RIGHT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.max_sppm_channels.get(1).or_else(|| t.max_sppm_channels.first()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MaxSppmRight) => ColumnDef {
            id: col_id,
            header: "Peak R".to_string(),
            align: ColumnAlign::Right,
            min_width: 9,
            extract: Box::new(|t| t.max_sppm_channels.get(1).or_else(|| t.max_sppm_channels.first()).map(|v| format_db_suffix(*v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MaxSppmRight) => ColumnDef {
            id: col_id,
            header: "Peak R".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| t.max_sppm_channels.get(1).or_else(|| t.max_sppm_channels.first()).map(|v| format_db_plain(*v, false)).unwrap_or_else(|| "-".to_string())),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::RmsJoint) => ColumnDef {
            id: col_id,
            header: "RMS".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| format!("{:.2}", t.combined_rms_db())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::RmsJoint) => ColumnDef {
            id: col_id,
            header: "RMS".to_string(),
            align: ColumnAlign::Right,
            min_width: 9,
            extract: Box::new(|t| format_db_suffix(t.combined_rms_db())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::RmsJoint) => ColumnDef {
            id: col_id,
            header: "RMS dB".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| format_db_plain(t.combined_rms_db(), false)),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::RmsLeft) => ColumnDef {
            id: col_id,
            header: "RMS LEFT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.rms_channels.first().map(|v| format!("{:.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::RmsLeft) => ColumnDef {
            id: col_id,
            header: "RMS L".to_string(),
            align: ColumnAlign::Right,
            min_width: 9,
            extract: Box::new(|t| t.rms_channels.first().map(|v| format_db_suffix(*v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::RmsLeft) => ColumnDef {
            id: col_id,
            header: "RMS L".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| t.rms_channels.first().map(|v| format_db_plain(*v, false)).unwrap_or_else(|| "-".to_string())),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::RmsRight) => ColumnDef {
            id: col_id,
            header: "RMS RIGHT".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| t.rms_channels.get(1).or_else(|| t.rms_channels.first()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::RmsRight) => ColumnDef {
            id: col_id,
            header: "RMS R".to_string(),
            align: ColumnAlign::Right,
            min_width: 9,
            extract: Box::new(|t| t.rms_channels.get(1).or_else(|| t.rms_channels.first()).map(|v| format_db_suffix(*v)).unwrap_or_else(|| "-".to_string())),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::RmsRight) => ColumnDef {
            id: col_id,
            header: "RMS R".to_string(),
            align: ColumnAlign::Right,
            min_width: 7,
            extract: Box::new(|t| t.rms_channels.get(1).or_else(|| t.rms_channels.first()).map(|v| format_db_plain(*v, false)).unwrap_or_else(|| "-".to_string())),
        },

        // Ratios
        (FolderLogStyle::TruePeaksTable, FolderColumnId::Plr) => ColumnDef {
            id: col_id,
            header: "PLR".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| format!("{:.2}", t.plr)),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::Plr) => ColumnDef {
            id: col_id,
            header: "PLR".to_string(),
            align: ColumnAlign::Right,
            min_width: 8,
            extract: Box::new(|t| format_unit_suffix(t.plr, "LU", false)),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::Plr) => ColumnDef {
            id: col_id,
            header: "PLR".to_string(),
            align: ColumnAlign::Right,
            min_width: 6,
            extract: Box::new(|t| format_db_plain(t.plr, false)),
        },

        (FolderLogStyle::TruePeaksTable, FolderColumnId::MinPsr) => ColumnDef {
            id: col_id,
            header: "Min. PSR".to_string(),
            align: ColumnAlign::Right,
            min_width: 0,
            extract: Box::new(|t| format!("{:.2}", t.min_psr)),
        },
        (FolderLogStyle::ClassicFooDr, FolderColumnId::MinPsr) => ColumnDef {
            id: col_id,
            header: "Min. PSR".to_string(),
            align: ColumnAlign::Right,
            min_width: 8,
            extract: Box::new(|t| format_unit_suffix(t.min_psr, "LU", false)),
        },
        (FolderLogStyle::CompactMastering, FolderColumnId::MinPsr) => ColumnDef {
            id: col_id,
            header: "PSR".to_string(),
            align: ColumnAlign::Right,
            min_width: 6,
            extract: Box::new(|t| format_db_plain(t.min_psr, false)),
        },
    }
}

fn render_space_aligned_table(cols: &[ColumnDef], tracks: &[TrackAnalysis]) -> (String, Vec<String>, usize) {
    let num_cols = cols.len();
    let mut col_widths: Vec<usize> = cols
        .iter()
        .map(|c| visual_width(&c.header).max(c.min_width))
        .collect();
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

    let format_row = |cells: &[String]| -> String {
        let mut line = String::new();
        for (c_idx, cell) in cells.iter().enumerate() {
            let col = &cols[c_idx];
            let is_last = c_idx + 1 == num_cols;
            if c_idx > 0 {
                if is_last && col.id == FolderColumnId::FileName {
                    line.push_str("   ");
                } else {
                    line.push_str("  ");
                }
            }
            if is_last && col.align == ColumnAlign::Left {
                line.push_str(cell);
            } else if col.align == ColumnAlign::Left {
                line.push_str(&pad_right_visual(cell, col_widths[c_idx]));
            } else {
                line.push_str(&pad_left_visual(cell, col_widths[c_idx]));
            }
        }
        line
    };

    let hdr_cells: Vec<String> = cols.iter().map(|c| c.header.clone()).collect();
    let hdr_line = format_row(&hdr_cells);
    let mut max_w = visual_width(&hdr_line).max(80);
    let mut row_lines = Vec::with_capacity(matrix.len());
    for row in &matrix {
        let rl = format_row(row);
        max_w = max_w.max(visual_width(&rl));
        row_lines.push(rl);
    }

    (hdr_line, row_lines, max_w)
}

fn format_classic_foo_dr(
    folder_path: &Path,
    tracks: &[TrackAnalysis],
    cols_cfg: &FolderColumnsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
    include_header: bool,
) -> String {
    let album_name = folder_display_name(folder_path, "Album");
    let cols = build_columns_for_style(
        FolderLogStyle::ClassicFooDr,
        cols_cfg,
        name_opts,
        surround_mode,
        tracks,
    );

    let is_default_classic = cols.len() == 5
        && cols[0].id == FolderColumnId::DrPmf
        && cols[1].id == FolderColumnId::MaxSppmJoint
        && cols[2].id == FolderColumnId::RmsJoint
        && cols[3].id == FolderColumnId::Duration
        && cols[4].id == FolderColumnId::FileName;

    let (hdr_line, row_lines, rule_w) = if is_default_classic {
        let hdr = "DR         Peak         RMS     Duration Track".to_string();
        let mut rlines = Vec::with_capacity(tracks.len());
        for t in tracks {
            let dr_col = format!("DR{}", t.dr_track_rounded);
            let peak_col = format_db_suffix(t.max_sppm_joint);
            let rms_col = format_db_suffix(t.combined_rms_db());
            let dur_col = format_duration_mmss(t.duration_secs());
            let track_name = format_log_track_name(&t.file_stem, name_opts);
            rlines.push(format!(
                "{:<9}{:>9}   {:>9} {:>9} {}",
                dr_col, peak_col, rms_col, dur_col, track_name
            ));
        }
        (hdr, rlines, 80usize)
    } else {
        render_space_aligned_table(&cols, tracks)
    };

    let rule_dash = "-".repeat(rule_w);
    let rule_eq = "=".repeat(rule_w);

    let mut lines: Vec<String> = Vec::with_capacity(tracks.len() + 18);
    if include_header {
        lines.push(rule_dash.clone());
        lines.push(format!("Analyzed: {}", album_name));
        lines.push(rule_dash.clone());
    }
    lines.push(String::new());
    lines.push(hdr_line);
    lines.push(rule_dash.clone());
    lines.extend(row_lines);

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
    cols_cfg: &FolderColumnsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
    include_header: bool,
) -> String {
    let album_name = folder_display_name(folder_path, "Album");
    let official_dr = compute_official_album_dr(tracks);
    let cols = build_columns_for_style(
        FolderLogStyle::CompactMastering,
        cols_cfg,
        name_opts,
        surround_mode,
        tracks,
    );

    let is_default_compact = cols.len() == 7
        && cols[0].id == FolderColumnId::DrPmf
        && cols[1].id == FolderColumnId::MaxSppmJoint
        && cols[2].id == FolderColumnId::MaxTplItu
        && cols[3].id == FolderColumnId::Lufsi
        && cols[4].id == FolderColumnId::MinPsr
        && cols[5].id == FolderColumnId::Duration
        && cols[6].id == FolderColumnId::FileName;

    let (hdr_line, row_lines, rule_w) = if is_default_compact {
        let hdr = format!(
            "{:<6}{:>9}{:>10}{:>8}{:>7}{:>9}   {}",
            "DR", "Peak dB", "TPL dBTP", "LUFSi", "PSR", "Time", "Track"
        );
        let mut rlines = Vec::with_capacity(tracks.len());
        for t in tracks {
            rlines.push(format!(
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
        (hdr, rlines, 80usize)
    } else {
        render_space_aligned_table(&cols, tracks)
    };

    let rule_dash = "-".repeat(rule_w);
    let rule_eq = "=".repeat(rule_w);

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
    if include_header {
        lines.push(rule_eq);
        lines.push(format!("Album:   {}", album_name));
    } else {
        lines.push(String::new());
    }
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
    lines.push(hdr_line);
    lines.push(rule_dash.clone());
    lines.extend(row_lines);

    lines.push(rule_dash);
    lines.join("\n")
}

fn format_truepeaks_table(
    folder_path: &Path,
    tracks: &[TrackAnalysis],
    cols_cfg: &FolderColumnsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
    include_header: bool,
) -> String {
    let cols = build_columns_for_style(
        FolderLogStyle::TruePeaksTable,
        cols_cfg,
        name_opts,
        surround_mode,
        tracks,
    );

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

    let mut lines: Vec<String> = Vec::with_capacity(tracks.len() + 8);
    lines.push(String::new());
    if include_header {
        lines.push(folder_display_name(folder_path, "Album"));
        lines.push(String::new());
    }

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

    if include_header {
        lines.push(String::new());
        lines.push(format!("Number of Files: {}", tracks.len()));
        lines.push(format!("Album DR: {}", compute_official_album_dr(tracks)));
    }

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

fn append_directory_banner(out: &mut Vec<String>, label: &str, tracks: &[TrackAnalysis]) {
    let rule_dash = "-".repeat(80);
    let n = tracks.len();
    let local_dr = compute_official_album_dr(tracks);
    out.push(rule_dash.clone());
    out.push(format!(
        "Directory: {}  (DR{} | {} {})",
        label,
        local_dr,
        n,
        track_word(n)
    ));
    out.push(rule_dash);
}

fn append_sections(
    out: &mut Vec<String>,
    sections: &[(String, &FolderTreeNode)],
    style: FolderLogStyle,
    cols_cfg: &FolderColumnsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
) {
    for (rel_path, node) in sections {
        if !out.is_empty() {
            out.push(String::new());
        }
        append_directory_banner(out, rel_path, &node.tracks);
        out.push(format_folder_log_with_header(
            &node.path,
            &node.tracks,
            style,
            cols_cfg,
            name_opts,
            surround_mode,
            false,
        ));
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
    let mut sections = Vec::new();
    collect_non_empty_sections(root, &root.display_name, &mut sections);
    append_sections(out, &sections, style, cols_cfg, name_opts, surround_mode);
}

pub fn format_recursive_folder_log(
    root: &FolderTreeNode,
    style: FolderLogStyle,
    cols_cfg: &FolderColumnsConfig,
    name_opts: &LogFilenameOptions,
    surround_mode: SurroundFolderMode,
) -> String {
    let mut sections = Vec::new();
    collect_non_empty_sections(root, &root.display_name, &mut sections);
    if sections.len() == 1 {
        let (_, only_node) = sections[0];
        return format_folder_log(
            &only_node.path,
            &only_node.tracks,
            style,
            cols_cfg,
            name_opts,
            surround_mode,
        );
    }

    let rule_eq = "=".repeat(80);
    let total_tracks = root.total_track_count();
    let folder_cnt = sections.len();
    let overall_dr = root.official_dr();

    let mut out = Vec::new();
    append_sections(&mut out, &sections, style, cols_cfg, name_opts, surround_mode);

    if folder_cnt > 1 {
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
    }

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

    if folder_cnt > 1 || !loose_tracks.is_empty() {
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
    }

    out.join("\n")
}
