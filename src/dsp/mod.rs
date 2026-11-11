pub mod dr_pmf;
pub mod loudness;
pub mod stats;

use crate::decoder::{open_audio_file, DecodeError, SpeakerChannel, CHUNK_FRAMES};
use dr_pmf::DrPmfAccumulator;
use loudness::LoudnessAnalyzer;
use stats::{compute_plr, TrackStatsAccumulator};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone)]
pub struct TrackAnalysis {
    pub file_stem: String,
    pub file_format: String,
    pub sample_rate: u32,
    pub total_frames: u64,
    pub file_size_bytes: u64,
    pub channels: Vec<SpeakerChannel>,
    pub word_length: Option<u32>,
    pub bits_used: Option<u32>,

    pub max_tpl_channels: Vec<f64>,
    pub max_tpl_joint: f64,

    pub max_sppm_channels: Vec<f64>,
    pub max_sppm_joint: f64,

    pub rms_channels: Vec<f64>,

    pub max_mom_channels: Vec<f64>,
    pub max_mom_joint: f64,

    pub max_short_term_channels: Vec<f64>,
    pub max_short_term_joint: f64,

    pub integrated_lufs: f64,

    pub dr_channels: Vec<f64>,
    pub dr_track_raw: f64,
    pub dr_track_rounded: i32,

    pub min_psr: f64,
    pub plr: f64,
    pub lra: f64,
}

impl TrackAnalysis {
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate > 0 {
            (self.total_frames as f64) / (self.sample_rate as f64)
        } else {
            0.0
        }
    }

    /// Quadratic mean AES17 RMS across active non-LFE channels (falling back to all channels).
    pub fn combined_rms_db(&self) -> f64 {
        let mut sum_pow = 0.0_f64;
        let mut count = 0usize;
        for (idx, &db) in self.rms_channels.iter().enumerate() {
            let is_lfe = self.channels.get(idx).map(|c| c.is_lfe()).unwrap_or(false);
            if !is_lfe && db.is_finite() {
                sum_pow += 10.0_f64.powf(db / 10.0);
                count += 1;
            }
        }
        if count == 0 {
            for &db in &self.rms_channels {
                if db.is_finite() {
                    sum_pow += 10.0_f64.powf(db / 10.0);
                    count += 1;
                }
            }
        }
        if count > 0 && sum_pow > 0.0 {
            10.0 * (sum_pow / (count as f64)).log10()
        } else {
            f64::NEG_INFINITY
        }
    }
}

pub fn analyze_file(
    path: &Path,
    include_lfe_in_dr: bool,
    cancel_flag: &AtomicBool,
) -> Result<TrackAnalysis, DecodeError> {
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(DecodeError::Cancelled);
    }

    let mut decoder = open_audio_file(path)?;
    let meta = decoder.metadata().clone();
    let sample_rate = meta.sample_rate;
    let channels = meta.channels.clone();
    let num_channels = channels.len().max(1);

    let mut loudness = LoudnessAnalyzer::new(sample_rate, &channels)
        .map_err(DecodeError::Format)?;
    let mut dr_acc = DrPmfAccumulator::new(sample_rate, &channels);
    let mut stats_acc = TrackStatsAccumulator::new(num_channels);

    let mut chunk_buf = Vec::with_capacity(CHUNK_FRAMES * num_channels);
    let mut bit_mask: u32 = 0;
    let mut total_frames: u64 = 0;
    let file_size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(DecodeError::Cancelled);
        }

        let frames = decoder.next_chunk(&mut chunk_buf, &mut bit_mask)?;
        if frames == 0 {
            break;
        }
        total_frames += frames as u64;

        loudness.process_frames(&chunk_buf, frames);
        dr_acc.process_frames(&chunk_buf, frames);
        stats_acc.process_frames(&chunk_buf, frames);
    }

    let final_meta = decoder.metadata().clone();
    let loud_res = loudness.finalize();
    let dr_res = dr_acc.finalize(include_lfe_in_dr);
    let stats_res = stats_acc.finalize(final_meta.is_lossless_integer, bit_mask);

    let plr = compute_plr(
        loud_res.max_tpl_joint_db,
        stats_res.sppm_joint_db,
        loud_res.integrated_lufs,
    );

    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .nfc()
        .collect::<String>();

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| format!(".{}", s.to_ascii_lowercase()))
        .unwrap_or_default();

    Ok(TrackAnalysis {
        file_stem,
        file_format: ext,
        sample_rate,
        total_frames,
        file_size_bytes,
        channels: final_meta.channels,
        word_length: final_meta.word_length,
        bits_used: stats_res.bits_used,
        max_tpl_channels: loud_res.true_peaks_db,
        max_tpl_joint: loud_res.max_tpl_joint_db,
        max_sppm_channels: stats_res.sppm_channels_db,
        max_sppm_joint: stats_res.sppm_joint_db,
        rms_channels: dr_res.aes17_rms_channels_db,
        max_mom_channels: loud_res.max_momentary_channels,
        max_mom_joint: loud_res.max_momentary_joint,
        max_short_term_channels: loud_res.max_short_term_channels,
        max_short_term_joint: loud_res.max_short_term_joint,
        integrated_lufs: loud_res.integrated_lufs,
        dr_channels: dr_res.channel_dr,
        dr_track_raw: dr_res.track_dr_raw,
        dr_track_rounded: dr_res.track_dr_rounded,
        min_psr: loud_res.min_psr,
        plr,
        lra: loud_res.lra,
    })
}
