use crate::decoder::SpeakerChannel;
use ebur128::{EbuR128, Mode};
use std::collections::VecDeque;

const LOUDNESS_OFFSET_LUFS: f64 = -0.691;

/// Exact ITU-R BS.1770-4 two-stage K-weighting biquad filter (bilinear transform at any sample rate,
/// matching `ebur128::filter`).
#[derive(Clone)]
struct KWeightFilter {
    b1: [f64; 3],
    a1: [f64; 3],
    b2: [f64; 3],
    a2: [f64; 3],
    s1: [f64; 2],
    s2: [f64; 2],
}

impl KWeightFilter {
    fn new(sample_rate: f64) -> Self {
        let f0_1 = 1681.974450955533;
        let g_1 = 3.999843853973347;
        let q_1 = 0.7071752369554196;
        let k_1 = (std::f64::consts::PI * f0_1 / sample_rate).tan();
        let vh = 10.0_f64.powf(g_1 / 20.0);
        let vb = vh.powf(0.4996667741545416);
        let a0_1 = 1.0 + k_1 / q_1 + k_1 * k_1;
        let b1 = [
            (vh + vb * k_1 / q_1 + k_1 * k_1) / a0_1,
            2.0 * (k_1 * k_1 - vh) / a0_1,
            (vh - vb * k_1 / q_1 + k_1 * k_1) / a0_1,
        ];
        let a1 = [
            1.0,
            2.0 * (k_1 * k_1 - 1.0) / a0_1,
            (1.0 - k_1 / q_1 + k_1 * k_1) / a0_1,
        ];

        let f0_2 = 38.13547087602444;
        let q_2 = 0.5003270373238773;
        let k_2 = (std::f64::consts::PI * f0_2 / sample_rate).tan();
        let a0_2 = 1.0 + k_2 / q_2 + k_2 * k_2;
        let b2 = [1.0, -2.0, 1.0];
        let a2 = [
            1.0,
            2.0 * (k_2 * k_2 - 1.0) / a0_2,
            (1.0 - k_2 / q_2 + k_2 * k_2) / a0_2,
        ];

        Self {
            b1,
            a1,
            b2,
            a2,
            s1: [0.0; 2],
            s2: [0.0; 2],
        }
    }

    #[inline(always)]
    fn process_sq(&mut self, x: f64) -> f64 {
        let w1 = x - self.a1[1] * self.s1[0] - self.a1[2] * self.s1[1];
        let y1 = self.b1[0] * w1 + self.b1[1] * self.s1[0] + self.b1[2] * self.s1[1];
        self.s1[1] = self.s1[0];
        self.s1[0] = w1;

        let w2 = y1 - self.a2[1] * self.s2[0] - self.a2[2] * self.s2[1];
        let y2 = self.b2[0] * w2 + self.b2[1] * self.s2[0] + self.b2[2] * self.s2[1];
        self.s2[1] = self.s2[0];
        self.s2[0] = w2;

        // Guard against denormals
        if self.s1[0].abs() < f64::MIN_POSITIVE {
            self.s1[0] = 0.0;
        }
        if self.s2[0].abs() < f64::MIN_POSITIVE {
            self.s2[0] = 0.0;
        }

        y2 * y2
    }
}

pub struct LoudnessAnalyzer {
    joint_ebu: EbuR128,
    num_channels: usize,
    itu_weights: Vec<f64>,
    k_filters: Vec<KWeightFilter>,

    /// Window lengths in frames (400 ms and 3.0 s) matching `ebur128` grid sizing
    w400_frames: usize,
    w3s_frames: usize,

    /// Circular history buffer of per-channel K-weighted squared samples (length = num_channels * w3s_frames)
    ring_sq: Vec<f64>,
    ring_pos: usize,
    pos_400_out: usize,
    frames_seen: u64,

    /// Running sums for 400ms Momentary and 3s Short-Term per channel
    sum_400: Vec<f64>,
    sum_3s: Vec<f64>,
    /// Periodic resync counter to prevent any floating-point drift on long streams
    resync_countdown: usize,

    /// Monotonic decreasing deque of `(frame_index, joint_abs_sample)` over trailing 3s window
    peak_deque: VecDeque<(u64, f64)>,

    /// Linear window energy maxima & PSR ratio tracked without `log10` in the inner loop
    max_sum_m_joint: f64,
    max_sum_s_joint: f64,
    max_sum_m_channels: Vec<f64>,
    max_sum_s_channels: Vec<f64>,
    min_psr_lin_ratio: f64,
    psr_gate_sum_thresh: f64,
}

impl LoudnessAnalyzer {
    pub fn new(sample_rate: u32, channels: &[SpeakerChannel]) -> Result<Self, String> {
        let num_channels = channels.len().max(1);
        let mut joint_ebu = EbuR128::new(
            num_channels as u32,
            sample_rate,
            Mode::I | Mode::LRA | Mode::SAMPLE_PEAK | Mode::TRUE_PEAK,
        )
        .map_err(|e| format!("Failed to init EbuR128: {:?}", e))?;

        let mut itu_weights = Vec::with_capacity(num_channels);
        for (i, ch) in channels.iter().enumerate() {
            let _ = joint_ebu.set_channel(i as u32, ch.ebur128_channel());
            itu_weights.push(ch.itu_bs1770_weight());
        }

        let samples_in_100ms = ((sample_rate as usize) + 5) / 10;
        let w400_frames = (samples_in_100ms * 4).max(1);
        let w3s_frames = (samples_in_100ms * 30).max(1);

        // -70.0 LUFS threshold in linear sum_s_joint units: 10^((-70.0 + 0.691)/10) * w3s_frames
        let psr_gate_sum_thresh =
            10.0_f64.powf((-70.0 - LOUDNESS_OFFSET_LUFS) / 10.0) * (w3s_frames as f64);

        let k_filters = (0..num_channels)
            .map(|_| KWeightFilter::new(sample_rate as f64))
            .collect();

        Ok(Self {
            joint_ebu,
            num_channels,
            itu_weights,
            k_filters,
            w400_frames,
            w3s_frames,
            ring_sq: vec![0.0; num_channels * w3s_frames],
            ring_pos: 0,
            pos_400_out: w3s_frames - w400_frames,
            frames_seen: 0,
            sum_400: vec![0.0; num_channels],
            sum_3s: vec![0.0; num_channels],
            resync_countdown: w3s_frames,
            peak_deque: VecDeque::with_capacity(128),
            max_sum_m_joint: 0.0,
            max_sum_s_joint: 0.0,
            max_sum_m_channels: vec![0.0; num_channels],
            max_sum_s_channels: vec![0.0; num_channels],
            min_psr_lin_ratio: f64::INFINITY,
            psr_gate_sum_thresh,
        })
    }

    pub fn process_frames(&mut self, interleaved: &[f64], frames: usize) {
        if frames == 0 {
            return;
        }
        let _ = self
            .joint_ebu
            .add_frames_f64(&interleaved[..frames * self.num_channels]);

        let ch_count = self.num_channels;
        let w400_u64 = self.w400_frames as u64;
        let w3s_u64 = self.w3s_frames as u64;
        let w3s = self.w3s_frames;

        for f in 0..frames {
            let base = f * ch_count;
            let ring_base = self.ring_pos * ch_count;
            let idx_400_out = self.pos_400_out * ch_count;

            let mut frame_abs_peak = 0.0_f64;
            let mut sum_m_joint_sq = 0.0_f64;
            let mut sum_s_joint_sq = 0.0_f64;

            for c in 0..ch_count {
                let s = interleaved[base + c];
                let a = s.abs();
                if a > frame_abs_peak {
                    frame_abs_peak = a;
                }

                let sq_new = self.k_filters[c].process_sq(s);
                let sq_old_3s = self.ring_sq[ring_base + c];
                let sq_old_400 = self.ring_sq[idx_400_out + c];

                // Write new sample into 3s ring
                self.ring_sq[ring_base + c] = sq_new;

                let s3 = self.sum_3s[c] + sq_new - sq_old_3s;
                let s4 = self.sum_400[c] + sq_new - sq_old_400;
                self.sum_3s[c] = s3;
                self.sum_400[c] = s4;

                let w = self.itu_weights[c];
                if w > 0.0 {
                    sum_m_joint_sq += w * s4;
                    sum_s_joint_sq += w * s3;
                }
            }

            // Update monotonic deque for exact 3-second sliding Sample Peak
            let cur_idx = self.frames_seen;
            while let Some(&(_, back_v)) = self.peak_deque.back() {
                if back_v <= frame_abs_peak {
                    self.peak_deque.pop_back();
                } else {
                    break;
                }
            }
            self.peak_deque.push_back((cur_idx, frame_abs_peak));
            if cur_idx >= w3s_u64 {
                let expire = cur_idx - w3s_u64;
                while let Some(&(front_idx, _)) = self.peak_deque.front() {
                    if front_idx <= expire {
                        self.peak_deque.pop_front();
                    } else {
                        break;
                    }
                }
            }

            self.ring_pos += 1;
            if self.ring_pos == w3s {
                self.ring_pos = 0;
            }
            self.pos_400_out += 1;
            if self.pos_400_out == w3s {
                self.pos_400_out = 0;
            }
            self.frames_seen += 1;

            // Track linear energy maxima (zero `log10` calls in hot loop)
            if self.frames_seen >= w400_u64 {
                if sum_m_joint_sq > self.max_sum_m_joint {
                    self.max_sum_m_joint = sum_m_joint_sq;
                }
                for c in 0..ch_count {
                    if self.sum_400[c] > self.max_sum_m_channels[c] {
                        self.max_sum_m_channels[c] = self.sum_400[c];
                    }
                }
            }

            if self.frames_seen >= w3s_u64 {
                if sum_s_joint_sq > self.max_sum_s_joint {
                    self.max_sum_s_joint = sum_s_joint_sq;
                }
                if sum_s_joint_sq > self.psr_gate_sum_thresh {
                    if let Some(&(_, win_peak)) = self.peak_deque.front() {
                        if win_peak > 0.0 {
                            // peak^2 / sum_s_joint_sq is monotonically related to 20*log10(peak) - ST_lufs
                            let ratio = (win_peak * win_peak) / sum_s_joint_sq;
                            if ratio < self.min_psr_lin_ratio {
                                self.min_psr_lin_ratio = ratio;
                            }
                        }
                    }
                }
                for c in 0..ch_count {
                    if self.sum_3s[c] > self.max_sum_s_channels[c] {
                        self.max_sum_s_channels[c] = self.sum_3s[c];
                    }
                }
            }

            self.resync_countdown -= 1;
            if self.resync_countdown == 0 {
                self.resync_sums();
                self.resync_countdown = w3s;
            }
        }
    }

    /// Recomputes running window sums from the circular buffer every 3 seconds to eliminate
    /// any long-stream floating-point accumulation drift.
    fn resync_sums(&mut self) {
        let ch_count = self.num_channels;
        self.sum_3s.fill(0.0);
        self.sum_400.fill(0.0);

        let valid_3s = (self.frames_seen as usize).min(self.w3s_frames);
        let w3s = self.w3s_frames;
        let mut slot = self.ring_pos;
        for offset in 1..=valid_3s {
            if slot == 0 {
                slot = w3s - 1;
            } else {
                slot -= 1;
            }
            let base = slot * ch_count;
            for c in 0..ch_count {
                let v = self.ring_sq[base + c];
                self.sum_3s[c] += v;
                if offset <= self.w400_frames {
                    self.sum_400[c] += v;
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

        let dual_mono_mult = if self.num_channels > 1 { 2.0 } else { 1.0 };
        let inv_w400 = 1.0 / (self.w400_frames as f64);
        let inv_w3s = 1.0 / (self.w3s_frames as f64);

        let to_lufs = |energy: f64| -> f64 {
            if energy > 0.0 {
                LOUDNESS_OFFSET_LUFS + 10.0 * energy.log10()
            } else {
                f64::NEG_INFINITY
            }
        };

        let max_momentary_joint = to_lufs(self.max_sum_m_joint * inv_w400);
        let max_short_term_joint = to_lufs(self.max_sum_s_joint * inv_w3s);

        let max_momentary_channels = self
            .max_sum_m_channels
            .iter()
            .map(|&s| to_lufs(s * dual_mono_mult * inv_w400))
            .collect();
        let max_short_term_channels = self
            .max_sum_s_channels
            .iter()
            .map(|&s| to_lufs(s * dual_mono_mult * inv_w3s))
            .collect();

        // PSR = 20*log10(peak) - (-0.691 + 10*log10(sum_s / w3s))
        //     = 0.691 + 10*log10(w3s * (peak^2 / sum_s))
        let min_psr = if self.min_psr_lin_ratio.is_finite() && self.min_psr_lin_ratio > 0.0 {
            -LOUDNESS_OFFSET_LUFS + 10.0 * (self.min_psr_lin_ratio * (self.w3s_frames as f64)).log10()
        } else {
            0.0
        };

        LoudnessResults {
            integrated_lufs,
            lra,
            max_momentary_joint,
            max_short_term_joint,
            max_momentary_channels,
            max_short_term_channels,
            true_peaks_db,
            max_tpl_joint_db,
            min_psr,
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
