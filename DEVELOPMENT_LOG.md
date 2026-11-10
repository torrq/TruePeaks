# TruePeaks Development Progress Log

## 2026-11-10 — Project Initialization & Specification Analysis

### 1. Repository & Ignore Configuration
- Created [`.gitignore`](file:///c:/Users/nathan/Projects/git/TruePeaks/.gitignore) excluding `/target`, `scratch/`, and `truepeaks_audio_analyzer_specification_plan.md`.
- Created [`DEVELOPMENT_LOG.md`](file:///c:/Users/nathan/Projects/git/TruePeaks/DEVELOPMENT_LOG.md) (tracked in git, **not** added to `.gitignore`) to record ongoing implementation progress, architectural decisions, and verification results.

### 2. Specification & Golden Reference Analysis
- Reviewed `truepeaks_audio_analyzer_specification_plan.md` covering the 100% pure-Rust tech stack (`eframe`/`egui`, `symphonia`, `ebur128`, `rayon`, `crossbeam-channel`, `rfd`, `unicode-normalization`, `unicode-width`, `encoding_rs`, `serde`, `toml`, `directories`, `chrono`).
- Inspected the 24 golden reference single-track logs and 2 folder logs in `scratch/` (*Red Hot Chili Peppers - Stadium Arcadium (2006)* and *Electric Light Orchestra - Eldorado (1974)*):
  - Verified exact byte-level layout for Single-File Logs:
    - Leading empty line (`\r\n`), header metadata (`File Name`, `Absolute Path`, `File Format`, and `Channel Layout` when $>2$ channels), 100-char sample-rate/word-length/bits-used block, 100-char channel header line, and 106-char metric rows (`49`-char left-aligned label + `19`-char columns formatted as `format!("{:>6}{:<13}", value, suffix)`).
  - Verified exact byte-level layout for Folder Logs:
    - Leading empty line (`\r\n`), `Folder Path:   <path>`, dynamic column widths (`max(visual_width(header), max_row_visual_width)`) with Unicode NFC normalization and visual width padding (`unicode-width`), right-aligned cells separated by `" | "`, and summary footer (`Number of EP/Album Files` and `Official EP/Album DR` using the unrounded track DR mean).

---

## 2026-11-10 — Core Architecture & Module Implementation

### 3. Configuration & Persistence ([`src/config.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/config.rs))
- Implemented `AppConfig` serialized via `serde` + `toml` to the OS user config directory (`directories::ProjectDirs`).
- Implemented `FolderColumnsConfig` with all 26 toggleable columns organized into 7 UI groups (`Info`, `DR`, `True Peak Level`, `LUFS`, `Channel Specific LUFS`, `Old Skool`, `Ratios`), matching the default 9-column stereo folder layout.
- Implemented `SingleFileRowsConfig` with all 10 metric rows enabled by default.
- Implemented `SurroundFolderMode` (`ExpandPerChannel` vs `StrictTwoChannel`), `CharsetMode` (`UTF-8`, `UTF-8 BOM`, `UTF-16 LE`, `UTF-16 BE`, `Windows-1252`), `LineEndingMode` (`CRLF`, `LF`, `CR`), thread pool sizing, and LFE DR inclusion toggle.

### 4. Multi-Format Streaming Audio Decoder ([`src/decoder/`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/decoder/mod.rs))
- Implemented [`src/decoder/mod.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/decoder/mod.rs):
  - Defined `SpeakerChannel` (`FrontLeft`, `FrontRight`, `FrontCenter`, `Lfe`, `LeftSurround`, `RightSurround`, `LeftRearSurround`, `RightRearSurround`, `LeftSideSurround`, `RightSideSurround`, `Aux(usize)`) with SMPTE/ITU standard layout ordering (`Mono`, `Stereo`, `2.1`, `3.0`, `4.0`, `5.0`, `5.1`, `6.1`, `7.1`) and `WAVEFORMATEXTENSIBLE` (`dwChannelMask`) parsing.
  - Defined `ChunkedDecoder` trait streaming `CHUNK_FRAMES = 8192` frames per call to bound per-thread memory and enable sub-file `AtomicBool` cooperative cancellation.
- Implemented [`src/decoder/symphonia_dec.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/decoder/symphonia_dec.rs):
  - Full support for `.wav`, `.bwf` (standard RIFF), `.aiff`/`.aif`/`.aifc`, `.flac`, `.m4a`/`.alac`, `.mp3`, `.aac`, `.ogg`/`.oga`.
  - Pre-float MSB-aligned 32-bit `bit_mask` accumulation across `U8`, `S16`, `S24`, and `S32` sample buffers for exact `Bits Used` (`32 - bit_mask.trailing_zeros()`) detection.
- Implemented [`src/decoder/w64_rf64_dec.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/decoder/w64_rf64_dec.rs):
  - Pure-Rust streaming demuxer and PCM decoder (`Int` 8/16/24/32-bit and `Float` 32/64-bit) for Sony Wave64 (`.w64` 128-bit GUID chunks with 8-byte alignment) and 64-bit Broadcast Wave (`RF64` / `BW64` with `ds64` 64-bit size table).

### 5. Single-Pass DSP Engine ([`src/dsp/`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/dsp/mod.rs))
- Implemented [`src/dsp/loudness.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/dsp/loudness.rs):
  - Joint `ebur128` instance configured with `Mode::I | Mode::S | Mode::M | Mode::LRA | Mode::TRUE_PEAK` and ITU-R BS.1770-4 channel map (`Left`, `Right`, `Center`, `Unused` for LFE, `LeftSurround`, `RightSurround`).
  - Per-channel `ebur128` instances in Dual-Mono (`[Channel::Left, Channel::Right]`) configuration so each discrete channel (including LFE) yields its exact `(non-ITU)` Momentary and Short-Term maxima.
  - Sliding 3-second (`3 * sample_rate`) ring-buffer peak tracker paired with 10 Hz (`sample_rate / 10`) Short-Term loudness evaluation to compute `Min. PSR (SPPM-to-Short-Term Loudness Ratio)`.
- Implemented [`src/dsp/dr_pmf.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/dsp/dr_pmf.rs) & [`src/dsp/stats.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/dsp/stats.rs):
  - Discovered and verified the exact Pleasurize Music Foundation (PMF) block size at $44.1\text{ kHz}$ multiples: `pmf_block_frames(sr) = 3 * (sr + 60 * (sr / 44100))` (`132,480` frames at $44.1\text{ kHz}$, `144,000` frames at $48\text{ kHz}$).
  - Verified that whole-track AES17 `RMS` is the quadratic mean across all $K$ PMF blocks (`2.0 * sum_sq / actual_frames`), achieving **48 out of 48 exact matches (`0.00 dB` error)** across all 24 reference tracks in `scratch/`.
  - Verified top-20% block RMS (`M = floor(0.20 * K)`) with nominal 3-second denominator scaling (`block_frames / (3 * sample_rate)`) and 2nd-highest block peak (`P(2)`), achieving **24 out of 24 exact matches** on `DR (PMF)` track integers and both `Official EP/Album DR` values (`5` and `11`).

### 6. Log Formatting & Desktop GUI ([`src/formatter/`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/formatter/mod.rs), [`src/scanner.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/scanner.rs), [`src/app.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/app.rs))
- Implemented [`src/formatter/mod.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/formatter/mod.rs), [`src/formatter/single_log.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/formatter/single_log.rs), and [`src/formatter/folder_log.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/formatter/folder_log.rs) with Unicode NFC normalization, `unicode-width` visual column alignment, configurable line endings (`CRLF`/`LF`/`CR`), and `encoding_rs` character transcoding.
- Implemented [`src/scanner.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/scanner.rs) with a bounded Rayon `ThreadPool` and `crossbeam-channel` progress streaming.
- Implemented [`src/app.rs`](file:///c:/Users/nathan/Projects/git/TruePeaks/src/app.rs) with dark-mode `egui` styling, drag-and-drop file/folder scanning, progress bar, monospace log preview, and the full Settings modal with 7 pill-button groups (`Rounding::same(14.0)`) and 10 single-file row checkboxes.

---

## 2026-11-10 — Verification & Test Suite Results

### 7. Automated Verification Suite (`cargo test --release`)
- **`test_golden_stereo_reference_parity_and_unicode_alignment`**: **PASSED**
  - Analyzed all 14 tracks of *Red Hot Chili Peppers - Stadium Arcadium (2006)* and all 10 tracks of *Electric Light Orchestra - Eldorado (1974)*.
  - Verified exact line counts, 100-char header widths, 106-char metric row widths, `Sample Rate`/`Word Length`/`Bits Used` values, `Max. SPPM`, `RMS` (48/48 channels exact match), `DR (PMF)` Joint column (24/24 tracks exact match), and `Official EP/Album DR` (`5` for *Stadium Arcadium*, `11` for *Eldorado*).
  - Verified identical visual pipe `|` column offsets across all rows, including tracks with multi-byte UTF-8 characters (`’` `U+2019` in `06 - She’s Only 18`, `12 - C’mon Girl`, `02 - Can’t Get It Out of My Head`, `07 - Nobody’s Child`).
- **`test_formats_bits_used_and_high_sample_rates`**: **PASSED**
  - Verified $384\text{ kHz}$ 24-bit WAV with 16 active MSB bits (`Sample Rate: 384k`, `Word Length: 24`, `Bits Used: 16`).
  - Verified $96\text{ kHz}$ 24-bit 64-bit `RF64`/`ds64` `.bwf` with full 24-bit depth (`Word Length: 24`, `Bits Used: 24`).
  - Verified $48\text{ kHz}$ 16-bit Sony Wave64 (`.w64`) decoding and bitmask inspection.
- **`test_surround_5_1_and_7_1_channel_weighting_and_lfe_exclusion`**: **PASSED**
  - Verified 6-channel ($5.1$: `L, R, C, LFE, Ls, Rs`) `WAVEFORMATEXTENSIBLE` channel mask parsing, LFE measurement in per-channel `Max. SPPM`/`RMS`/`DR`, LFE exclusion/inclusion toggle in joint `DR (PMF)`, dynamic 182-char 6-channel Single-File Log formatting, and expanded per-channel Surround Folder Log formatting (`TPL L`, `TPL LFE`, `DR LFE`).
