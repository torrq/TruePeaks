pub struct TrackStatsAccumulator {
    num_channels: usize,
    total_frames: u64,
    sum_sq: Vec<f64>,
    max_abs_sample: Vec<f64>,
    max_joint_lin: f64,
    max_joint_frame: u64,
    sum_l: f64,
    sum_r: f64,
    sum_lr: f64,
}

#[derive(Debug, Clone)]
pub struct TrackStatsResults {
    pub sppm_channels_db: Vec<f64>,
    pub sppm_joint_db: f64,
    pub max_sppm_frame: u64,
    pub aes17_rms_channels_db: Vec<f64>,
    pub bits_used: Option<u32>,
    pub stereo_correlation: Option<f64>,
}

impl TrackStatsAccumulator {
    pub fn new(num_channels: usize) -> Self {
        let ch = num_channels.max(1);
        Self {
            num_channels: ch,
            total_frames: 0,
            sum_sq: vec![0.0; ch],
            max_abs_sample: vec![0.0; ch],
            max_joint_lin: 0.0,
            max_joint_frame: 0,
            sum_l: 0.0,
            sum_r: 0.0,
            sum_lr: 0.0,
        }
    }

    pub fn process_frames(&mut self, interleaved: &[f64], frames: usize) {
        for f in 0..frames {
            let base = f * self.num_channels;
            let frame_idx = self.total_frames + (f as u64);

            for c in 0..self.num_channels {
                let s = interleaved[base + c];
                self.sum_sq[c] += s * s;
                let a = s.abs();
                if a > self.max_abs_sample[c] {
                    self.max_abs_sample[c] = a;
                }
                if a > self.max_joint_lin {
                    self.max_joint_lin = a;
                    self.max_joint_frame = frame_idx;
                }
            }

            if self.num_channels >= 2 {
                let l = interleaved[base];
                let r = interleaved[base + 1];
                self.sum_l += l;
                self.sum_r += r;
                self.sum_lr += l * r;
            }
        }
        self.total_frames += frames as u64;
    }

    pub fn finalize(&self, is_lossless_integer: bool, bit_mask: u32) -> TrackStatsResults {
        let mut sppm_channels_db = Vec::with_capacity(self.num_channels);

        for &peak in &self.max_abs_sample {
            let db = if peak > 0.0 {
                20.0 * peak.log10()
            } else {
                f64::NEG_INFINITY
            };
            sppm_channels_db.push(db);
        }

        let sppm_joint_db = if self.max_joint_lin > 0.0 {
            20.0 * self.max_joint_lin.log10()
        } else {
            f64::NEG_INFINITY
        };

        let mut aes17_rms_channels_db = Vec::with_capacity(self.num_channels);
        let n = self.total_frames.max(1) as f64;
        for &sq in &self.sum_sq {
            let mean_sq_aes17 = (2.0 * sq) / n;
            let db = if mean_sq_aes17 > 0.0 {
                10.0 * mean_sq_aes17.log10()
            } else {
                f64::NEG_INFINITY
            };
            aes17_rms_channels_db.push(db);
        }

        let bits_used = if is_lossless_integer {
            if bit_mask == 0 {
                Some(0)
            } else {
                Some(32 - bit_mask.trailing_zeros())
            }
        } else {
            None
        };

        let stereo_correlation = if self.num_channels >= 2 && self.total_frames > 0 {
            let n = self.total_frames as f64;
            let num = n * self.sum_lr - self.sum_l * self.sum_r;
            let var_l = (n * self.sum_sq[0] - self.sum_l * self.sum_l).max(0.0);
            let var_r = (n * self.sum_sq[1] - self.sum_r * self.sum_r).max(0.0);
            let denom = (var_l * var_r).sqrt();
            if denom > 0.0 {
                Some((num / denom).clamp(-1.0, 1.0))
            } else {
                None
            }
        } else {
            None
        };

        TrackStatsResults {
            sppm_channels_db,
            sppm_joint_db,
            max_sppm_frame: self.max_joint_frame,
            aes17_rms_channels_db,
            bits_used,
            stereo_correlation,
        }
    }
}

pub fn compute_plr(max_tpl_joint_db: f64, max_sppm_joint_db: f64, lufsi: f64) -> f64 {
    // Check Section 6.6: PLR (TP-to-Integrated Loudness Ratio)
    // Verify against reference logs: e.g., when Max. TPL > 0 dBTP, capped at 0.0 (or SPPM/TPL).
    let ref_peak = if max_tpl_joint_db > 0.0 {
        0.0_f64
    } else {
        max_tpl_joint_db.max(max_sppm_joint_db)
    };
    ref_peak - lufsi
}
