pub mod dr_pmf;
pub mod loudness;
pub mod stats;

use crate::decoder::cue::{parse_cue_sheet, CueTrackSpec};
use crate::decoder::{
    open_audio_file, AudioStreamMetadata, AudioTags, DecodeError, SpeakerChannel, CHUNK_FRAMES,
};
use dr_pmf::DrPmfAccumulator;
use loudness::{tpl_oversampling_factor, LoudnessAnalyzer, TimelinePoint};
use stats::{compute_plr, TrackStatsAccumulator};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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
    pub tpl_oversample_factor: u32,

    pub max_sppm_channels: Vec<f64>,
    pub max_sppm_joint: f64,
    pub max_sppm_time_secs: f64,

    pub rms_channels: Vec<f64>,

    pub max_mom_channels: Vec<f64>,
    pub max_mom_joint: f64,

    pub max_short_term_channels: Vec<f64>,
    pub max_short_term_joint: f64,

    pub integrated_lufs: f64,
    pub replay_gain_track_db: Option<f64>,

    pub dr_channels: Vec<f64>,
    pub dr_track_raw: f64,
    pub dr_track_rounded: i32,

    pub min_psr: f64,
    pub min_psr_time_secs: Option<f64>,
    pub plr: f64,
    pub lra: f64,

    pub stereo_correlation: Option<f64>,
    pub tags: Option<AudioTags>,
    pub timeline: Vec<TimelinePoint>,
}

impl TrackAnalysis {
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate > 0 {
            (self.total_frames as f64) / (self.sample_rate as f64)
        } else {
            0.0
        }
    }

    pub fn display_name(&self, use_embedded_tags: bool) -> String {
        if use_embedded_tags {
            if let Some(ref tags) = self.tags {
                if let Some(ref title) = tags.title {
                    let clean = title.trim().nfc().collect::<String>();
                    if !clean.is_empty() {
                        return if let Some(num) = tags.track_number {
                            format!("{:02} - {}", num, clean)
                        } else {
                            clean
                        };
                    }
                }
            }
        }
        self.file_stem.nfc().collect::<String>()
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

struct TrackDspPipeline {
    loudness: LoudnessAnalyzer,
    dr_acc: DrPmfAccumulator,
    stats_acc: TrackStatsAccumulator,
    sample_rate: u32,
    total_frames: u64,
}

impl TrackDspPipeline {
    fn new(meta: &AudioStreamMetadata) -> Result<Self, DecodeError> {
        let sample_rate = meta.sample_rate;
        let channels = &meta.channels;
        let num_channels = channels.len().max(1);
        Ok(Self {
            loudness: LoudnessAnalyzer::new(sample_rate, channels)
                .map_err(DecodeError::Format)?,
            dr_acc: DrPmfAccumulator::new(sample_rate, channels),
            stats_acc: TrackStatsAccumulator::new(num_channels),
            sample_rate,
            total_frames: 0,
        })
    }

    fn process_frames(&mut self, interleaved: &[f64], frames: usize) {
        if frames == 0 {
            return;
        }
        self.loudness.process_frames(interleaved, frames);
        self.dr_acc.process_frames(interleaved, frames);
        self.stats_acc.process_frames(interleaved, frames);
        self.total_frames += frames as u64;
    }

    #[allow(clippy::too_many_arguments)]
    fn finalize(
        mut self,
        file_stem: String,
        file_format: String,
        file_size_bytes: u64,
        meta: AudioStreamMetadata,
        bit_mask: u32,
        include_lfe_in_dr: bool,
        tags_override: Option<AudioTags>,
    ) -> TrackAnalysis {
        let loud_res = self.loudness.finalize();
        let dr_res = self.dr_acc.finalize(include_lfe_in_dr);
        let stats_res = self
            .stats_acc
            .finalize(meta.is_lossless_integer, bit_mask);

        let plr = compute_plr(
            loud_res.max_tpl_joint_db,
            stats_res.sppm_joint_db,
            loud_res.integrated_lufs,
        );
        let replay_gain_track_db = if loud_res.integrated_lufs.is_finite() {
            Some(-18.0 - loud_res.integrated_lufs)
        } else {
            None
        };
        let sr_f = self.sample_rate.max(1) as f64;
        let max_sppm_time_secs = (stats_res.max_sppm_frame as f64) / sr_f;
        let tpl_oversample_factor = tpl_oversampling_factor(self.sample_rate);
        let tags = tags_override.or(meta.tags);

        TrackAnalysis {
            file_stem,
            file_format,
            sample_rate: self.sample_rate,
            total_frames: self.total_frames,
            file_size_bytes,
            channels: meta.channels,
            word_length: meta.word_length,
            bits_used: stats_res.bits_used,
            max_tpl_channels: loud_res.true_peaks_db,
            max_tpl_joint: loud_res.max_tpl_joint_db,
            tpl_oversample_factor,
            max_sppm_channels: stats_res.sppm_channels_db,
            max_sppm_joint: stats_res.sppm_joint_db,
            max_sppm_time_secs,
            rms_channels: stats_res.aes17_rms_channels_db,
            max_mom_channels: loud_res.max_momentary_channels,
            max_mom_joint: loud_res.max_momentary_joint,
            max_short_term_channels: loud_res.max_short_term_channels,
            max_short_term_joint: loud_res.max_short_term_joint,
            integrated_lufs: loud_res.integrated_lufs,
            replay_gain_track_db,
            dr_channels: dr_res.channel_dr,
            dr_track_raw: dr_res.track_dr_raw,
            dr_track_rounded: dr_res.track_dr_rounded,
            min_psr: loud_res.min_psr,
            min_psr_time_secs: loud_res.min_psr_time_secs,
            plr,
            lra: loud_res.lra,
            stereo_correlation: stats_res.stereo_correlation,
            tags,
            timeline: loud_res.timeline,
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
    let num_channels = meta.channels.len().max(1);
    let mut pipeline = TrackDspPipeline::new(&meta)?;

    let mut chunk_buf = Vec::with_capacity(CHUNK_FRAMES * num_channels);
    let mut bit_mask: u32 = 0;
    let file_size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(DecodeError::Cancelled);
        }

        let frames = decoder.next_chunk(&mut chunk_buf, &mut bit_mask)?;
        if frames == 0 {
            break;
        }
        pipeline.process_frames(&chunk_buf, frames);
    }

    let final_meta = decoder.metadata().clone();
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

    Ok(pipeline.finalize(
        file_stem,
        ext,
        file_size_bytes,
        final_meta,
        bit_mask,
        include_lfe_in_dr,
        None,
    ))
}

/// Decodes and slices a `.cue` sheet in a single streaming pass per referenced audio file,
/// producing per-track [`TrackAnalysis`] reports with embedded track titles and performers.
pub fn analyze_cue_sheet(
    cue_path: &Path,
    include_lfe_in_dr: bool,
    cancel_flag: &AtomicBool,
) -> Result<(String, Vec<TrackAnalysis>), DecodeError> {
    let sheet = parse_cue_sheet(cue_path).map_err(DecodeError::Format)?;
    let album_label = sheet
        .title
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            cue_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Album")
                .nfc()
                .collect::<String>()
        });

    // Group track specs by referenced audio file so each image file is decoded once
    let mut by_file: BTreeMap<PathBuf, Vec<CueTrackSpec>> = BTreeMap::new();
    for spec in sheet.tracks {
        by_file
            .entry(spec.file_path.clone())
            .or_default()
            .push(spec);
    }

    let mut analyzed: Vec<(u32, TrackAnalysis)> = Vec::new();
    for (audio_path, mut specs) in by_file {
        specs.sort_by_key(|s| (s.track_number, (s.start_secs * 1000.0) as u64));
        let mut decoder = open_audio_file(&audio_path)?;
        let meta = decoder.metadata().clone();
        let sr = meta.sample_rate as f64;
        let num_ch = meta.channels.len().max(1);
        let file_size_bytes = std::fs::metadata(&audio_path).map(|m| m.len()).unwrap_or(0);
        let ext = audio_path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| format!(".{}", s.to_ascii_lowercase()))
            .unwrap_or_default();

        let mut slots: Vec<(CueTrackSpec, u64, u64, TrackDspPipeline)> = Vec::with_capacity(specs.len());
        for s in specs {
            let start_frame = (s.start_secs * sr).round().max(0.0) as u64;
            let end_frame = s
                .end_secs
                .map(|e| (e * sr).round().max(0.0) as u64)
                .unwrap_or(u64::MAX);
            slots.push((s, start_frame, end_frame, TrackDspPipeline::new(&meta)?));
        }

        let mut chunk_buf = Vec::with_capacity(CHUNK_FRAMES * num_ch);
        let mut bit_mask: u32 = 0;
        let mut cur_frame: u64 = 0;

        loop {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(DecodeError::Cancelled);
            }
            let frames = decoder.next_chunk(&mut chunk_buf, &mut bit_mask)?;
            if frames == 0 {
                break;
            }
            let chunk_start = cur_frame;
            let chunk_end = cur_frame + (frames as u64);

            for (_, t_start, t_end, pipe) in &mut slots {
                if *t_end <= chunk_start || *t_start >= chunk_end {
                    continue;
                }
                let lo = ((*t_start).max(chunk_start) - chunk_start) as usize;
                let hi = ((*t_end).min(chunk_end) - chunk_start) as usize;
                if hi > lo {
                    pipe.process_frames(&chunk_buf[lo * num_ch..hi * num_ch], hi - lo);
                }
            }
            cur_frame = chunk_end;
        }

        let final_meta = decoder.metadata().clone();
        let total_audio_frames = cur_frame.max(1);
        for (spec, _, _, pipe) in slots {
            let prop_bytes = ((file_size_bytes as u128 * pipe.total_frames as u128)
                / (total_audio_frames as u128)) as u64;
            let stem = format!("{:02} - {}", spec.track_number, spec.title)
                .nfc()
                .collect::<String>();
            let tags = AudioTags {
                title: Some(spec.title.clone()),
                artist: spec.performer.clone(),
                album: sheet.title.clone(),
                track_number: Some(spec.track_number),
            };
            let ta = pipe.finalize(
                stem,
                ext.clone(),
                prop_bytes,
                final_meta.clone(),
                bit_mask,
                include_lfe_in_dr,
                Some(tags),
            );
            analyzed.push((spec.track_number, ta));
        }
    }

    analyzed.sort_by_key(|(num, _)| *num);
    Ok((
        album_label,
        analyzed.into_iter().map(|(_, ta)| ta).collect(),
    ))
}
