use crate::config::SingleFileRowsConfig;
use crate::decoder::layout_description;
use crate::dsp::TrackAnalysis;
use crate::formatter::format_sample_rate;

pub fn format_single_file_log(track: &TrackAnalysis, rows_cfg: &SingleFileRowsConfig) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(20);

    // Leading empty line
    lines.push(String::new());
    lines.push(format!("File Name:     {}", track.file_stem));
    lines.push(format!("File Format:   {}", track.file_format));

    let num_ch = track.channels.len().max(1);
    if num_ch > 2 {
        lines.push(format!(
            "Channel Layout: {}",
            layout_description(&track.channels)
        ));
    }

    lines.push(String::new());
    lines.push(
        "                                               Sample Rate:       Word Length:        Bits Used:    "
            .to_string(),
    );

    let sr_str = format_sample_rate(track.sample_rate);
    let wl_str = track
        .word_length
        .map(|w| w.to_string())
        .unwrap_or_else(|| "-".to_string());
    let bu_str = track
        .bits_used
        .map(|b| b.to_string())
        .unwrap_or_else(|| "-".to_string());

    lines.push(format!("{:>56}{:>17}{:>19}        ", sr_str, wl_str, bu_str));
    lines.push(String::new());

    // Channel header line
    let mut ch_header = String::from(" ".repeat(51));
    for ch in &track.channels {
        let lbl = ch.single_log_header(num_ch);
        ch_header.push_str(&format!("{:<19}", lbl));
    }
    ch_header.push_str("Joint      ");
    lines.push(ch_header);

    // Helper for formatting a 19-char cell: 6-char right-aligned number + 13-char left-aligned suffix
    let fmt_cell = |num_str: &str, suffix: &str| -> String {
        format!("{:>6}{:<13}", num_str, suffix)
    };
    let blank_cell = "                   "; // 19 spaces

    // 1. Max. TPL
    if rows_cfg.max_tpl {
        let mut line = format!("{:<49}", "Max. TPL (True Peak Level - BS.1770)");
        for &v in &track.max_tpl_channels {
            line.push_str(&fmt_cell(&format!("{:+.2}", v), ""));
        }
        line.push_str(&fmt_cell(&format!("{:+.2}", track.max_tpl_joint), " (ITU)"));
        lines.push(line);
    }

    // 2. Max. SPPM
    if rows_cfg.max_sppm {
        let mut line = format!("{:<49}", "Max. SPPM (Sample Peak Program Metering)");
        for &v in &track.max_sppm_channels {
            line.push_str(&fmt_cell(&format!("{:.2}", v), " dBFS"));
        }
        line.push_str(&fmt_cell(&format!("{:.2}", track.max_sppm_joint), " dBFS"));
        lines.push(line);
    }

    // 3. RMS
    if rows_cfg.rms {
        let mut line = format!("{:<49}", "RMS");
        for &v in &track.rms_channels {
            line.push_str(&fmt_cell(&format!("{:.2}", v), ""));
        }
        line.push_str(blank_cell);
        lines.push(line);
    }

    // 4. Max. Momentary Loudness
    if rows_cfg.max_momentary {
        let mut line = format!("{:<49}", "Max. Momentary Loudness (BS.1770)");
        for &v in &track.max_mom_channels {
            line.push_str(&fmt_cell(&format!("{:.2}", v), " (non-ITU)"));
        }
        line.push_str(&fmt_cell(&format!("{:.2}", track.max_mom_joint), " (ITU)"));
        lines.push(line);
    }

    // 5. Max. Short-Term Loudness
    if rows_cfg.max_short_term {
        let mut line = format!("{:<49}", "Max. Short-Term Loudness (BS.1770)");
        for &v in &track.max_short_term_channels {
            line.push_str(&fmt_cell(&format!("{:.2}", v), " (non-ITU)"));
        }
        line.push_str(&fmt_cell(
            &format!("{:.2}", track.max_short_term_joint),
            " (ITU)",
        ));
        lines.push(line);
    }

    // 6. Integrated Loudness
    if rows_cfg.integrated_loudness {
        let mut line = format!("{:<49}", "Integrated Loudness (LUFS - BS.1770)");
        for _ in 0..num_ch {
            line.push_str(blank_cell);
        }
        line.push_str(&fmt_cell(&format!("{:.2}", track.integrated_lufs), " (ITU)"));
        lines.push(line);
    }

    // 7. DR (PMF Dynamic Range)
    if rows_cfg.dr_pmf {
        let mut line = format!("{:<49}", "DR (PMF Dynamic Range)");
        for &v in &track.dr_channels {
            line.push_str(&fmt_cell(&format!("{:.2}", v), ""));
        }
        line.push_str(&fmt_cell(
            &format!("{}", track.dr_track_rounded),
            " (PMF)",
        ));
        lines.push(line);
    }

    // 8. Min. PSR
    if rows_cfg.min_psr {
        let mut line = format!("{:<49}", "Min. PSR (SPPM-to-Short-Term Loudness Ratio)");
        for _ in 0..num_ch {
            line.push_str(blank_cell);
        }
        line.push_str(&fmt_cell(&format!("{:.2}", track.min_psr), " (AES)"));
        lines.push(line);
    }

    // 9. PLR
    if rows_cfg.plr {
        let mut line = format!("{:<49}", "PLR (TP-to-Integrated Loudness Ratio)");
        for _ in 0..num_ch {
            line.push_str(blank_cell);
        }
        line.push_str(&fmt_cell(&format!("{:.2}", track.plr), " (AES)"));
        lines.push(line);
    }

    // 10. LRA
    if rows_cfg.lra {
        let mut line = format!("{:<49}", "LRA (Loudness Range - BS.1770)");
        for _ in 0..num_ch {
            line.push_str(blank_cell);
        }
        line.push_str(&fmt_cell(&format!("{:.2}", track.lra), " (ITU)"));
        lines.push(line);
    }

    lines.join("\n")
}
