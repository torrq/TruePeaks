<img width="1200" height="396" alt="TruePeaks" src="https://github.com/user-attachments/assets/6fb22e6b-4f78-4bf0-a7d2-b32a102a2e0e" />

# TruePeaks

High-performance, pure-Rust desktop analyzer for **[ITU-R BS.1770-4 / EBU R128](https://en.wikipedia.org/wiki/LUFS) loudness**, **oversampled True Peak**, **continuous sample-sliding PSR**, **PLR**, **LRA**, and **PMF Dynamic Range (DR)**.

Zero runtime dependencies or FFmpeg installations required — decoding, filtering, peak detection, and UI rendering run natively across **Windows**, **Linux**, and **macOS**.

---

## Features

- **Pure-Rust Decoding (`Symphonia` + Custom `RF64`/`BW64`/`W64` Reader):**
  - Supports **WAV**, **BWF**, **AIFF** (`.aif`, `.aifc`, `.aiff`), **RF64/BW64**, **Sony Wave64** (`.w64`), **FLAC**, **ALAC** (`.alac`, `.m4a`, `.mp4`), **CAF**, **MP3**, **AAC**, and **Ogg Vorbis** (`.ogg`, `.oga`).
  - Supports lossless integer [PCM](https://en.wikipedia.org/wiki/Pulse-code_modulation) (8, 16, 24, 32-bit) as well as 32-bit and 64-bit [IEEE 754](https://en.wikipedia.org/wiki/IEEE_754) floating-point streams across mono, stereo, 5.1, 7.1, and multichannel layouts.
- **Parallel Multi-Core Processing:** Album folders and multi-folder drops decode and analyze tracks concurrently using a configurable `Rayon` thread pool with real-time progress reporting and instant cancellation.
- **Three Folder Log Styles (`Settings -> Folder Logs (DR.txt)`):**
  - **TruePeaks Table (Default):** Pipe-delimited multi-column table with 26 individually toggleable metrics and presets (`Standard`, `Minimal DR`, `Mastering`, `All Columns`), expanding per-channel columns automatically for surround files when enabled.
  - **Classic DR (`foo_dr` Style):** Reproduces the traditional `DR / Peak / RMS / Duration / Track` layout and album codec summary (`Number of tracks`, `Official DR value`, `Samplerate`, `Channels`, `Bits per sample`, `Bitrate`, `Codec`) while omitting software/date header banners.
  - **Compact Mastering:** Pure-ASCII mastering overview featuring duration-weighted average loudness, max true peak, and max sample peak above tightly aligned `DR`, `Peak dB`, `TPL dBTP`, `LUFSi`, `PSR`, `Time`, and `Track` columns.
- **Flexible Batch & Directory Workflows:**
  - Drag and drop single files, single album folders, or **multiple folders at once** (creates a separate log inside each folder by default, or merges into one combined batch log when enabled).
  - Optional recursive subfolder scanning (**off** by default).
- **Safe Output & Filename Controls (`Settings -> Output & Files`):**
  - Collision-safe atomic saving via `create_new` — existing logs are preserved by numbering new files (`DR_2.txt`, `DR_3.txt`, etc.) unless **Overwrite Existing Log File** is selected.
  - Dynamic filename templates (`{folder_name}`, `{file_name}`, `{ext}`, `{album_dr}`, `{date}`, `{time}`).
  - Optional logged track-name truncation (default suggestion: `64` characters) and non-ASCII character stripping applied strictly inside log text without renaming files on disk.
  - Export encodings: `UTF-8`, `UTF-8 with BOM`, `UTF-16 LE`, `UTF-16 BE`, and `Windows-1252`, with `CRLF`, `LF`, or `CR` line endings.
- **Native Desktop UI (`eframe` / `egui`):**
  - Automatic `System Default`, `Dark`, and `Light` themes, adjustable monospace log font size (`A-` / `A+` or `10–22 pt`), full-view split Settings screen, About screen (click the top-left `TruePeaks` badge/title), and persistent window size and position (`980 × 660` centered default).

---

## Shortcuts & Configuration

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+O` / `Cmd+O` | Select and analyze a single audio file |
| `Ctrl+Shift+O` / `Cmd+Shift+O` | Select and analyze an album folder |
| `Ctrl+,` / `Cmd+,` | Toggle Settings view |
| `Esc` | Cancel active scan or return to Analyzer view |

Settings are persisted automatically in `config.toml` under the standard OS config directory (`directories::ProjectDirs::from("org", "TruePeaks", "TruePeaks")`):
- **Windows:** `%APPDATA%\TruePeaks\TruePeaks\config\config.toml`
- **macOS:** `~/Library/Application Support/org.TruePeaks.TruePeaks/config.toml`
- **Linux:** `$XDG_CONFIG_HOME/truepeaks/config.toml` (or `~/.config/truepeaks/config.toml`)

---

## Mathematical Specification

All internal DSP runs in 64-bit [IEEE 754 floating point](https://en.wikipedia.org/wiki/IEEE_754) (`f64`), processing decoded audio in 8,192-frame chunks (`CHUNK_FRAMES = 8192`). Let $`C \ge 1`$ be the channel count, $`f_s`$ the sample rate in Hz, and $`N`$ the total frame count.

### 1. PCM Normalization & Actual Bit Depth (`bits_used`)

Decoded [pulse-code modulation (PCM)](https://en.wikipedia.org/wiki/Pulse-code_modulation) integer samples are normalized symmetrically into $`[-1, 1)`$ by power-of-2 full-scale divisors:

```math
x[n] =
\begin{cases}
\dfrac{s[n] - 128}{2^{7}} & \text{unsigned 8-bit PCM} \\[8pt]
\dfrac{s[n]}{2^{b-1}} & \text{signed } b\text{-bit integer PCM } (b \in \{16, 24, 32\})
\end{cases}
```

Floating-point streams (`f32`, `f64`) are promoted to `f64` directly without integer rescaling.

To detect zero-padded lossless [audio bit depth](https://en.wikipedia.org/wiki/Audio_bit_depth) containers (such as 16-bit audio stored inside a 24-bit WAV or FLAC file), `TruePeaks` left-aligns every lossless integer sample to the most significant bits of an unsigned 32-bit word (`U8 << 24`, `S16 << 16`, `S24 << 8`, `S32 << 0`) and accumulates a [bitwise OR](https://en.wikipedia.org/wiki/Bitwise_operation) mask across the entire track:

```math
M = \bigvee_{n=0}^{N-1} \bigvee_{c=0}^{C-1} \mathrm{MSB}_{32}\!\left(s_c[n]\right)
```

The effective active bit depth is determined by counting trailing zero bits ([find first set / trailing zero count](https://en.wikipedia.org/wiki/Find_first_set)) from the LSB:

```math
\text{Bits Used} =
\begin{cases}
0, & M = 0 \text{ (digital silence)} \\[4pt]
32 - \mathrm{tzcnt}(M), & M \neq 0
\end{cases}
```

For example, if 16-bit PCM sits inside 24-bit words with zeroed lowest 8 bits, the bottom 16 bits of $`M`$ are zero ($`\mathrm{tzcnt}(M) = 16`$), giving $`\text{Bits Used} = 16`$. Lossy codecs (`MP3`, `AAC`, `Vorbis`) and floating-point PCM report `-` (`None`).

---

### 2. Sample Peak (SPPM) & Oversampled True Peak (TPL)

#### Sample Peak Program Metering (SPPM)

Per-channel and joint SPPM measure the exact peak decoded sample magnitude in [decibels relative to full scale (dBFS)](https://en.wikipedia.org/wiki/DBFS):

```math
\text{SPPM}_c = 20 \log_{10}\!\left(\max_{0 \le n < N} |x_c[n]|\right), \qquad
\text{SPPM}_{\text{joint}} = 20 \log_{10}\!\left(\max_{0 \le c < C,\, 0 \le n < N} |x_c[n]|\right)
```

(Silent signals with peak $`0`$ yield $`-\infty\text{ dB}`$; see [Decibel](https://en.wikipedia.org/wiki/Decibel) for the field-quantity $`20\log_{10}`$ amplitude ratio.)

#### Inter-Sample True Peak Level (TPL)

Continuous-time reconstruction between discrete PCM samples (via the [Whittaker–Shannon interpolation formula](https://en.wikipedia.org/wiki/Whittaker%E2%80%93Shannon_interpolation_formula)) can overshoot $`\pm 1.0`$ even when every raw sample lies inside $`[-1, 1]`$. Per **ITU-R BS.1770-4 Annex 2** (via `ebur128` `Mode::TRUE_PEAK`), `TruePeaks` [upsamples](https://en.wikipedia.org/wiki/Upsampling) using a 48-tap [finite impulse response (FIR)](https://en.wikipedia.org/wiki/Finite_impulse_response) polyphase interpolator built from a [Hann window](https://en.wikipedia.org/wiki/Hann_function)-weighted [sinc function](https://en.wikipedia.org/wiki/Sinc_function) kernel ($`0 \le j \lt 48`$):

```math
w[j] = \frac{1}{2}\!\left(1 - \cos\!\left(\frac{2\pi j}{48}\right)\right), \qquad
h[j] = w[j] \cdot \mathrm{sinc}\!\left(\frac{j - 24}{F}\right)
```

where the [oversampling](https://en.wikipedia.org/wiki/Oversampling) factor $`F`$ adapts to sample rate (and its corresponding [Nyquist frequency](https://en.wikipedia.org/wiki/Nyquist_frequency)):
- $`F = 4`$ ($`4\times`$ oversampling, $`12`$ active taps per phase) when $`f_s \lt 96\text{ kHz}`$
- $`F = 2`$ ($`2\times`$ oversampling, $`24`$ active taps per phase) when $`96\text{ kHz} \le f_s \lt 192\text{ kHz}`$
- Direct sample evaluation ($`F = 1`$) when $`f_s \ge 192\text{ kHz}`$

True Peak levels in $`\text{dBTP}`$ are:

```math
\text{TPL}_c = 20 \log_{10}\!\left(\max_{k} |\tilde{x}_c[k]|\right), \qquad
\text{TPL}_{\text{joint}} = \max_{0 \le c < C} \text{TPL}_c
```

---

### 3. ITU-R BS.1770-4 K-Weighting Filter & Channel Weights

Perceptual loudness filtering cascades two [bilinear-transformed](https://en.wikipedia.org/wiki/Bilinear_transform) second-order [infinite impulse response (IIR)](https://en.wikipedia.org/wiki/Infinite_impulse_response) [digital biquad filters](https://en.wikipedia.org/wiki/Digital_biquad_filter) for each channel, with rational $`z`$-domain [transfer functions](https://en.wikipedia.org/wiki/Transfer_function) exact at any sample rate $`f_s`$:

#### Stage 1: Acoustic Head Pre-Filter (High Shelf)

With center frequency $`f_{0,1} = 1681.974450955533\text{ Hz}`$, shelf gain $`G = 3.999843853973347\text{ dB}`$, and quality factor $`Q_1 = 0.7071752369554196`$:

```math
K_1 = \tan\!\left(\frac{\pi f_{0,1}}{f_s}\right), \quad
V_h = 10^{G/20}, \quad
V_b = V_h^{0.4996667741545416}, \quad
a_{0,1} = 1 + \frac{K_1}{Q_1} + K_1^2
```

```math
H_1(z) = \frac{\dfrac{V_h + V_b K_1 / Q_1 + K_1^2}{a_{0,1}} + \dfrac{2(K_1^2 - V_h)}{a_{0,1}} z^{-1} + \dfrac{V_h - V_b K_1 / Q_1 + K_1^2}{a_{0,1}} z^{-2}}{1 + \dfrac{2(K_1^2 - 1)}{a_{0,1}} z^{-1} + \dfrac{1 - K_1 / Q_1 + K_1^2}{a_{0,1}} z^{-2}}
```

#### Stage 2: RLB Weighting Filter (High-Pass)

With cutoff $`f_{0,2} = 38.13547087602444\text{ Hz}`$ and $`Q_2 = 0.5003270373238773`$:

```math
K_2 = \tan\!\left(\frac{\pi f_{0,2}}{f_s}\right), \quad
a_{0,2} = 1 + \frac{K_2}{Q_2} + K_2^2
```

```math
H_2(z) = \frac{1 - 2z^{-1} + z^{-2}}{1 + \dfrac{2(K_2^2 - 1)}{a_{0,2}} z^{-1} + \dfrac{1 - K_2 / Q_2 + K_2^2}{a_{0,2}} z^{-2}}
```

Let $`y_c[n]`$ denote the output of $`H_2(H_1(x_c[n]))`$ (implemented in Direct Form II Transposed with denormal flushing).

#### BS.1770-4 Channel Power Weights ($`G_c`$)

- **Front Left (`L`), Front Right (`R`), Front Center (`C`):** $`G_c = 1.0`$ ($`0\text{ dB}`$)
- **Surround / Side / Rear channels (`Ls`, `Rs`, `Lrs`, `Rrs`, `Lss`, `Rss`):** $`G_c = 1.41`$ ($`\approx +1.5\text{ dB}`$)
- **Low-Frequency Effects (`LFE`):** $`G_c = 0.0`$ (strictly excluded from ITU loudness)

Loudness in [LUFS / LKFS](https://en.wikipedia.org/wiki/LUFS) is computed from mean-square weighted power $`z`$ using the BS.1770 offset constant $`L_0 = -0.691`$:

```math
L(z) = -0.691 + 10\log_{10}(z)
```

---

### 4. Continuous Sliding Loudness (Momentary & Short-Term), Integrated LUFS & LRA

#### Continuous Sample-Sliding Momentary (400 ms) & Short-Term (3.0 s)

Standard [EBU R 128](https://en.wikipedia.org/wiki/EBU_R_128) meters often step Momentary and Short-Term windows in coarse $`100\text{ ms}`$ hops, which can straddle and underestimate brief transients. `TruePeaks` instead maintains a $`3.0\text{ s}`$ [circular buffer](https://en.wikipedia.org/wiki/Circular_buffer) (`ring_sq`) of $`y_c[n]^2`$ and updates exact sliding sums at **every single frame $`n`$**:

```math
N_{100\text{ms}} = \left\lfloor \frac{f_s + 5}{10} \right\rfloor, \qquad
W_M = 4 N_{100\text{ms}}\;(400\text{ ms}), \qquad
W_S = 30 N_{100\text{ms}}\;(3.0\text{ s})
```

```math
S_{M,c}[n] = \sum_{i=0}^{W_M - 1} y_c[n - i]^2, \qquad
S_{S,c}[n] = \sum_{i=0}^{W_S - 1} y_c[n - i]^2
```

Every $`W_S`$ frames ($`3.0\text{ s}`$), `resync_sums()` recomputes $`S_{M,c}`$ and $`S_{S,c}`$ directly from the circular buffer to eliminate floating-point summation drift on arbitrarily long tracks. Peak linear energies are tracked in the hot loop with zero `log10` calls:

```math
\text{Max. Momentary}_{\text{joint}} = -0.691 + 10\log_{10}\!\left(\frac{1}{W_M} \max_{n \ge W_M} \sum_{c=0}^{C-1} G_c S_{M,c}[n]\right)
```

```math
\text{Max. Short-Term}_{\text{joint}} = -0.691 + 10\log_{10}\!\left(\frac{1}{W_S} \max_{n \ge W_S} \sum_{c=0}^{C-1} G_c S_{S,c}[n]\right)
```

##### Dual-Mono Single-Channel Convention

For multichannel material ($`C \gt 1`$), a full-scale sine sent identically to both $`L`$ and $`R`$ has joint power $`2\times`$ that of a single channel. Per-channel Momentary and Short-Term maxima multiply single-channel window energy by $`2.0`$ ($`+3.0103\text{ dB}`$) when $`C \gt 1`$ so equal signals produce identical per-channel and joint LUFS values:

```math
\alpha_{\text{dm}} = \begin{cases} 2.0, & C > 1 \\ 1.0, & C = 1 \end{cases}, \qquad
\text{Max. Momentary}_c = -0.691 + 10\log_{10}\!\left(\frac{\alpha_{\text{dm}}}{W_M} \max_{n \ge W_M} S_{M,c}[n]\right)
```

(and analogously over $`W_S`$ for $`\text{Max. Short-Term}_c`$).

#### Integrated Loudness ($`\text{LUFS}_i`$)

Computed per [ITU-R BS.1770](https://en.wikipedia.org/wiki/LUFS) / [EBU R 128](https://en.wikipedia.org/wiki/EBU_R_128) dual-gated $`400\text{ ms}`$ overlapping blocks ($`100\text{ ms}`$ step / $`75\%`$ overlap):

1. **Absolute Gate ($`\Gamma_a = -70.0\text{ LUFS}`$):** Retain all block energies $`z_j`$ with $`L(z_j) \gt -70.0\text{ LUFS}`$ and compute intermediate loudness and relative gate:

```math
\Gamma_r = -0.691 + 10\log_{10}\!\left(\frac{1}{|J_a|}\sum_{j \in J_a} z_j\right) - 10.0\text{ LU}
```

2. **Relative Gate ($`\Gamma_r`$):** Retain blocks $`J_r = \{j \in J_a : L(z_j) \gt \Gamma_r\}`$:

```math
\text{LUFS}_i = -0.691 + 10\log_{10}\!\left(\frac{1}{|J_r|}\sum_{j \in J_r} z_j\right)
```

#### Loudness Range (LRA)

Over $`3.0\text{ s}`$ Short-Term block energies gated by an absolute threshold of $`-70.0\text{ LUFS}`$ and a relative threshold of $`-20.0\text{ LU}`$ below gated Short-Term loudness, [EBU R 128](https://en.wikipedia.org/wiki/EBU_R_128) (Tech 3342) defines LRA as the spread between the **95th** and **10th** [percentiles](https://en.wikipedia.org/wiki/Percentile):

```math
\text{LRA} = P_{95}\!\left(L_S^{\text{gated}}\right) - P_{10}\!\left(L_S^{\text{gated}}\right)\;\text{LU}
```

---

### 5. Continuous Minimum PSR & PLR

#### Continuous Min. PSR (AES Peak-to-Short-Term Loudness Ratio)

Related to the classic [crest factor](https://en.wikipedia.org/wiki/Crest_factor), PSR compares peak sample amplitude against short-term perceived loudness across sliding $`3.0\text{ s}`$ windows ($`W_S`$ frames). Rather than sampling coarse hops, `TruePeaks` tracks the exact sliding window sample maximum at **every sample frame** via an $`O(1)`$-amortized monotonic decreasing [double-ended queue (`VecDeque`)](https://en.wikipedia.org/wiki/Double-ended_queue):

```math
P_{3\text{s}}[n] = \max_{\substack{0 \le c < C \\ 0 \le i < W_S}} |x_c[n - i]|
```

Over all active windows above the $`-70.0\text{ LUFS}`$ Short-Term gate ($`z_{S,\text{joint}}[n] \gt 10^{(-70.0 + 0.691)/10}`$ and $`P_{3\text{s}}[n] \gt 0`$):

```math
\text{PSR}[n] = 20\log_{10}\!\left(P_{3\text{s}}[n]\right) - \left(-0.691 + 10\log_{10}\!\left(\frac{\sum_c G_c S_{S,c}[n]}{W_S}\right)\right)
```

Rearranging into a monotonic linear ratio avoids computing `log10` per frame:

```math
r_{\text{PSR}}[n] = \frac{P_{3\text{s}}[n]^2}{\sum_{c=0}^{C-1} G_c S_{S,c}[n]}, \qquad
\text{Min. PSR} = 0.691 + 10\log_{10}\!\left(W_S \cdot \min_{n \ge W_S,\, L_S[n] > -70} r_{\text{PSR}}[n]\right)\;\text{LU}
```

#### PLR (Peak-to-Integrated-Loudness Ratio)

Evaluates whole-track [crest factor](https://en.wikipedia.org/wiki/Crest_factor)-style macro-headroom against Integrated Loudness, capping reference peaks above $`0\text{ dBTP}`$ at $`0.0\text{ dBTP}`$:

```math
P_{\text{ref}} =
\begin{cases}
0.0, & \text{TPL}_{\text{joint}} > 0.0 \\[4pt]
\max\!\left(\text{TPL}_{\text{joint}},\, \text{SPPM}_{\text{joint}}\right), & \text{otherwise}
\end{cases}, \qquad
\text{PLR} = P_{\text{ref}} - \text{LUFS}_i\;\text{LU}
```

---

### 6. AES17 Sinusoid-Normalized RMS & PMF Dynamic Range (DR)

#### AES17 RMS

Unweighted [root mean square (RMS)](https://en.wikipedia.org/wiki/Root_mean_square) of a full-scale sine wave $`x(t) = \sin(\omega t)`$ equals $`1/\sqrt{2} \approx -3.0103\text{ dBFS}`$. Following **AES17** / **PMF** convention, squared amplitude is scaled by $`2.0`$ so a $`0\text{ dBFS}`$ sine reads **$`0.00\text{ dBFS}`$**:

```math
\text{RMS}_{\text{AES17}, c} = 10\log_{10}\!\left(\frac{2}{N} \sum_{n=0}^{N-1} x_c[n]^2\right)\;\text{dBFS}
```

When combining channels for `Classic DR (foo_dr Style)` album summaries, active non-LFE channels $`A`$ are [arithmetic mean](https://en.wikipedia.org/wiki/Arithmetic_mean)-averaged in linear power:

```math
\text{RMS}_{\text{combined}} = 10\log_{10}\!\left(\frac{1}{|A|}\sum_{c \in A} 10^{\text{RMS}_{\text{AES17}, c}/10}\right)
```

#### PMF Dynamic Range (`dr_pmf.rs`)

Motivated by measuring musical [dynamic range](https://en.wikipedia.org/wiki/Dynamic_range) (in contrast to peak-normalized hyper-compression in the [loudness war](https://en.wikipedia.org/wiki/Loudness_war)), the Pleasurize Music Foundation (PMF) algorithm partitions each channel into contiguous blocks of length $`N_{\text{blk}}(f_s)`$ frames (plus any final partial block normalized by its actual frame count $`N_b`$):

```math
N_{\text{blk}}(f_s) =
\begin{cases}
3\left(f_s + 60 \cdot \dfrac{f_s}{44100}\right), & 44100 \mid f_s \quad (\text{e.g. } 132{,}480 \text{ at } 44.1\text{ kHz}, \approx 3.00408\text{ s}) \\[10pt]
3\,f_s, & \text{otherwise} \quad (\text{e.g. } 144{,}000 \text{ at } 48\text{ kHz})
\end{cases}
```

For each block $`b \in \{1, \dots, K\}`$ of length $`N_b`$ frames in channel $`c`$:

```math
\sigma_{c,b}^2 = \frac{2}{N_b}\sum_{n \in \text{block } b} x_c[n]^2, \qquad
p_{c,b} = \max_{n \in \text{block } b} |x_c[n]|
```

1. **Top 20% Loudest Block [RMS](https://en.wikipedia.org/wiki/Root_mean_square) ($`R_{20, c}`$):** Sort $`\{\sigma_{c,b}^2\}`$ descending as $`\sigma_{c,(1)}^2 \ge \sigma_{c,(2)}^2 \ge \dots \ge \sigma_{c,(K)}^2`$, select $`M = \max(1, \lfloor 0.20\,K \rfloor)`$:

```math
R_{20, c} = \sqrt{\frac{1}{M}\sum_{i=1}^{M} \sigma_{c,(i)}^2}
```

2. **Second-Highest Block Peak ($`P_{2, c}`$):** Sort $`\{p_{c,b}\}`$ descending as $`p_{c,(1)} \ge p_{c,(2)} \ge \dots`$, discarding the single loudest block peak so isolated transient spikes do not inflate the rating:

```math
P_{2,c} =
\begin{cases}
p_{c,(2)}, & K \ge 2 \\[4pt]
p_{c,(1)}, & K = 1
\end{cases}
```

3. **Channel DR ([crest factor](https://en.wikipedia.org/wiki/Crest_factor) of top-20% RMS vs 2nd block peak in dB):**

```math
\text{DR}_c = 20\log_{10}\!\left(\frac{P_{2,c}}{R_{20,c}}\right)
```

4. **Joint Track DR & Official Album DR:** Let $`A`$ be active channels (excluding `LFE` by default, or all channels if `include_lfe_in_dr` is enabled or all channels are LFE). Averaging uses unrounded floating-point $`\text{DR}_c`$ and $`\text{DR}_{\text{track, raw}}^{(t)}`$ across all $`T`$ tracks before rounding to nearest integer (`round()`):

```math
\text{DR}_{\text{track, raw}} = \frac{1}{|A|}\sum_{c \in A} \text{DR}_c, \qquad
\text{DR}_{\text{track}} = \mathrm{round}\!\left(\text{DR}_{\text{track, raw}}\right)
```

```math
\text{Official Album DR} = \mathrm{round}\!\left(\frac{1}{T}\sum_{t=1}^{T} \text{DR}_{\text{track, raw}}^{(t)}\right)
```

(Because whole-track $`\text{RMS}_{\text{AES17}, c} = 10\log_{10}\!\left(\frac{1}{K}\sum_{b=1}^K \sigma_{c,b}^2\right)`$ for uniform blocks, `dr_pmf` also computes per-channel AES17 RMS over block energies.)

---

### 7. Album Average Bitrate (`Classic DR` Footer)

When generating the `Classic DR (foo_dr Style)` album metadata block, aggregate bitrate across $`T`$ analyzed files is computed from total on-disk file sizes and total decoded audio duration:

```math
\text{Bitrate (kbps)} = \mathrm{round}\!\left(\frac{8 \sum_{t=1}^{T} \text{bytes}_t}{1000 \sum_{t=1}^{T} (N_t / f_{s,t})}\right)
```

---

## Building from Source & Testing

### Prerequisites
- Rust toolchain (`1.75+` / stable, with Cargo)
- **Linux additional system libraries** (for `eframe` / `winit` / `rfd`):
  ```bash
  sudo apt-get install -y \
    libxkbcommon-dev libwayland-dev libgl1-mesa-dev \
    libx11-dev libxcursor-dev libxrandr-dev libxi-dev
  ```

### Build & Run Tests

The test suite generates synthetic audio test probes directly in temporary directories (no external commercial tracks required):

```bash
cargo build --release --locked
cargo test --release --locked
```

Precompiled release archives for **Windows (`x86_64`)**, **Linux (`x86_64`, GLIBC $`\ge 2.28`$)**, and **macOS (`Universal arm64 + x86_64` `.app`)** are built via GitHub Actions on every push and published automatically with `SHA256SUMS.txt` on `v*` tags. See [GitHub Releases](https://github.com/torrq/TruePeaks/releases).

---

## License

[MIT License](LICENSE) — Copyright © 2026 `torrq`
