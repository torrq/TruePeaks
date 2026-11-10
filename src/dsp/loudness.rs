use crate::decoder::SpeakerChannel;
use ebur128::{EbuR128, Mode};

const DUAL_MONO_OFFSET_DB: f64 = 3.010299956639812; // 10 * log10(2.0)

pub struct LoudnessAnalyzer {
    joint_ebu: EbuR128,
    channel_ebus: Vec<EbuR128>,
    num_channels: usize,
    sample_rate: u32,
    /// Hop size in frames for Momentary / Short-Term / PSR evaluation (100ms = sample_rate / 10)
    hop_frames: usize,
    frames_since_hop: usize,
    total_frames_processed: u64,

    pub max_momentary_joint: f64,
    pub max_short_term_joint: f64,
    pub max_momentary_channels: Vec<f64>,
    pub max_short_term_channels: Vec<f64>,

    /// Sliding 3-second (30 hops of 100ms) joint sample peak ring buffer for Min. PSR
    hop_peak_ring: Vec<f64>,
    hop_peak_idx: usize,
    current_hop_peak: f64,
    pub min_psr: f64,

    /// Scratch buffer for single-channel deinterleaving within a hop
    single_ch_buf: Vec<f64>,
}

impl LoudnessAnalyzer {
    pub fn new(sample_rate: u32, channels: &[SpeakerChannel]) -> Result<Self, String> {
        let num_channels = channels.len().max(1);
        let mut joint_ebu = EbuR128::new(
            num_channels as u32,
            sample_rate,
            Mode::I | Mode::LRA | Mode::M | Mode::S | Mode::SAMPLE_PEAK | Mode::TRUE_PEAK,
        )
        .map_err(|e| format!("Failed to init joint EbuR128: {:?}", e))?;

        for (i, ch) in channels.iter().enumerate() {
            let _ = joint_ebu.set_channel(i as u32, ch.ebur128_channel());
        }

        let mut channel_ebus = Vec::with_capacity(num_channels);
        for _ in 0..num_channels {
            let mut ch_ebu = EbuR128::new(1, sample_rate, Mode::M | Mode::S)
                .map_err(|e| format!("Failed to init channel EbuR128: {:?}", e))?;
            let _ = ch_ebu.set_channel(0, ebur128::Channel::Left);
            channel_ebus.push(ch_ebu);
        }

        // 10 Hz (100 ms) evaluation step per ITU-R BS.1770-4 / EBU Tech 3341
        let hop_frames = ((sample_rate as usize) / 10).max(1);

        Ok(Self {
            joint_ebu,
            channel_ebus,
            num_channels,
            sample_rate,
            hop_frames,
            frames_since_hop: 0,
            total_frames_processed: 0,
            max_momentary_joint: f64::NEG_INFINITY,
            max_short_term_joint: f64::NEG_INFINITY,
            max_momentary_channels: vec![f64::NEG_INFINITY; num_channels],
            max_short_term_channels: vec![f64::NEG_INFINITY; num_channels],
            hop_peak_ring: vec![0.0; 30],
            hop_peak_idx: 0,
            current_hop_peak: 0.0,
            min_psr: f64::INFINITY,
            single_ch_buf: Vec::with_capacity(hop_frames),
        })
    }

    pub fn process_frames(&mut self, interleaved: &[f64], frames: usize) {
        let mut offset_frames = 0usize;
        while offset_frames < frames {
            let needed = self.hop_frames - self.frames_since_hop;
            let take = needed.min(frames - offset_frames);
            let slice_start = offset_frames * self.num_channels;
            let slice_end = (offset_frames + take) * self.num_channels;
            let sub_slice = &interleaved[slice_start..slice_end];

            let _ = self.joint_ebu.add_frames_f64(sub_slice);

            // Track joint sample peak within this 100ms hop
            for &s in sub_slice {
                let a = s.abs();
                if a > self.current_hop_peak {
                    self.current_hop_peak = a;
                }
            }

            // Feed each single-channel dual-mono analyzer
            for ch in 0..self.num_channels {
                self.single_ch_buf.clear();
                for f in 0..take {
                    self.single_ch_buf.push(sub_slice[f * self.num_channels + ch]);
                }
                let _ = self.channel_ebus[ch].add_frames_f64(&self.single_ch_buf);
            }

            self.frames_since_hop += take;
            self.total_frames_processed += take as u64;
            offset_frames += take;

            if self.frames_since_hop == self.hop_frames {
                self.evaluate_hop();
                self.frames_since_hop = 0;
            }
        }
    }

    fn evaluate_hop(&mut self) {
        // Record 100ms peak into 3s (30-hop) ring buffer
        self.hop_peak_ring[self.hop_peak_idx] = self.current_hop_peak;
        self.hop_peak_idx = (self.hop_peak_idx + 1) % 30;
        self.current_hop_peak = 0.0;

        let frames_400ms = (self.sample_rate as u64 * 4) / 10;
        let frames_3s = (self.sample_rate as u64) * 3;

        if self.total_frames_processed >= frames_400ms {
            if let Ok(m) = self.joint_ebu.loudness_momentary() {
                if m.is_finite() && m > self.max_momentary_joint {
                    self.max_momentary_joint = m;
                }
            }
            for ch in 0..self.num_channels {
                if let Ok(m) = self.channel_ebus[ch].loudness_momentary() {
                    if m.is_finite() {
                        let m_dual = if self.num_channels > 1 {
                            m + DUAL_MONO_OFFSET_DB
                        } else {
                            m
                        };
                        if m_dual > self.max_momentary_channels[ch] {
                            self.max_momentary_channels[ch] = m_dual;
                        }
                    }
                }
            }
        }

        if self.total_frames_processed >= frames_3s {
            if let Ok(s) = self.joint_ebu.loudness_shortterm() {
                if s.is_finite() && s > self.max_short_term_joint {
                    self.max_short_term_joint = s;
                }
                // Evaluate Min. PSR when Short-Term loudness is above gating threshold
                if s.is_finite() && s > -70.0 {
                    let window_peak = self
                        .hop_peak_ring
                        .iter()
                        .copied()
                        .fold(0.0_f64, f64::max);
                    if window_peak > 0.0 {
                        let sppm_3s_db = 20.0 * window_peak.log10();
                        let psr = sppm_3s_db - s;
                        if psr < self.min_psr {
                            self.min_psr = psr;
                        }
                    }
                }
            }
            for ch in 0..self.num_channels {
                if let Ok(s) = self.channel_ebus[ch].loudness_shortterm() {
                    if s.is_finite() {
                        let s_dual = if self.num_channels > 1 {
                            s + DUAL_MONO_OFFSET_DB
                        } else {
                            s
                        };
                        if s_dual > self.max_short_term_channels[ch] {
                            self.max_short_term_channels[ch] = s_dual;
                        }
                    }
                }
            }
        }
    }

    pub fn finalize(&mut self) -> LoudnessResults {
        let integrated_lufs = self
            .joint_ebu
            .loudness_global()
            .unwrap_or(f64::NEG_INFINITY);
        let lra = self.joint_ebu.loudness_range().unwrap_or(0.0);

        let mut true_peaks_db = Vec::with_capacity(self.num_channels);
        let mut max_tpl_joint_db = f64::NEG_INFINITY;

        for ch in 0..self.num_channels {
            let tp_lin = self.joint_ebu.true_peak(ch as u32).unwrap_or(0.0);
            let tp_db = if tp_lin > 0.0 {
                20.0 * tp_lin.log10()
            } else {
                f64::NEG_INFINITY
            };
            if tp_db > max_tpl_joint_db {
                max_tpl_joint_db = tp_db;
            }
            true_peaks_db.push(tp_db);
        }

        LoudnessResults {
            integrated_lufs,
            lra,
            max_momentary_joint: self.max_momentary_joint,
            max_short_term_joint: self.max_short_term_joint,
            max_momentary_channels: self.max_momentary_channels.clone(),
            max_short_term_channels: self.max_short_term_channels.clone(),
            true_peaks_db,
            max_tpl_joint_db,
            min_psr: self.min_psr,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoudnessResults {
    pub integrated_lufs: f64,
    pub lra: f64,
    pub max_momentary_joint: f64,
    pub max_short_term_joint: f64,
    pub max_momentary_channels: Vec<f64>,
    pub max_short_term_channels: Vec<f64>,
    pub true_peaks_db: Vec<f64>,
    pub max_tpl_joint_db: f64,
    pub min_psr: f64,
}
