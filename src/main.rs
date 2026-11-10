#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod app;
pub mod config;
pub mod decoder;
pub mod dsp;
pub mod formatter;
pub mod scanner;

use app::TruePeaksApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("TruePeaks — Audio Loudness & Dynamic Range Analyzer")
            .with_inner_size([980.0, 660.0])
            .with_min_inner_size([640.0, 420.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "TruePeaks",
        native_options,
        Box::new(|cc| Ok(Box::new(TruePeaksApp::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use crate::config::{AppConfig, SurroundFolderMode};
    use crate::decoder::SpeakerChannel;
    use crate::dsp::analyze_file;
    use crate::formatter::folder_log::format_folder_log;
    use crate::formatter::single_log::format_single_file_log;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn test_golden_stereo_reference_parity_and_unicode_alignment() {
        let cancel = AtomicBool::new(false);
        let cfg = AppConfig::default();

        let albums = [
            (
                r"scratch\Red Hot Chili Peppers - Stadium Arcadium (2006)",
                r"scratch\Red Hot Chili Peppers - Stadium Arcadium (2006)\Red Hot Chili Peppers - Stadium Arcadium (2006)_log.txt",
                5,
            ),
            (
                r"scratch\Electric Light Orchestra - Eldorado (1974)",
                r"scratch\Electric Light Orchestra - Eldorado (1974)\Electric Light Orchestra - Eldorado (1974)_log.txt",
                11,
            ),
        ];

        for (dir_str, ref_folder_log_str, expected_album_dr) in albums {
            let dir = Path::new(dir_str);
            if !dir.exists() {
                continue;
            }
            let mut wav_files: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .filter_map(|e| e.ok().map(|d| d.path()))
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("wav"))
                .collect();
            wav_files.sort();

            let mut tracks = Vec::with_capacity(wav_files.len());
            for wav_p in &wav_files {
                let track = analyze_file(wav_p, false, &cancel).expect("Failed to analyze WAV");
                let stem = wav_p.file_stem().unwrap().to_str().unwrap();
                let ref_single_path = wav_p.with_file_name(format!("{}_log.txt", stem));
                let ref_single = std::fs::read_to_string(&ref_single_path).unwrap();

                let gen_single = format_single_file_log(&track, &cfg.single_rows);

                // Verify line-by-line structural width & exact RMS / SPPM / Bits Used / DR (PMF)
                let ref_lines: Vec<&str> = ref_single.lines().collect();
                let gen_lines: Vec<&str> = gen_single.lines().collect();
                assert_eq!(ref_lines.len(), gen_lines.len(), "Line count mismatch on {}", stem);

                // Header lines 5, 6, 8 (0-indexed) are 100 chars; metric lines 9..19 are 106 chars
                assert_eq!(gen_lines[5].len(), 100, "Sample Rate header width on {}", stem);
                assert_eq!(gen_lines[6].len(), 100, "Sample Rate values width on {}", stem);
                assert_eq!(gen_lines[8].len(), 100, "Left/Right/Joint header width on {}", stem);
                for row_idx in 9..19 {
                    assert_eq!(
                        gen_lines[row_idx].len(),
                        106,
                        "Metric row {} width mismatch on {}",
                        row_idx,
                        stem
                    );
                }

                // Exact match on Sample Rate / Word Length / Bits Used values line (6)
                assert_eq!(gen_lines[6], ref_lines[6], "Metadata values mismatch on {}", stem);
                // Exact match on Max. SPPM line (10) (treating -0.00 dBFS and 0.00 dBFS as equivalent 0.00 dBFS)
                assert_eq!(
                    gen_lines[10].replace("-0.00 dBFS", " 0.00 dBFS"),
                    ref_lines[10].replace("-0.00 dBFS", " 0.00 dBFS"),
                    "Max. SPPM mismatch on {}",
                    stem
                );
                // Exact match on RMS line (11) across all 24 tracks (48 channels)
                assert_eq!(gen_lines[11], ref_lines[11], "AES17 RMS mismatch on {}", stem);
                // Exact match on DR (PMF) Joint column (line 15) across all 24 tracks
                assert_eq!(
                    &gen_lines[15][87..],
                    &ref_lines[15][87..],
                    "Joint DR (PMF) mismatch on {}",
                    stem
                );

                tracks.push(track);
            }

            // Compute Official EP/Album DR from unrounded track DR mean
            let mean_dr: f64 =
                tracks.iter().map(|t| t.dr_track_raw).sum::<f64>() / (tracks.len() as f64);
            assert_eq!(
                mean_dr.round() as i32,
                expected_album_dr,
                "Official EP/Album DR mismatch for {}",
                dir_str
            );

            // Generate Folder Log and verify visual column alignment (`|` pipe positions)
            let gen_folder = format_folder_log(
                dir,
                &tracks,
                &cfg.folder_columns,
                cfg.surround_folder_mode,
            );
            let ref_folder = std::fs::read_to_string(ref_folder_log_str).unwrap();
            let gen_flines: Vec<&str> = gen_folder.lines().collect();
            let ref_flines: Vec<&str> = ref_folder.lines().collect();
            assert_eq!(gen_flines.len(), ref_flines.len());

            // Check that Official EP/Album DR line matches exactly
            assert_eq!(
                gen_flines.last().unwrap(),
                ref_flines.last().unwrap(),
                "Official EP/Album DR footer line mismatch in {}",
                dir_str
            );

            // Verify that every table row (header + all track rows) has identical visual pipe column offsets,
            // including tracks with multi-byte Unicode right-single-quotation marks (U+2019 ’)
            let header_line = gen_flines[3];
            let expected_pipes: Vec<usize> = header_line
                .split('|')
                .scan(0usize, |acc, seg| {
                    *acc += UnicodeWidthStr::width(seg) + 1;
                    Some(*acc)
                })
                .collect();

            for row_line in &gen_flines[5..5 + tracks.len()] {
                let row_pipes: Vec<usize> = row_line
                    .split('|')
                    .scan(0usize, |acc, seg| {
                        *acc += UnicodeWidthStr::width(seg) + 1;
                        Some(*acc)
                    })
                    .collect();
                assert_eq!(
                    row_pipes, expected_pipes,
                    "Visual pipe alignment mismatch on row: {}",
                    row_line
                );
            }
        }
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
        // Note: For >2 channels, Line 4 is "Channel Layout: ...", so metric rows start at Line 10
        let mut cfg = AppConfig::default();
        let single_log = format_single_file_log(&track_no_lfe_dr, &cfg.single_rows);
        let s_lines: Vec<&str> = single_log.lines().collect();
        assert_eq!(s_lines[10].len(), 49 + 7 * 19);

        // Verify Surround Folder Log expanded mode works cleanly
        cfg.surround_folder_mode = SurroundFolderMode::ExpandPerChannel;
        cfg.folder_columns.max_tpl_left = true;
        cfg.folder_columns.dr_left = true;
        let folder_log = format_folder_log(
            &temp_dir,
            &[track_no_lfe_dr],
            &cfg.folder_columns,
            cfg.surround_folder_mode,
        );
        assert!(folder_log.contains("TPL L"));
        assert!(folder_log.contains("TPL LFE"));
        assert!(folder_log.contains("DR LFE"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}

