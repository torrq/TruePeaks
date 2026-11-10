pub struct TrackStatsAccumulator {
    num_channels: usize,
    total_frames: u64,
    sum_sq: Vec<f64>,
    max_abs_sample: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct TrackStatsResults {
    pub sppm_channels_db: Vec<f64>,
    pub sppm_joint_db: f64,
    pub aes17_rms_channels_db: Vec<f64>,
    pub bits_used: Option<u32>,
}

impl TrackStatsAccumulator {
    pub fn new(num_channels: usize) -> Self {
        let ch = num_channels.max(1);
        Self {
            num_channels: ch,
            total_frames: 0,
            sum_sq: vec![0.0; ch],
            max_abs_sample: vec![0.0; ch],
        }
    }

    pub fn process_frames(&mut self, interleaved: &[f64], frames: usize) {
        for f in 0..frames {
            let base = f * self.num_channels;
            for c in 0..self.num_channels {
                let s = interleaved[base + c];
                self.sum_sq[c] += s * s;
                let a = s.abs();
                if a > self.max_abs_sample[c] {
                    self.max_abs_sample[c] = a;
                }
            }
        }
        self.total_frames += frames as u64;
    }

    pub fn finalize(&self, is_lossless_integer: bool, bit_mask: u32) -> TrackStatsResults {
        let mut sppm_channels_db = Vec::with_capacity(self.num_channels);
        let mut max_joint_lin = 0.0_f64;

        for &peak in &self.max_abs_sample {
            if peak > max_joint_lin {
                max_joint_lin = peak;
            }
            let db = if peak > 0.0 {
                20.0 * peak.log10()
            } else {
                f64::NEG_INFINITY
            };
            sppm_channels_db.push(db);
        }

        let sppm_joint_db = if max_joint_lin > 0.0 {
            20.0 * max_joint_lin.log10()
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

        TrackStatsResults {
            sppm_channels_db,
            sppm_joint_db,
            aes17_rms_channels_db,
            bits_used,
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
