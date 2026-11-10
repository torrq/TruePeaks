use crate::decoder::SpeakerChannel;

pub struct DrPmfAccumulator {
    num_channels: usize,
    channels: Vec<SpeakerChannel>,
    block_frames: usize,
    current_block_frames: usize,
    current_sum_sq: Vec<f64>,
    current_peak: Vec<f64>,
    block_rms_sq: Vec<Vec<f64>>,
    block_peaks: Vec<Vec<f64>>,
}

#[derive(Debug, Clone)]
pub struct DrPmfResults {
    pub channel_dr: Vec<f64>,
    pub track_dr_raw: f64,
    pub track_dr_rounded: i32,
    pub aes17_rms_channels_db: Vec<f64>,
}

pub fn pmf_block_frames(sample_rate: u32) -> usize {
    let sr = sample_rate as usize;
    if sr > 0 && sr % 44_100 == 0 {
        let mult = sr / 44_100;
        3 * (sr + 60 * mult)
    } else {
        3 * sr.max(1)
    }
}

impl DrPmfAccumulator {
    pub fn new(sample_rate: u32, channels: &[SpeakerChannel]) -> Self {
        let num_channels = channels.len().max(1);
        let block_frames = pmf_block_frames(sample_rate);
        Self {
            num_channels,
            channels: channels.to_vec(),
            block_frames,
            current_block_frames: 0,
            current_sum_sq: vec![0.0; num_channels],
            current_peak: vec![0.0; num_channels],
            block_rms_sq: vec![Vec::new(); num_channels],
            block_peaks: vec![Vec::new(); num_channels],
        }
    }

    pub fn process_frames(&mut self, interleaved: &[f64], frames: usize) {
        let mut offset = 0usize;
        while offset < frames {
            let needed = self.block_frames - self.current_block_frames;
            let take = needed.min(frames - offset);
            let base = offset * self.num_channels;

            for f in 0..take {
                let frame_idx = base + f * self.num_channels;
                for c in 0..self.num_channels {
                    let s = interleaved[frame_idx + c];
                    self.current_sum_sq[c] += s * s;
                    let a = s.abs();
                    if a > self.current_peak[c] {
                        self.current_peak[c] = a;
                    }
                }
            }

            self.current_block_frames += take;
            offset += take;

            if self.current_block_frames == self.block_frames {
                self.commit_block(self.block_frames);
            }
        }
    }

    fn commit_block(&mut self, actual_frames: usize) {
        let denom = actual_frames.max(1) as f64;
        for c in 0..self.num_channels {
            // AES17 RMS squared = 2.0 * sum(x^2) / actual_frames
            let rms_sq = (2.0 * self.current_sum_sq[c]) / denom;
            self.block_rms_sq[c].push(rms_sq);
            self.block_peaks[c].push(self.current_peak[c]);
            self.current_sum_sq[c] = 0.0;
            self.current_peak[c] = 0.0;
        }
        self.current_block_frames = 0;
    }

    pub fn finalize(&mut self, include_lfe_in_dr: bool) -> DrPmfResults {
        if self.current_block_frames > 0 {
            self.commit_block(self.current_block_frames);
        }

        let mut channel_dr = Vec::with_capacity(self.num_channels);
        let mut aes17_rms_channels_db = Vec::with_capacity(self.num_channels);

        for c in 0..self.num_channels {
            let mut rms_sq_list = self.block_rms_sq[c].clone();
            let mut peaks_list = self.block_peaks[c].clone();
            let k = rms_sq_list.len();
            if k == 0 {
                channel_dr.push(0.0);
                aes17_rms_channels_db.push(f64::NEG_INFINITY);
                continue;
            }

            // Whole-track AES17 RMS is the quadratic mean across all K PMF blocks
            let mean_all_rms_sq: f64 = rms_sq_list.iter().sum::<f64>() / (k as f64);
            let rms_db = if mean_all_rms_sq > 0.0 {
                10.0 * mean_all_rms_sq.log10()
            } else {
                f64::NEG_INFINITY
            };
            aes17_rms_channels_db.push(rms_db);

            rms_sq_list.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
            peaks_list.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

            let m = ((k as f64 * 0.20).floor() as usize).max(1);
            let mean_top_rms_sq: f64 = rms_sq_list[..m].iter().sum::<f64>() / (m as f64);
            let r_20 = mean_top_rms_sq.sqrt();

            let p_2 = if peaks_list.len() >= 2 {
                peaks_list[1]
            } else {
                peaks_list[0]
            };

            let dr_c = if r_20 > 0.0 && p_2 > 0.0 {
                20.0 * (p_2 / r_20).log10()
            } else {
                0.0
            };
            channel_dr.push(dr_c);
        }

        // Compute Joint Track DR across active channels (excluding LFE unless configured)
        let mut active_sum = 0.0;
        let mut active_count = 0usize;
        for (c, &dr_val) in channel_dr.iter().enumerate() {
            let is_lfe = self.channels.get(c).map(|ch| ch.is_lfe()).unwrap_or(false);
            if include_lfe_in_dr || !is_lfe {
                active_sum += dr_val;
                active_count += 1;
            }
        }
        if active_count == 0 && !channel_dr.is_empty() {
            active_sum = channel_dr.iter().sum();
            active_count = channel_dr.len();
        }

        let track_dr_raw = if active_count > 0 {
            active_sum / (active_count as f64)
        } else {
            0.0
        };
        let track_dr_rounded = track_dr_raw.round() as i32;

        DrPmfResults {
            channel_dr,
            track_dr_raw,
            track_dr_rounded,
            aes17_rms_channels_db,
        }
    }
}
