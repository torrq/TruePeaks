use crate::config::{LogFilenameOptions, SurroundFolderMode};
use crate::dsp::TrackAnalysis;
use crate::formatter::folder_log::compute_official_album_dr;
use crate::formatter::{format_log_track_name, format_sample_rate, format_timestamp_mmss};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Markdown,
    Json,
}

fn track_label(t: &TrackAnalysis, name_opts: &LogFilenameOptions, use_tags: bool) -> String {
    let raw = t.display_name(use_tags);
    format_log_track_name(&raw, name_opts)
}

fn format_duration_sec(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub(crate) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn json_f64_or_null(v: f64) -> String {
    if v.is_finite() {
        format!("{:.3}", v)
    } else {
        "null".to_string()
    }
}

pub fn export_tracks(
    title: &str,
    tracks: &[TrackAnalysis],
    fmt: ExportFormat,
    name_opts: &LogFilenameOptions,
    use_tags: bool,
    _surround_mode: SurroundFolderMode,
) -> String {
    match fmt {
        ExportFormat::Csv => export_csv(tracks, name_opts, use_tags),
        ExportFormat::Markdown => export_markdown(title, tracks, name_opts, use_tags),
        ExportFormat::Json => export_json(title, tracks, name_opts, use_tags),
    }
}

fn export_csv(
    tracks: &[TrackAnalysis],
    name_opts: &LogFilenameOptions,
    use_tags: bool,
) -> String {
    let mut lines = Vec::with_capacity(tracks.len() + 1);
    lines.push(
        "Track,Format,SampleRateHz,WordLength,BitsUsed,Duration,DR_PMF,DR_Raw,MaxTPL_dBTP,TPL_Oversample,MaxSPPM_dBFS,MaxSPPM_At,RMS_dBFS,LUFSi,LRA_LU,MinPSR_LU,MinPSR_At,PLR_LU,ReplayGain_dB,StereoCorr"
            .to_string(),
    );

    for t in tracks {
        let row = [
            csv_escape(&track_label(t, name_opts, use_tags)),
            csv_escape(&t.file_format),
            t.sample_rate.to_string(),
            t.word_length.map(|w| w.to_string()).unwrap_or_default(),
            t.bits_used.map(|b| b.to_string()).unwrap_or_default(),
            format_duration_sec(t.duration_secs()),
            t.dr_track_rounded.to_string(),
            format!("{:.2}", t.dr_track_raw),
            if t.max_tpl_joint.is_finite() {
                format!("{:+.2}", t.max_tpl_joint)
            } else {
                String::new()
            },
            format!("{}x", t.tpl_oversample_factor),
            if t.max_sppm_joint.is_finite() {
                format!("{:.2}", t.max_sppm_joint)
            } else {
                String::new()
            },
            format_timestamp_mmss(t.max_sppm_time_secs),
            if t.combined_rms_db().is_finite() {
                format!("{:.2}", t.combined_rms_db())
            } else {
                String::new()
            },
            if t.integrated_lufs.is_finite() {
                format!("{:.2}", t.integrated_lufs)
            } else {
                String::new()
            },
            format!("{:.2}", t.lra),
            format!("{:.2}", t.min_psr),
            t.min_psr_time_secs
                .map(format_timestamp_mmss)
                .unwrap_or_default(),
            if t.plr.is_finite() {
                format!("{:.2}", t.plr)
            } else {
                String::new()
            },
            t.replay_gain_track_db
                .map(|rg| format!("{:+.2}", rg))
                .unwrap_or_default(),
            t.stereo_correlation
                .map(|r| format!("{:+.3}", r))
                .unwrap_or_default(),
        ];
        lines.push(row.join(","));
    }

    lines.join("\n")
}

fn export_markdown(
    title: &str,
    tracks: &[TrackAnalysis],
    name_opts: &LogFilenameOptions,
    use_tags: bool,
) -> String {
    let album_dr = compute_official_album_dr(tracks);
    let mut lines = Vec::with_capacity(tracks.len() + 8);
    lines.push(format!("### {}", title));
    lines.push(String::new());
    lines.push(format!(
        "- **Official Album DR:** `DR{}`  \n- **Tracks:** `{}`",
        album_dr,
        tracks.len()
    ));
    lines.push(String::new());
    lines.push("| # | Track | Format | SR / Bits | Duration | DR | Max TPL | Max SPPM | LUFSi | Min PSR | LRA | RG Track | Corr |".to_string());
    lines.push("| ---: | :--- | :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |".to_string());

    for (idx, t) in tracks.iter().enumerate() {
        let name = track_label(t, name_opts, use_tags).replace('|', "\\|");
        let bits = match (t.word_length, t.bits_used) {
            (Some(w), Some(b)) if b != w => format!("{}/{}-in-{}", format_sample_rate(t.sample_rate), b, w),
            (Some(w), _) => format!("{} {}-bit", format_sample_rate(t.sample_rate), w),
            _ => format_sample_rate(t.sample_rate),
        };
        let tpl = if t.max_tpl_joint.is_finite() {
            format!("{:+.2} dBTP", t.max_tpl_joint)
        } else {
            "-".to_string()
        };
        let sppm = if t.max_sppm_joint.is_finite() {
            format!("{:.2} dBFS", t.max_sppm_joint)
        } else {
            "-".to_string()
        };
        let rg = t
            .replay_gain_track_db
            .map(|v| format!("{:+.2} dB", v))
            .unwrap_or_else(|| "-".to_string());
        let corr = t
            .stereo_correlation
            .map(|r| format!("{:+.2}", r))
            .unwrap_or_else(|| "-".to_string());
        lines.push(format!(
            "| {} | {} | {} | {} | {} | DR{} | {} | {} | {:.2} | {:.2} | {:.2} | {} | {} |",
            idx + 1,
            name,
            t.file_format,
            bits,
            format_duration_sec(t.duration_secs()),
            t.dr_track_rounded,
            tpl,
            sppm,
            t.integrated_lufs,
            t.min_psr,
            t.lra,
            rg,
            corr,
        ));
    }

    lines.join("\n")
}

fn export_json(
    title: &str,
    tracks: &[TrackAnalysis],
    name_opts: &LogFilenameOptions,
    use_tags: bool,
) -> String {
    let album_dr = compute_official_album_dr(tracks);
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"title\": \"{}\",\n", json_escape(title)));
    out.push_str(&format!("  \"album_dr\": {},\n", album_dr));
    out.push_str(&format!("  \"track_count\": {},\n", tracks.len()));
    out.push_str("  \"tracks\": [\n");
    for (idx, t) in tracks.iter().enumerate() {
        let name = track_label(t, name_opts, use_tags);
        let wl = t
            .word_length
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string());
        let bu = t
            .bits_used
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string());
        let rg = t
            .replay_gain_track_db
            .map(json_f64_or_null)
            .unwrap_or_else(|| "null".to_string());
        let corr = t
            .stereo_correlation
            .map(json_f64_or_null)
            .unwrap_or_else(|| "null".to_string());
        let comma = if idx + 1 < tracks.len() { "," } else { "" };
        out.push_str("    {\n");
        out.push_str(&format!("      \"track\": \"{}\",\n", json_escape(&name)));
        out.push_str(&format!("      \"format\": \"{}\",\n", json_escape(&t.file_format)));
        out.push_str(&format!("      \"sample_rate_hz\": {},\n", t.sample_rate));
        out.push_str(&format!("      \"channels\": {},\n", t.channels.len()));
        out.push_str(&format!("      \"word_length\": {},\n", wl));
        out.push_str(&format!("      \"bits_used\": {},\n", bu));
        out.push_str(&format!("      \"duration_secs\": {:.3},\n", t.duration_secs()));
        out.push_str(&format!("      \"dr_pmf\": {},\n", t.dr_track_rounded));
        out.push_str(&format!("      \"dr_raw\": {:.3},\n", t.dr_track_raw));
        out.push_str(&format!("      \"max_tpl_dbtp\": {},\n", json_f64_or_null(t.max_tpl_joint)));
        out.push_str(&format!("      \"tpl_oversample_factor\": {},\n", t.tpl_oversample_factor));
        out.push_str(&format!("      \"max_sppm_dbfs\": {},\n", json_f64_or_null(t.max_sppm_joint)));
        out.push_str(&format!("      \"max_sppm_time_secs\": {:.3},\n", t.max_sppm_time_secs));
        out.push_str(&format!("      \"rms_aes17_dbfs\": {},\n", json_f64_or_null(t.combined_rms_db())));
        out.push_str(&format!("      \"integrated_lufs\": {},\n", json_f64_or_null(t.integrated_lufs)));
        out.push_str(&format!("      \"lra_lu\": {},\n", json_f64_or_null(t.lra)));
        out.push_str(&format!("      \"min_psr_lu\": {},\n", json_f64_or_null(t.min_psr)));
        out.push_str(&format!("      \"plr_lu\": {},\n", json_f64_or_null(t.plr)));
        out.push_str(&format!("      \"replay_gain_track_db\": {},\n", rg));
        out.push_str(&format!("      \"stereo_correlation\": {}\n", corr));
        out.push_str(&format!("    }}{}\n", comma));
    }
    out.push_str("  ]\n}");
    out
}

/// Formats an A/B release comparison table (Baseline `A` vs Remaster/Alternate `B`).
pub fn format_comparison_text(
    label_a: &str,
    tracks_a: &[TrackAnalysis],
    label_b: &str,
    tracks_b: &[TrackAnalysis],
    name_opts: &LogFilenameOptions,
    use_tags: bool,
) -> String {
    let dr_a = compute_official_album_dr(tracks_a);
    let dr_b = compute_official_album_dr(tracks_b);
    let rule_eq = "=".repeat(96);
    let rule_dash = "-".repeat(96);

    let mut lines = Vec::new();
    lines.push(rule_eq.clone());
    lines.push(format!("A/B Mastering Comparison:  A = \"{}\" (DR{})   vs   B = \"{}\" (DR{})", label_a, dr_a, label_b, dr_b));
    lines.push(format!(
        "Album DR Delta (B - A):    {:+}",
        dr_b - dr_a
    ));
    lines.push(rule_dash.clone());
    lines.push(format!(
        "{:>4}  {:>6}  {:>6}  {:>5}  {:>8}  {:>8}  {:>8}  {:>8}  {}",
        "#", "DR A", "DR B", "dDR", "LUFS A", "LUFS B", "TPL A", "TPL B", "Track"
    ));
    lines.push(rule_dash.clone());

    let n = tracks_a.len().max(tracks_b.len());
    for i in 0..n {
        let a = tracks_a.get(i);
        let b = tracks_b.get(i);
        let dr_a_s = a.map(|t| format!("DR{}", t.dr_track_rounded)).unwrap_or_else(|| "-".into());
        let dr_b_s = b.map(|t| format!("DR{}", t.dr_track_rounded)).unwrap_or_else(|| "-".into());
        let d_dr = match (a, b) {
            (Some(ta), Some(tb)) => format!("{:+}", tb.dr_track_rounded - ta.dr_track_rounded),
            _ => "-".into(),
        };
        let lufs_a = a
            .map(|t| format!("{:.2}", t.integrated_lufs))
            .unwrap_or_else(|| "-".into());
        let lufs_b = b
            .map(|t| format!("{:.2}", t.integrated_lufs))
            .unwrap_or_else(|| "-".into());
        let tpl_a = a
            .map(|t| format!("{:+.2}", t.max_tpl_joint))
            .unwrap_or_else(|| "-".into());
        let tpl_b = b
            .map(|t| format!("{:+.2}", t.max_tpl_joint))
            .unwrap_or_else(|| "-".into());
        let track_name = a
            .or(b)
            .map(|t| track_label(t, name_opts, use_tags))
            .unwrap_or_default();
        lines.push(format!(
            "{:>4}  {:>6}  {:>6}  {:>5}  {:>8}  {:>8}  {:>8}  {:>8}  {}",
            i + 1,
            dr_a_s,
            dr_b_s,
            d_dr,
            lufs_a,
            lufs_b,
            tpl_a,
            tpl_b,
            track_name
        ));
    }
    lines.push(rule_eq);
    lines.join("\n")
}
