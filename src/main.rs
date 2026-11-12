#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod app;
pub mod config;
pub mod decoder;
pub mod dsp;
pub mod formatter;
pub mod math_assets;
pub mod math_svg;
pub mod scanner;

use app::TruePeaksApp;

fn main() -> eframe::Result<()> {
    let mut initial_cfg = config::AppConfig::load();
    initial_cfg.sanitize_window_geometry();
    let has_saved_pos = initial_cfg.has_saved_window_position();

    let window_title = format!(
        "TruePeaks — Audio Loudness & Dynamic Range Analyzer v{}",
        env!("CARGO_PKG_VERSION")
    );
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(window_title)
        .with_inner_size([initial_cfg.window_width, initial_cfg.window_height])
        .with_min_inner_size([config::MIN_WINDOW_WIDTH, config::MIN_WINDOW_HEIGHT])
        .with_drag_and_drop(true);
    if let (Some(x), Some(y)) = (initial_cfg.window_x, initial_cfg.window_y) {
        viewport = viewport.with_position([x, y]);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        centered: !has_saved_pos,
        ..Default::default()
    };

    eframe::run_native(
        "TruePeaks",
        native_options,
        Box::new(move |cc| Ok(Box::new(TruePeaksApp::new(cc, initial_cfg)))),
    )
}

#[cfg(test)]
mod tests {
    use crate::config::{AppConfig, SurroundFolderMode};
    use crate::decoder::SpeakerChannel;
    use crate::dsp::analyze_file;
    use crate::formatter::folder_log::format_folder_log;
    use crate::formatter::single_log::format_single_file_log;
    use std::io::Write;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use unicode_width::UnicodeWidthStr;

    fn write_pcm24_stereo_wav(path: &Path, sr: u32, l: &[f64], r: &[f64]) {
        let n = l.len().min(r.len());
        let data_size = (n * 6) as u32;
        let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + data_size).to_le_bytes()).unwrap();
        f.write_all(b"WAVEfmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap();
        f.write_all(&2u16.to_le_bytes()).unwrap();
        f.write_all(&sr.to_le_bytes()).unwrap();
        f.write_all(&(sr * 6).to_le_bytes()).unwrap();
        f.write_all(&6u16.to_le_bytes()).unwrap();
        f.write_all(&24u16.to_le_bytes()).unwrap();
        f.write_all(b"data").unwrap();
        f.write_all(&data_size.to_le_bytes()).unwrap();
        for i in 0..n {
            for s in [l[i], r[i]] {
                let q = (s * 8388608.0).round().clamp(-8388608.0, 8388607.0) as i32;
                let le = q.to_le_bytes();
                f.write_all(&le[0..3]).unwrap();
            }
        }
    }

    fn write_dr_test_wav(path: &Path, target_dr: f64) {
        let b = 132_480usize; // 44.1 kHz PMF block length
        let top_amp = 10.0_f64.powf(-target_dr / 20.0);
        let mut l = Vec::with_capacity(10 * b);
        let mut r = Vec::with_capacity(10 * b);
        for blk in 0..10 {
            let scale = if blk < 2 { 1.0 } else { 0.25 };
            for j in 0..b {
                let t = ((blk * b + j) as f64) / 44_100.0;
                let s = top_amp * scale * (2.0 * std::f64::consts::PI * 997.0 * t).sin();
                let sample = if blk < 2 && j == 1000 { 0.999999 } else { s };
                l.push(sample);
                r.push(sample);
            }
        }
        write_pcm24_stereo_wav(path, 44_100, &l, &r);
    }

    fn gen_sine(sr: u32, secs: usize, freq: f64, amp: f64) -> Vec<f64> {
        let n = (sr as usize) * secs;
        (0..n)
            .map(|i| amp * (2.0 * std::f64::consts::PI * freq * (i as f64) / (sr as f64)).sin())
            .collect()
    }

    #[test]
    fn test_bs1770_calibration_and_continuous_sliding_loudness() {
        use crate::dsp::loudness::LoudnessAnalyzer;

        // Verify ITU-R BS.1770-4 calibration: 997 Hz -20.00 dBFS stereo sine (amp = 0.1)
        // Must measure -20.00 LUFS for LUFSi, Momentary, and Short-Term (with PSR ~ 0.00)
        for &(sr, tol) in &[(44_100u32, 0.005_f64), (48_000u32, 0.001_f64)] {
            let channels = vec![SpeakerChannel::FrontLeft, SpeakerChannel::FrontRight];
            let mut analyzer = LoudnessAnalyzer::new(sr, &channels).unwrap();
            let total_frames = (sr as usize) * 5;
            let mut buf = Vec::with_capacity(8192 * 2);
            let mut idx = 0usize;
            while idx < total_frames {
                let take = (total_frames - idx).min(8192);
                buf.clear();
                for _ in 0..take {
                    let v = 0.1
                        * (2.0 * std::f64::consts::PI * 997.0 * (idx as f64) / (sr as f64)).sin();
                    buf.push(v);
                    buf.push(v);
                    idx += 1;
                }
                analyzer.process_frames(&buf, take);
            }
            let res = analyzer.finalize();
            assert!(
                (res.integrated_lufs - (-20.0)).abs() < tol,
                "LUFSi calibration failed at {} Hz: {}",
                sr,
                res.integrated_lufs
            );
            assert!(
                (res.max_short_term_joint - (-20.0)).abs() < tol + 0.01,
                "Short-Term calibration failed at {} Hz: {}",
                sr,
                res.max_short_term_joint
            );
            assert!(
                (res.max_momentary_joint - (-20.0)).abs() < tol + 0.02,
                "Momentary calibration failed at {} Hz: {}",
                sr,
                res.max_momentary_joint
            );
            assert!(
                res.min_psr.abs() < 0.05,
                "Sine PSR should be ~0.00 dB at {} Hz, got {}",
                sr,
                res.min_psr
            );
        }

        // Verify K-weighting frequency response (100 Hz ~ -21.82 LUFS, 5 kHz ~ -16.67 LUFS @ -20 dBFS)
        for &(freq, expected_lufs) in &[(100.0_f64, -21.82_f64), (5000.0_f64, -16.67_f64)] {
            let channels = vec![SpeakerChannel::FrontLeft, SpeakerChannel::FrontRight];
            let mut analyzer = LoudnessAnalyzer::new(44_100, &channels).unwrap();
            let sig = gen_sine(44_100, 4, freq, 0.1);
            let interleaved: Vec<f64> = sig.iter().flat_map(|&s| [s, s]).collect();
            analyzer.process_frames(&interleaved, sig.len());
            let res = analyzer.finalize();
            assert!(
                (res.integrated_lufs - expected_lufs).abs() < 0.02,
                "K-weighting failed at {} Hz: expected {}, got {}",
                freq,
                expected_lufs,
                res.integrated_lufs
            );
        }

        // Verify continuous sample-sliding Momentary captures an isolated mid-hop 30 ms burst cleanly
        {
            let channels = vec![SpeakerChannel::FrontLeft, SpeakerChannel::FrontRight];
            let mut analyzer = LoudnessAnalyzer::new(44_100, &channels).unwrap();
            let mut sig = gen_sine(44_100, 3, 997.0, 0.056234); // -25 dBFS bed
            let start = (44_100.0 * 1.045) as usize; // intentionally offset between 100 ms grid points
            for j in 0..1323 {
                sig[start + j] =
                    0.794328 * (2.0 * std::f64::consts::PI * 4000.0 * (j as f64) / 44_100.0).sin(); // -2 dBFS 4 kHz burst
            }
            let interleaved: Vec<f64> = sig.iter().flat_map(|&s| [s, s]).collect();
            analyzer.process_frames(&interleaved, sig.len());
            let res = analyzer.finalize();
            assert!(
                res.max_momentary_joint > -11.5,
                "Expected continuous sliding Momentary peak > -11.5 LUFS, got {}",
                res.max_momentary_joint
            );
        }
    }

    #[test]
    fn test_generated_album_dr_formatting_and_unicode_alignment() {
        let cancel = AtomicBool::new(false);
        let cfg = AppConfig::default();
        let album_dir = std::env::temp_dir().join("truepeaks_album_tests");
        let _ = std::fs::remove_dir_all(&album_dir);
        std::fs::create_dir_all(&album_dir).unwrap();

        write_dr_test_wav(&album_dir.join("01 - Track\u{2019}s Title.wav"), 8.0);
        write_dr_test_wav(
            &album_dir.join("02 - \u{65e5}\u{672c}\u{8a9e}\u{30c8}\u{30e9}\u{30c3}\u{30af} \u{2014} \u{442}\u{435}\u{441}\u{442}.wav"),
            10.0,
        );
        write_dr_test_wav(&album_dir.join("03 - Caf\u{00e9} Cr\u{00e8}me.wav"), 12.0);

        let mut files: Vec<_> = std::fs::read_dir(&album_dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|d| d.path()))
            .collect();
        files.sort();

        let expected_drs = [8.0_f64, 10.0, 12.0];
        let mut tracks = Vec::with_capacity(files.len());
        for (p, &exp_dr) in files.iter().zip(&expected_drs) {
            let track = analyze_file(p, false, &cancel).unwrap();
            assert!(
                (track.dr_channels[0] - exp_dr).abs() < 0.02,
                "DR_L mismatch on {}: got {}",
                track.file_stem,
                track.dr_channels[0]
            );
            assert_eq!(track.word_length, Some(24));
            assert_eq!(track.bits_used, Some(24));
            assert!(track.max_tpl_joint >= track.max_sppm_joint - 0.01);

            let single_log = format_single_file_log(&track, &cfg.single_rows, &cfg.log_filenames);
            let lines: Vec<&str> = single_log.lines().collect();
            assert!(!single_log.contains("Absolute Path:"));
            assert_eq!(lines.len(), 18, "Single log line count mismatch");
            assert_eq!(lines[4].len(), 100);
            assert_eq!(lines[5].len(), 100);
            assert_eq!(lines[7].len(), 100);
            for row_idx in 8..18 {
                assert_eq!(lines[row_idx].len(), 106, "Metric row {} width mismatch", row_idx);
            }
            tracks.push(track);
        }

        let mean_dr: f64 = tracks.iter().map(|t| t.dr_track_raw).sum::<f64>() / (tracks.len() as f64);
        assert_eq!(mean_dr.round() as i32, 10);

        let folder_log = format_folder_log(
            &album_dir,
            &tracks,
            cfg.folder_log_style,
            &cfg.folder_columns,
            &cfg.log_filenames,
            cfg.surround_folder_mode,
        );
        let flines: Vec<&str> = folder_log.lines().collect();
        assert_eq!(flines[1], "truepeaks_album_tests");
        assert!(!flines[1].contains("Folder Path:"));
        assert_eq!(cfg.folder_log_pattern, "DR.txt");
        assert_eq!(
            crate::formatter::expand_filename_pattern(
                &cfg.folder_log_pattern,
                "truepeaks_album_tests",
                None,
                None,
                Some(10),
            ),
            "DR.txt"
        );
        assert_eq!(flines[flines.len() - 2], "Number of Files: 3");
        assert_eq!(flines.last().copied(), Some("Album DR: 10"));

        let header_line = flines[3];
        let expected_pipes: Vec<usize> = header_line
            .split('|')
            .scan(0usize, |acc, seg| {
                *acc += UnicodeWidthStr::width(seg) + 1;
                Some(*acc)
            })
            .collect();
        for row_line in &flines[5..5 + tracks.len()] {
            let row_pipes: Vec<usize> = row_line
                .split('|')
                .scan(0usize, |acc, seg| {
                    *acc += UnicodeWidthStr::width(seg) + 1;
                    Some(*acc)
                })
                .collect();
            assert_eq!(row_pipes, expected_pipes, "Visual alignment mismatch: {}", row_line);
        }

        let _ = std::fs::remove_dir_all(&album_dir);
    }

    #[test]
    fn test_formats_bits_used_and_high_sample_rates() {
        let cancel = AtomicBool::new(false);
        let temp_dir = std::env::temp_dir().join("truepeaks_format_tests");
        let _ = std::fs::create_dir_all(&temp_dir);

        // 1. 24-bit WAV at 384 kHz where only upper 16 bits are active (low 8 bits = 0)
        let wav_384k = temp_dir.join("test_384k_24in16.wav");
        {
            let sr = 384_000u32;
            let channels = 2u16;
            let bits = 24u16;
            let bytes_per_sample = 3u16;
            let block_align = channels * bytes_per_sample;
            let byte_rate = sr * (block_align as u32);
            let num_frames = 38_400usize; // 0.1s
            let data_size = (num_frames * block_align as usize) as u32;

            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"RIFF");
            bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
            bytes.extend_from_slice(b"WAVE");
            bytes.extend_from_slice(b"fmt ");
            bytes.extend_from_slice(&16u32.to_le_bytes());
            bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
            bytes.extend_from_slice(&channels.to_le_bytes());
            bytes.extend_from_slice(&sr.to_le_bytes());
            bytes.extend_from_slice(&byte_rate.to_le_bytes());
            bytes.extend_from_slice(&block_align.to_le_bytes());
            bytes.extend_from_slice(&bits.to_le_bytes());
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&data_size.to_le_bytes());

            for i in 0..num_frames {
                // 16-bit value shifted into 24-bit container (low byte 0x00, bit 8 active)
                let s16: i16 = if i % 2 == 0 { 0x4001 } else { -0x4001 };
                let s24: i32 = (s16 as i32) << 8;
                let le = s24.to_le_bytes();
                bytes.extend_from_slice(&le[0..3]);
                bytes.extend_from_slice(&le[0..3]);
            }
            std::fs::write(&wav_384k, &bytes).unwrap();
        }

        let res_384k = analyze_file(&wav_384k, false, &cancel).unwrap();
        assert_eq!(res_384k.sample_rate, 384_000);
        assert_eq!(res_384k.word_length, Some(24));
        assert_eq!(res_384k.bits_used, Some(16));
        assert_eq!(crate::formatter::format_sample_rate(res_384k.sample_rate), "384k");

        // 2. RF64 / .bwf with ds64 chunk
        let bwf_rf64 = temp_dir.join("test_rf64.bwf");
        {
            let sr = 96_000u32;
            let channels = 2u16;
            let bits = 24u16;
            let block_align = 6u16;
            let byte_rate = sr * 6;
            let num_frames = 9600usize;
            let data_size = (num_frames * 6) as u64;

            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"RF64");
            bytes.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
            bytes.extend_from_slice(b"WAVE");
            // ds64 chunk (28 bytes)
            bytes.extend_from_slice(b"ds64");
            bytes.extend_from_slice(&28u32.to_le_bytes());
            bytes.extend_from_slice(&(64 + data_size).to_le_bytes()); // riffSize64
            bytes.extend_from_slice(&data_size.to_le_bytes()); // dataSize64
            bytes.extend_from_slice(&(num_frames as u64).to_le_bytes()); // sampleCount64
            bytes.extend_from_slice(&0u32.to_le_bytes()); // tableLength
            // fmt chunk
            bytes.extend_from_slice(b"fmt ");
            bytes.extend_from_slice(&16u32.to_le_bytes());
            bytes.extend_from_slice(&1u16.to_le_bytes());
            bytes.extend_from_slice(&channels.to_le_bytes());
            bytes.extend_from_slice(&sr.to_le_bytes());
            bytes.extend_from_slice(&byte_rate.to_le_bytes());
            bytes.extend_from_slice(&block_align.to_le_bytes());
            bytes.extend_from_slice(&bits.to_le_bytes());
            // data chunk
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
            for i in 0..num_frames {
                // Full 24-bit depth (LSB bit 0 active)
                let s24: i32 = if i % 2 == 0 { 0x20_0001 } else { -0x20_0001 };
                let le = s24.to_le_bytes();
                bytes.extend_from_slice(&le[0..3]);
                bytes.extend_from_slice(&le[0..3]);
            }
            std::fs::write(&bwf_rf64, &bytes).unwrap();
        }

        let res_rf64 = analyze_file(&bwf_rf64, false, &cancel).unwrap();
        assert_eq!(res_rf64.sample_rate, 96_000);
        assert_eq!(res_rf64.word_length, Some(24));
        assert_eq!(res_rf64.bits_used, Some(24));

        // 3. Sony Wave64 (.w64)
        let w64_path = temp_dir.join("test_wave64.w64");
        {
            const GUID_RIFF: [u8; 16] = [
                0x72, 0x69, 0x66, 0x66, 0x2E, 0x91, 0xCF, 0x11, 0xA5, 0xD6, 0x28, 0xDB, 0x04, 0xC1,
                0x00, 0x00,
            ];
            const GUID_WAVE: [u8; 16] = [
                0x77, 0x61, 0x76, 0x65, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E,
                0xDB, 0x8A,
            ];
            const GUID_FMT: [u8; 16] = [
                0x66, 0x6D, 0x74, 0x20, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E,
                0xDB, 0x8A,
            ];
            const GUID_DATA: [u8; 16] = [
                0x64, 0x61, 0x74, 0x61, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E,
                0xDB, 0x8A,
            ];

            let sr = 48_000u32;
            let num_frames = 4800usize;
            let pcm_bytes = (num_frames * 4) as u64; // 16-bit stereo
            let fmt_chunk_size = 24u64 + 16u64; // 40 bytes (already 8-byte aligned)
            let data_chunk_size = 24u64 + pcm_bytes;
            let total_size = 40u64 + fmt_chunk_size + data_chunk_size;

            let mut bytes = Vec::new();
            bytes.extend_from_slice(&GUID_RIFF);
            bytes.extend_from_slice(&total_size.to_le_bytes());
            bytes.extend_from_slice(&GUID_WAVE);
            // fmt
            bytes.extend_from_slice(&GUID_FMT);
            bytes.extend_from_slice(&fmt_chunk_size.to_le_bytes());
            bytes.extend_from_slice(&1u16.to_le_bytes());
            bytes.extend_from_slice(&2u16.to_le_bytes());
            bytes.extend_from_slice(&sr.to_le_bytes());
            bytes.extend_from_slice(&(sr * 4).to_le_bytes());
            bytes.extend_from_slice(&4u16.to_le_bytes());
            bytes.extend_from_slice(&16u16.to_le_bytes());
            // data
            bytes.extend_from_slice(&GUID_DATA);
            bytes.extend_from_slice(&data_chunk_size.to_le_bytes());
            for i in 0..num_frames {
                let s16: i16 = if i % 2 == 0 { 16384 } else { -16384 };
                bytes.extend_from_slice(&s16.to_le_bytes());
                bytes.extend_from_slice(&s16.to_le_bytes());
            }
            std::fs::write(&w64_path, &bytes).unwrap();
        }

        let res_w64 = analyze_file(&w64_path, false, &cancel).unwrap();
        assert_eq!(res_w64.sample_rate, 48_000);
        assert_eq!(res_w64.word_length, Some(16));
        assert_eq!(res_w64.bits_used, Some(2)); // +/- 16384 uses bit 14 -> 2 MSB bits

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_surround_5_1_and_7_1_channel_weighting_and_lfe_exclusion() {
        let cancel = AtomicBool::new(false);
        let temp_dir = std::env::temp_dir().join("truepeaks_surround_tests");
        let _ = std::fs::create_dir_all(&temp_dir);

        // Create a 5.1 WAVEFORMATEXTENSIBLE file (L, R, C, LFE, Ls, Rs) at 48 kHz, 6 seconds (2 PMF blocks)
        let wav_5_1 = temp_dir.join("surround_5_1.wav");
        let sr = 48_000u32;
        let channels = 6u16;
        let num_frames = (sr as usize) * 6; // 6 seconds -> 2 blocks of 3s
        let block_align = channels * 2;
        let data_size = (num_frames * block_align as usize) as u32;

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(60 + data_size).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&0xFFFEu16.to_le_bytes()); // WAVE_FORMAT_EXTENSIBLE
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sr.to_le_bytes());
        bytes.extend_from_slice(&(sr * block_align as u32).to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        bytes.extend_from_slice(&16u16.to_le_bytes()); // wValidBitsPerSample
        // dwChannelMask = FL(1) | FR(2) | FC(4) | LFE(8) | BL(16) | BR(32) = 0x3F
        bytes.extend_from_slice(&0x0000_003Fu32.to_le_bytes());
        // SubFormat = KSDATAFORMAT_SUBTYPE_PCM (00000001-0000-0010-8000-00AA00389B71)
        bytes.extend_from_slice(&[
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38,
            0x9B, 0x71,
        ]);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_size.to_le_bytes());

        for i in 0..num_frames {
            let phase = 2.0 * std::f64::consts::PI * 997.0 * (i as f64) / (sr as f64);
            let s = phase.sin();
            // L, R, C, Ls, Rs have step-modulated amplitude (block 1 = 1.0, block 2 = 0.5)
            // LFE has a constant near-0 dBFS square wave (DR ~ 3 dB)
            let env = if i < (sr as usize) * 3 { 1.0 } else { 0.5 };
            let active_mod = ((s * 16384.0 * env) as i16).clamp(-32767, 32767);
            let lfe_val: i16 = if s >= 0.0 { 30000 } else { -30000 }; // Near 0 dBFS square wave in LFE

            for c in 0..6 {
                let v = if c == 3 { lfe_val } else { active_mod };
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        std::fs::write(&wav_5_1, &bytes).unwrap();

        let track_no_lfe_dr = analyze_file(&wav_5_1, false, &cancel).unwrap();
        let track_with_lfe_dr = analyze_file(&wav_5_1, true, &cancel).unwrap();

        assert_eq!(
            track_no_lfe_dr.channels,
            vec![
                SpeakerChannel::FrontLeft,
                SpeakerChannel::FrontRight,
                SpeakerChannel::FrontCenter,
                SpeakerChannel::Lfe,
                SpeakerChannel::LeftSurround,
                SpeakerChannel::RightSurround,
            ]
        );

        // Verify LFE channel (index 3) has near 0 dBFS SPPM & RMS, while active channels are lower
        assert!(track_no_lfe_dr.max_sppm_channels[3] > -1.0);
        // Verify LFE exclusion in DR: track_no_lfe_dr.dr_track_raw differs from track_with_lfe_dr.dr_track_raw
        assert!(
            (track_no_lfe_dr.dr_track_raw - track_with_lfe_dr.dr_track_raw).abs() > 0.1,
            "Expected LFE inclusion toggle to change joint DR: no_lfe={}, with_lfe={}",
            track_no_lfe_dr.dr_track_raw,
            track_with_lfe_dr.dr_track_raw
        );

        // Verify Single-File Log dynamically scales width for 6 channels (49 + 7 * 19 = 182 chars)
        // Note: For >2 channels, Line 3 is "Channel Layout: ...", so metric rows start at Line 9
        let mut cfg = AppConfig::default();
        let single_log = format_single_file_log(&track_no_lfe_dr, &cfg.single_rows, &cfg.log_filenames);
        assert!(!single_log.contains("Absolute Path:"));
        let s_lines: Vec<&str> = single_log.lines().collect();
        assert!(s_lines[3].starts_with("Channel Layout:"));
        assert_eq!(s_lines[9].len(), 49 + 7 * 19);

        // Verify Surround Folder Log expanded mode works cleanly
        cfg.surround_folder_mode = SurroundFolderMode::ExpandPerChannel;
        cfg.folder_columns.max_tpl_left = true;
        cfg.folder_columns.dr_left = true;
        let folder_log = format_folder_log(
            &temp_dir,
            &[track_no_lfe_dr],
            cfg.folder_log_style,
            &cfg.folder_columns,
            &cfg.log_filenames,
            cfg.surround_folder_mode,
        );
        assert!(folder_log.contains("TPL L"));
        assert!(folder_log.contains("TPL LFE"));
        assert!(folder_log.contains("DR LFE"));
        assert_eq!(
            folder_log.lines().nth(1),
            temp_dir.file_name().and_then(|s| s.to_str())
        );
        assert!(folder_log.contains("Number of Files: 1") && folder_log.contains("Album DR: "));
        assert_eq!(
            crate::formatter::expand_filename_pattern("  ", "Album", None, None, Some(10)),
            "DR.txt"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_true_peak_16x_sinc_and_diagnostic_probes() {
        let bessel_i0 = |x: f64| -> f64 {
            let mut sum = 1.0;
            let mut term = 1.0;
            let y = x * x / 4.0;
            for k in 1..28 {
                term *= y / ((k * k) as f64);
                sum += term;
            }
            sum
        };
        let beta = 9.0_f64;
        let i0_beta = bessel_i0(beta);
        const HALF_TAPS: isize = 24;
        const KERNEL_LEN: usize = 49;
        let mut phases = vec![[0.0_f64; KERNEL_LEN]; 16];
        let mut l1_bound = 0.0_f64;
        for sub in 0..16 {
            let frac = (sub as f64) / 16.0;
            let mut wsum = 0.0;
            for (idx, k) in (-HALF_TAPS..=HALF_TAPS).enumerate() {
                let d = frac - (k as f64);
                let alpha = d / (HALF_TAPS as f64 + 0.5);
                let c = if alpha.abs() < 1.0 {
                    let w = bessel_i0(beta * (1.0 - alpha * alpha).sqrt()) / i0_beta;
                    let sinc = if d.abs() < 1e-12 {
                        1.0
                    } else {
                        let x = std::f64::consts::PI * d;
                        x.sin() / x
                    };
                    w * sinc
                } else {
                    0.0
                };
                phases[sub][idx] = c;
                wsum += c;
            }
            let mut l1 = 0.0_f64;
            for v in &mut phases[sub] {
                *v /= wsum;
                l1 += v.abs();
            }
            if l1 > l1_bound {
                l1_bound = l1;
            }
        }

        let whole_file_16x_sinc_max = |samples: &[f64]| -> f64 {
            let n = samples.len();
            let mut best = samples.iter().fold(0.0_f64, |acc, &v| acc.max(v.abs()));
            const BLK: usize = 32;
            let n_blks = (n + BLK - 1) / BLK;
            let blk_max: Vec<f64> = (0..n_blks)
                .map(|b| {
                    let s = b * BLK;
                    let e = ((b + 1) * BLK).min(n);
                    samples[s..e].iter().fold(0.0_f64, |acc, &v| acc.max(v.abs()))
                })
                .collect();
            for b in 0..n_blks {
                let left_m = if b > 0 { blk_max[b - 1] } else { 0.0 };
                let mid_m = blk_max[b];
                let right_m = if b + 1 < n_blks { blk_max[b + 1] } else { 0.0 };
                if left_m.max(mid_m).max(right_m) * l1_bound <= best {
                    continue;
                }
                let s = b * BLK;
                let e = ((b + 1) * BLK).min(n);
                for pos in s..e {
                    if pos >= HALF_TAPS as usize && pos + (HALF_TAPS as usize) < n {
                        let win = &samples[pos - HALF_TAPS as usize..=pos + HALF_TAPS as usize];
                        for coeffs in phases.iter().skip(1) {
                            let acc: f64 = win.iter().zip(coeffs.iter()).map(|(&w, &c)| w * c).sum();
                            let val = acc.abs();
                            if val > best {
                                best = val;
                            }
                        }
                    }
                }
            }
            20.0 * best.log10()
        };

        let cancel = AtomicBool::new(false);
        let diag_dir = std::env::temp_dir().join("truepeaks_diag_tests");
        let _ = std::fs::remove_dir_all(&diag_dir);
        std::fs::create_dir_all(&diag_dir).unwrap();

        // 05: Exact ITU fs/4 Inter-Sample Peak (-9.03 dBFS samples, ~ -6.02 dBTP continuous)
        let isp_fs4: Vec<f64> = (0..44_100 * 4)
            .map(|i| 0.5 * (std::f64::consts::FRAC_PI_2 * (i as f64) + std::f64::consts::FRAC_PI_4).sin())
            .collect();
        let p05 = diag_dir.join("05_isp_fs4.wav");
        write_pcm24_stereo_wav(&p05, 44_100, &isp_fs4, &isp_fs4);
        let t05 = analyze_file(&p05, false, &cancel).unwrap();
        assert!((t05.max_sppm_joint - (-9.03)).abs() < 0.02);
        assert!(t05.max_tpl_joint > -6.10 && t05.max_tpl_joint < -5.85);

        // 06: Clipped high-frequency intersample burst vs whole-file 16x Kaiser Sinc
        let mut clipped_isp = gen_sine(44_100, 4, 440.0, 0.2);
        let burst = [-0.75, -0.85, -0.50, -0.9999, -0.996, -0.40, -0.9999, -0.75, -0.90, -0.999];
        for (k, &v) in burst.iter().enumerate() {
            clipped_isp[44_100 * 2 + k] = v;
        }
        let p06 = diag_dir.join("06_clipped_burst.wav");
        write_pcm24_stereo_wav(&p06, 44_100, &clipped_isp, &clipped_isp);
        let t06 = analyze_file(&p06, false, &cancel).unwrap();
        let gt06 = whole_file_16x_sinc_max(&clipped_isp);
        assert!(gt06 > 1.40, "Expected 16x Sinc > +1.40 dBTP, got {}", gt06);
        assert!(
            (t06.max_tpl_joint - gt06).abs() < 0.06,
            "Expected ebur128 close to 16x Sinc: eb={}, 16x={}",
            t06.max_tpl_joint,
            gt06
        );

        // 07: 1-sample -6.02 dBFS spikes every 2.5s across 12s -> TPL >= SPPM, DR=14, Min. PSR ~ 13.97
        let mut psr_a = gen_sine(44_100, 12, 997.0, 0.1);
        for k in 0..5 {
            let idx = ((0.5 + (k as f64) * 2.5) * 44_100.0) as usize;
            psr_a[idx] = 0.5;
        }
        let p07 = diag_dir.join("07_psr_spikes.wav");
        write_pcm24_stereo_wav(&p07, 44_100, &psr_a, &psr_a);
        let t07 = analyze_file(&p07, false, &cancel).unwrap();
        assert!(t07.max_tpl_joint >= t07.max_sppm_joint - 0.01);
        assert_eq!(t07.dr_track_rounded, 14);
        assert!((t07.min_psr - 13.97).abs() < 0.08);

        // 08: 15ms 4kHz bursts every 1.5s -> RMS=-19.07, TPL~16x Sinc, Min. PSR ~ 12.15
        let mut psr_b = gen_sine(44_100, 12, 997.0, 0.1);
        for k in 0..8 {
            let start = ((0.5 + (k as f64) * 1.5) * 44_100.0) as usize;
            for j in 0..662 {
                psr_b[start + j] = 0.5 * (2.0 * std::f64::consts::PI * 4000.0 * (j as f64) / 44_100.0).sin();
            }
        }
        let p08 = diag_dir.join("08_psr_bursts.wav");
        write_pcm24_stereo_wav(&p08, 44_100, &psr_b, &psr_b);
        let t08 = analyze_file(&p08, false, &cancel).unwrap();
        let gt08 = whole_file_16x_sinc_max(&psr_b);
        assert!((t08.rms_channels[0] - (-19.07)).abs() < 0.02);
        assert!((t08.max_tpl_joint - gt08).abs() < 0.04);
        assert!((t08.min_psr - 12.15).abs() < 0.10);

        // 14: Exact 5.3145 / 5.6945 DR tie + 27s tail -> DR=6, TPL>=0.00-0.01, Min. PSR ~ 0.18
        {
            let b = 132_480usize;
            let mut l_tie = Vec::with_capacity(10 * b);
            let mut r_tie = Vec::with_capacity(10 * b);
            let amp_l = 10.0_f64.powf(-5.3145 / 20.0);
            let amp_r = 10.0_f64.powf(-5.6945 / 20.0);
            for blk in 0..10 {
                let scale = if blk < 2 { 1.0 } else { 0.25 };
                for j in 0..b {
                    let t = ((blk * b + j) as f64) / 44_100.0;
                    let phase = (2.0 * std::f64::consts::PI * 997.0 * t).sin();
                    let (mut sl, mut sr) = (amp_l * scale * phase, amp_r * scale * phase);
                    if blk < 2 && j == 1000 {
                        sl = 0.999999;
                        sr = 0.999999;
                    }
                    l_tie.push(sl);
                    r_tie.push(sr);
                }
            }
            let p14 = diag_dir.join("14_dr_tie.wav");
            write_pcm24_stereo_wav(&p14, 44_100, &l_tie, &r_tie);
            let t14 = analyze_file(&p14, false, &cancel).unwrap();
            assert_eq!(t14.dr_track_rounded, 6);
            assert!(t14.max_tpl_joint >= -0.01);
            assert!((t14.min_psr - 0.18).abs() < 0.05);
        }

        let _ = std::fs::remove_dir_all(&diag_dir);
    }

    #[test]
    fn test_theme_config_defaults_and_presets() {
        use crate::config::{FolderColumnsConfig, FolderLogStyle, ThemeMode};

        let def = AppConfig::default();
        assert_eq!(def.theme, ThemeMode::System);
        assert_eq!(def.folder_log_style, FolderLogStyle::TruePeaksTable);
        assert!(!def.recurse_subfolders);
        assert!(!def.combine_dropped_folders);
        assert!(!def.overwrite_existing_logs);
        assert!(!def.log_filenames.limit_length);
        assert_eq!(def.log_filenames.max_length, 64);
        assert!(!def.log_filenames.strip_non_ascii);
        assert_eq!(def.folder_log_pattern, "DR.txt");
        assert!((def.log_font_size - 13.5).abs() < 1e-6);
        assert_eq!(def.window_x, None);
        assert_eq!(def.window_y, None);
        assert!((def.window_width - crate::config::DEFAULT_WINDOW_WIDTH).abs() < 1e-6);
        assert!((def.window_height - crate::config::DEFAULT_WINDOW_HEIGHT).abs() < 1e-6);
        assert!(!def.has_saved_window_position());

        assert_eq!(FolderLogStyle::all().len(), 3);
        assert_eq!(ThemeMode::System.next(), ThemeMode::Dark);
        assert_eq!(ThemeMode::Dark.next(), ThemeMode::Light);
        assert_eq!(ThemeMode::Light.next(), ThemeMode::System);

        // Existing configs without new fields must deserialize to safe defaults
        let parsed: AppConfig = toml::from_str("save_logs_to_disk = true\n").unwrap();
        assert_eq!(parsed.theme, ThemeMode::System);
        assert_eq!(parsed.folder_log_style, FolderLogStyle::TruePeaksTable);
        assert!(!parsed.recurse_subfolders);
        assert!(!parsed.combine_dropped_folders);
        assert!(!parsed.overwrite_existing_logs);
        assert!(!parsed.log_filenames.limit_length);
        assert_eq!(parsed.log_filenames.max_length, 64);
        assert!(!parsed.log_filenames.strip_non_ascii);
        assert_eq!(parsed.window_x, None);
        assert!((parsed.window_width - 980.0).abs() < 1e-6);
        assert!((parsed.window_height - 660.0).abs() < 1e-6);

        // Window geometry sanitization: reject Windows minimized (-32000) & clamp size, keep valid secondary monitor coords
        let mut bad_win = AppConfig::default();
        bad_win.window_x = Some(-32000.0);
        bad_win.window_y = Some(-32000.0);
        bad_win.window_width = 160.0;
        bad_win.window_height = f32::NAN;
        bad_win.sanitize_window_geometry();
        assert_eq!(bad_win.window_x, None);
        assert_eq!(bad_win.window_y, None);
        assert!((bad_win.window_width - crate::config::MIN_WINDOW_WIDTH).abs() < 1e-6);
        assert!((bad_win.window_height - crate::config::DEFAULT_WINDOW_HEIGHT).abs() < 1e-6);

        let mut dual_mon = AppConfig::default();
        dual_mon.window_x = Some(-1280.0);
        dual_mon.window_y = Some(90.0);
        dual_mon.window_width = 1120.0;
        dual_mon.window_height = 740.0;
        dual_mon.sanitize_window_geometry();
        assert_eq!(dual_mon.window_x, Some(-1280.0));
        assert_eq!(dual_mon.window_y, Some(90.0));
        assert!((dual_mon.window_width - 1120.0).abs() < 1e-6);
        assert!((dual_mon.window_height - 740.0).abs() < 1e-6);

        let minimal = FolderColumnsConfig::minimal();
        assert!(minimal.dr_pmf && !minimal.max_tpl_left && !minimal.lra);
        assert_eq!(minimal.active_count(), (6, 26));
        let mastering = FolderColumnsConfig::mastering();
        assert!(mastering.lra && mastering.plr && mastering.min_psr && !mastering.dr_left);
        let all = FolderColumnsConfig::all_enabled();
        assert!(all.file_name && all.bits_used && all.plr && all.min_psr);
        assert_eq!(all.active_count(), (26, 26));
    }

    #[test]
    fn test_folder_log_styles_tree_and_multi_drop() {
        use crate::config::FolderLogStyle;
        use crate::scanner::{spawn_scan_job, ScanEvent, ScanTarget};
        use crossbeam_channel::unbounded;
        use std::path::PathBuf;
        use std::sync::Arc;

        let run_scan_sync = |target: ScanTarget, cfg: AppConfig| -> (String, Vec<PathBuf>, usize, usize) {
            let (tx, rx) = unbounded();
            let cancel = Arc::new(AtomicBool::new(false));
            spawn_scan_job(target, cfg, cancel, tx);
            for ev in rx.iter() {
                match ev {
                    ScanEvent::Completed {
                        log_text,
                        saved_paths,
                        tracks_count,
                        folders_count,
                        ..
                    } => return (log_text, saved_paths, tracks_count, folders_count),
                    ScanEvent::Error(e) => panic!("Unexpected scan error: {}", e),
                    ScanEvent::Cancelled => panic!("Unexpected scan cancellation"),
                    ScanEvent::Started { .. } | ScanEvent::Progress { .. } => {}
                }
            }
            panic!("Scan channel closed before completion")
        };

        let temp_root = std::env::temp_dir().join("truepeaks_styles_tree_tests");
        let _ = std::fs::remove_dir_all(&temp_root);
        let disc1 = temp_root.join("Disc 1");
        let bonus = disc1.join("Bonus");
        std::fs::create_dir_all(&bonus).unwrap();

        let p_intro = temp_root.join("01_intro.wav");
        let p_disc1 = disc1.join("01_track_a.wav");
        let p_bonus = bonus.join("02_track_b.wav");
        write_dr_test_wav(&p_intro, 10.0);
        write_dr_test_wav(&p_disc1, 8.0);
        write_dr_test_wav(&p_bonus, 12.0);

        let cancel = AtomicBool::new(false);
        let tracks = vec![
            analyze_file(&p_intro, false, &cancel).unwrap(),
            analyze_file(&p_disc1, false, &cancel).unwrap(),
            analyze_file(&p_bonus, false, &cancel).unwrap(),
        ];

        // 1. Classic foo_dr style: omits foobar2000 & log date lines, matches exact columns & footer
        let cfg = AppConfig::default();
        let classic = format_folder_log(
            &temp_root,
            &tracks,
            FolderLogStyle::ClassicFooDr,
            &cfg.folder_columns,
            &cfg.log_filenames,
            cfg.surround_folder_mode,
        );
        assert!(!classic.contains("foobar2000"));
        assert!(!classic.contains("Dynamic Range Meter"));
        assert!(!classic.contains("log date:"));
        let clines: Vec<&str> = classic.lines().collect();
        assert_eq!(clines[0], "-".repeat(80));
        assert_eq!(clines[1], "Analyzed: truepeaks_styles_tree_tests");
        assert_eq!(clines[4], "DR         Peak         RMS     Duration Track");
        assert!(clines[6].starts_with("DR10     "));
        assert!(clines[6].contains(" dB"));
        assert!(clines[6].ends_with("01_intro"));
        assert!(classic.contains("Number of tracks:  3"));
        assert!(classic.contains("Official DR value: DR10"));
        assert!(classic.contains("Samplerate:        44100 Hz"));
        assert!(classic.contains("Channels:          2"));
        assert!(classic.contains("Bits per sample:   24"));
        assert!(classic.contains("Bitrate:           "));
        assert!(classic.contains("Codec:             PCM (WAV)"));
        assert_eq!(clines.last().copied(), Some("=".repeat(80).as_str()));

        // 2. Compact Mastering style: pure ASCII with summary header + key dynamics columns
        let compact = format_folder_log(
            &temp_root,
            &tracks,
            FolderLogStyle::CompactMastering,
            &cfg.folder_columns,
            &cfg.log_filenames,
            cfg.surround_folder_mode,
        );
        assert!(compact.is_ascii());
        assert!(compact.contains("Album:   truepeaks_styles_tree_tests"));
        assert!(compact.contains("Summary: DR10 (Official) | 3 tracks"));
        assert!(compact.contains("Metrics: Avg "));
        assert!(compact.contains("TPL dBTP"));
        assert!(compact.contains("LUFSi"));
        assert!(compact.contains("PSR"));

        // 3. Default scan does NOT recurse into subdirectories (only sees 1 root file)
        let mut flat_cfg = AppConfig::default();
        flat_cfg.save_logs_to_disk = false;
        let (_flat_log, flat_saved, flat_tracks, flat_folders) =
            run_scan_sync(ScanTarget::Folder(temp_root.clone()), flat_cfg);
        assert!(flat_saved.is_empty());
        assert_eq!(flat_tracks, 1);
        assert_eq!(flat_folders, 1);

        // 4. Recursive scan discovers nested subdirectories without ASCII directory tree diagram
        let mut rec_cfg = AppConfig::default();
        rec_cfg.recurse_subfolders = true;
        rec_cfg.save_logs_to_disk = false;
        let (rec_log, _, rec_tracks, _) =
            run_scan_sync(ScanTarget::Folder(temp_root.clone()), rec_cfg);
        assert_eq!(rec_tracks, 3);
        assert!(rec_log.is_ascii());
        assert!(!rec_log.contains("Directory Tree:"));
        assert!(!rec_log.contains("|-- "));
        assert!(!rec_log.contains("\\-- "));
        assert!(rec_log.contains("Directory: truepeaks_styles_tree_tests/Disc 1  (DR8 | 1 track)"));
        assert!(rec_log.contains("Directory: truepeaks_styles_tree_tests/Disc 1/Bonus  (DR12 | 1 track)"));
        assert!(rec_log.contains("Scan Summary: 3 folders | 3 tracks | Official Overall DR: 10"));
        assert!(!rec_log.lines().any(|l| l.trim() == "truepeaks_styles_tree_tests"));
        assert!(!rec_log.lines().any(|l| l.trim() == "Disc 1"));
        assert!(!rec_log.lines().any(|l| l.trim() == "Bonus"));
        assert!(!rec_log.contains("Number of Files:"));
        assert!(!rec_log.contains("Album DR:"));

        // 5. Multi-folder drop: separate log per directory by default
        let multi_root = temp_root.join("MultiDrop");
        let dir_a = multi_root.join("Album A");
        let dir_b = multi_root.join("Album B");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        write_dr_test_wav(&dir_a.join("01_a.wav"), 8.0);
        write_dr_test_wav(&dir_b.join("01_b.wav"), 12.0);

        let sep_cfg = AppConfig::default();
        let (_sep_log, sep_saved, sep_tracks, sep_folders) = run_scan_sync(
            ScanTarget::Multi {
                dirs: vec![dir_a.clone(), dir_b.clone()],
                files: Vec::new(),
            },
            sep_cfg,
        );
        assert_eq!(sep_tracks, 2);
        assert_eq!(sep_folders, 2);
        assert_eq!(sep_saved.len(), 2);
        assert!(dir_a.join("DR.txt").is_file());
        assert!(dir_b.join("DR.txt").is_file());

        // 6. Multi-folder drop with combine_dropped_folders = true -> single combined log
        let _ = std::fs::remove_file(dir_a.join("DR.txt"));
        let _ = std::fs::remove_file(dir_b.join("DR.txt"));
        let mut comb_cfg = AppConfig::default();
        comb_cfg.combine_dropped_folders = true;
        let (comb_log, comb_saved, comb_tracks, comb_folders) = run_scan_sync(
            ScanTarget::Multi {
                dirs: vec![dir_a.clone(), dir_b.clone()],
                files: Vec::new(),
            },
            comb_cfg,
        );
        assert_eq!(comb_tracks, 2);
        assert_eq!(comb_folders, 2);
        assert_eq!(comb_saved.len(), 1);
        assert_eq!(comb_saved[0], multi_root.join("DR.txt"));
        assert!(multi_root.join("DR.txt").is_file());
        assert!(!dir_a.join("DR.txt").exists());
        assert!(!dir_b.join("DR.txt").exists());
        assert!(comb_log.contains("Combined Batch Analysis: 2 folders | 2 tracks | Official Overall DR: 10"));
        assert!(!comb_log.contains("Dropped Folders"));
        assert!(!comb_log.contains("|-- "));
        assert!(!comb_log.contains("\\-- "));
        assert!(comb_log.contains("Directory: Album A  (DR8 | 1 track)"));
        assert!(comb_log.contains("Directory: Album B  (DR12 | 1 track)"));
        assert!(!comb_log.lines().any(|l| l.trim() == "Album A"));
        assert!(!comb_log.lines().any(|l| l.trim() == "Album B"));
        assert!(!comb_log.contains("Number of Files:"));
        assert!(!comb_log.contains("Album DR:"));
        assert!(comb_log.contains("Combined Summary: 2 folders | 2 tracks | Official Overall DR: 10"));

        let mut single_rec_cfg = AppConfig::default();
        single_rec_cfg.recurse_subfolders = true;
        single_rec_cfg.save_logs_to_disk = false;
        let (single_rec_log, _, single_rec_tracks, single_rec_folders) =
            run_scan_sync(ScanTarget::Folder(dir_a.clone()), single_rec_cfg);
        assert_eq!(single_rec_tracks, 1);
        assert_eq!(single_rec_folders, 1);
        assert!(!single_rec_log.contains("Directory: Album A"));
        assert!(single_rec_log.lines().any(|l| l.trim() == "Album A"));
        assert!(single_rec_log.contains("Number of Files: 1"));
        assert!(single_rec_log.contains("Album DR: 8"));
        assert!(!single_rec_log.contains("Scan Summary:"));

        let mut comb_classic_cfg = AppConfig::default();
        comb_classic_cfg.combine_dropped_folders = true;
        comb_classic_cfg.save_logs_to_disk = false;
        comb_classic_cfg.folder_log_style = FolderLogStyle::ClassicFooDr;
        let (comb_classic_log, _, _, _) = run_scan_sync(
            ScanTarget::Multi {
                dirs: vec![dir_a.clone(), dir_b.clone()],
                files: Vec::new(),
            },
            comb_classic_cfg,
        );
        assert!(comb_classic_log.contains("Directory: Album A  (DR8 | 1 track)"));
        assert!(!comb_classic_log.contains("Analyzed: Album A"));
        assert!(!comb_classic_log.contains("Analyzed: Album B"));

        let mut comb_compact_cfg = AppConfig::default();
        comb_compact_cfg.combine_dropped_folders = true;
        comb_compact_cfg.save_logs_to_disk = false;
        comb_compact_cfg.folder_log_style = FolderLogStyle::CompactMastering;
        let (comb_compact_log, _, _, _) = run_scan_sync(
            ScanTarget::Multi {
                dirs: vec![dir_a.clone(), dir_b.clone()],
                files: Vec::new(),
            },
            comb_compact_cfg,
        );
        assert!(comb_compact_log.contains("Directory: Album A  (DR8 | 1 track)"));
        assert!(!comb_compact_log.contains("Album:   Album A"));
        assert!(!comb_compact_log.contains("Album:   Album B"));

        // 7. Existing log preservation (default: number _2, _3) vs overwrite option
        assert_eq!(
            crate::formatter::numbered_candidate_path(Path::new("album/report"), 2),
            PathBuf::from("album/report_2")
        );
        std::fs::write(dir_a.join("DR.txt"), "ORIGINAL LOG").unwrap();
        let (_, saved2, _, _) =
            run_scan_sync(ScanTarget::Folder(dir_a.clone()), AppConfig::default());
        assert_eq!(saved2, vec![dir_a.join("DR_2.txt")]);
        assert!(dir_a.join("DR_2.txt").is_file());
        assert_eq!(std::fs::read_to_string(dir_a.join("DR.txt")).unwrap(), "ORIGINAL LOG");

        let (_, saved3, _, _) =
            run_scan_sync(ScanTarget::Folder(dir_a.clone()), AppConfig::default());
        assert_eq!(saved3, vec![dir_a.join("DR_3.txt")]);
        assert!(dir_a.join("DR_3.txt").is_file());

        let mut overwrite_cfg = AppConfig::default();
        overwrite_cfg.overwrite_existing_logs = true;
        let (ov_log, saved_ov, _, _) =
            run_scan_sync(ScanTarget::Folder(dir_a.clone()), overwrite_cfg);
        assert_eq!(saved_ov, vec![dir_a.join("DR.txt")]);
        assert!(!dir_a.join("DR_4.txt").exists());
        let dr_contents = std::fs::read_to_string(dir_a.join("DR.txt")).unwrap();
        assert_ne!(dr_contents, "ORIGINAL LOG");
        assert!(
            dr_contents.contains("Album A")
                && ov_log.contains("Number of Files: 1")
                && ov_log.contains("Album DR: 8")
        );

        // 8. Logged track filename restriction (chars limit) and non-ASCII stripping inside logs only
        use crate::config::LogFilenameOptions;
        use crate::formatter::format_log_track_name;

        let default_name_opts = LogFilenameOptions::default();
        assert_eq!(
            format_log_track_name("02 - 日本語トラック — тест", &default_name_opts),
            "02 - 日本語トラック — тест"
        );

        let long_unicode = "01 - Track\u{2019}s Extraordinarily Long Symphony Title For Testing Sixty Four Character Truncation";
        let mut trunc_opts = LogFilenameOptions::default();
        trunc_opts.limit_length = true;
        let trunc_out = format_log_track_name(long_unicode, &trunc_opts);
        assert!(trunc_out.chars().count() <= 64);
        assert!(trunc_out.ends_with("..."));
        assert!(trunc_out.contains('\u{2019}'));

        let both_opts = LogFilenameOptions {
            limit_length: true,
            max_length: 20,
            strip_non_ascii: true,
        };
        let stripped_trunc = format_log_track_name(long_unicode, &both_opts);
        assert!(stripped_trunc.is_ascii());
        assert!(stripped_trunc.chars().count() <= 20);
        assert!(stripped_trunc.ends_with("..."));
        assert_eq!(format_log_track_name("日本語", &both_opts), "[track]");

        let mut custom_track = tracks[0].clone();
        custom_track.file_stem = "03 - Café Crème Symphony Movement Number One Extremely Long Edition Title".to_string();
        let single_mod = format_single_file_log(&custom_track, &cfg.single_rows, &both_opts);
        assert!(single_mod.contains("File Name:     03 - Caf Crme Sym..."));
        for style in FolderLogStyle::all() {
            let folder_mod = format_folder_log(
                &temp_root,
                std::slice::from_ref(&custom_track),
                *style,
                &cfg.folder_columns,
                &both_opts,
                cfg.surround_folder_mode,
            );
            assert!(folder_mod.contains("03 - Caf Crme Sym..."));
            assert!(!folder_mod.contains("Café"));
        }

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn test_baked_math_svg_assets_rasterize_cleanly() {
        use crate::math_assets::{
            SECTION_1, SECTION_2, SECTION_3, SECTION_4, SECTION_5, SECTION_6, SECTION_7,
        };
        use crate::math_svg::rasterize_svg_rgba;

        let all_sections = [
            SECTION_1, SECTION_2, SECTION_3, SECTION_4, SECTION_5, SECTION_6, SECTION_7,
        ];
        let mut total = 0usize;
        for section in all_sections {
            assert!(!section.is_empty());
            for asset in section {
                assert!(asset.width_pt > 10.0 && asset.height_pt > 5.0);
                let (w, h, rgba) =
                    rasterize_svg_rgba(asset, 2.0).expect("embedded math svgz must rasterize");
                assert!(w > 10 && h > 5);
                assert_eq!(rgba.len(), (w as usize) * (h as usize) * 4);
                let non_empty_pixels = rgba.chunks_exact(4).filter(|px| px[3] > 20).count();
                assert!(
                    non_empty_pixels > 40,
                    "expected visible rendered ink in {}, found {} pixels",
                    asset.id,
                    non_empty_pixels
                );
                total += 1;
            }
        }
        assert_eq!(total, 33);
    }

    #[test]
    fn test_commit_max_len_input() {
        use crate::app::commit_max_len_input;

        let mut cur = 64usize;
        let mut s = "32".to_string();
        assert!(commit_max_len_input(&mut s, &mut cur));
        assert_eq!((cur, s.as_str()), (32, "32"));

        s = "4".to_string();
        assert!(commit_max_len_input(&mut s, &mut cur));
        assert_eq!((cur, s.as_str()), (8, "8"));

        s = "999".to_string();
        assert!(commit_max_len_input(&mut s, &mut cur));
        assert_eq!((cur, s.as_str()), (255, "255"));

        s = "".to_string();
        assert!(!commit_max_len_input(&mut s, &mut cur));
        assert_eq!((cur, s.as_str()), (255, "255"));
    }
}

