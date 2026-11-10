use crate::config::{FolderColumnsConfig, SurroundFolderMode};
use crate::dsp::TrackAnalysis;
use crate::formatter::{format_sample_rate, pad_left_visual, visual_width};
use std::path::Path;

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
    cols_cfg: &FolderColumnsConfig,
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
        cols.push(ColumnDef {
            header: "File Name".to_string(),
            extract: Box::new(|t| t.file_stem.clone()),
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

    let clean_folder = std::fs::canonicalize(folder_path)
        .map(|p| {
            let s = p.to_string_lossy().to_string();
            s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
        })
        .unwrap_or_else(|_| folder_path.to_string_lossy().to_string());

    let mut lines: Vec<String> = Vec::with_capacity(tracks.len() + 8);
    lines.push(String::new());
    lines.push(format!("Folder Path:   {}", clean_folder));
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
