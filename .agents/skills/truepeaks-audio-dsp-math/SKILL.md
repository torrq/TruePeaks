---
name: truepeaks-audio-dsp-math
description: >-
  Complete specification of all audio DSP, loudness, peak, dynamic range, phase
  correlation, and aggregation mathematics used in TruePeaks (`src/dsp/`,
  `src/decoder/`, `src/formatter/`). Use when verifying, debugging, extending,
  or explaining any acoustic or mathematical calculation in TruePeaks.
---

# TruePeaks Audio DSP & Acoustic Mathematics Reference

All audio analysis in `TruePeaks` runs in **64-bit IEEE 754 floating point (`f64`)**, processing decoded PCM in **8,192-frame chunks** (`CHUNK_FRAMES = 8192`). Let $C \ge 1$ be channel count, $f_s$ sample rate (Hz), and $N$ total decoded frames.

---

## 1. PCM Normalization & Effective Bit Depth (`src/decoder/`)

- **Signed integer PCM ($b \in \{16, 24, 32\}$):**
  $$x_c[n] = \frac{s_c[n]}{2^{b-1}} \in [-1.0, 1.0)$$
  Divisors: `32768.0` (16-bit), `8388608.0` (24-bit), `2147483648.0` (32-bit).
- **Unsigned 8-bit PCM:**
  $$x_c[n] = \frac{s_c[n] - 128}{128.0} \in [-1.0, 1.0)$$
- **Floating-point PCM (`f32`, `f64`):** Promoted directly to `f64` without integer rescaling (headroom above $0\text{ dBFS}$ preserved).
- **Effective Bit Depth (`bits_used`, lossless integers only):**
  Left-align every raw sample into an unsigned 32-bit MSB word (`U8 << 24`, `S16 << 16`, `S24 << 8`, `S32 << 0`) and OR across all frames & channels:
  $$M = \bigvee_{n=0}^{N-1} \bigvee_{c=0}^{C-1} \mathrm{MSB}_{32}(s_c[n]), \qquad \mathrm{BitsUsed} = \begin{cases} 0 & M = 0 \\ 32 - \mathrm{tzcnt}(M) & M \neq 0 \end{cases}$$
  Detects zero-padded containers (e.g., 16-bit audio stored inside 24-bit WAV/FLAC yields `tzcnt(M) = 16 -> 16` bits used). Lossy/float streams report `None` (`-`).

---

## 2. Sample Peak (`SPPM`) & Oversampled True Peak (`TPL`) (`src/dsp/stats.rs`, `src/dsp/loudness.rs`)

- **SPPM (Sample Peak Program Meter, dBFS):**
  $$\mathrm{SPPM}_c = 20\log_{10}\!\left(\max_{0 \le n < N} |x_c[n]|\right), \qquad \mathrm{SPPM}_{\mathrm{joint}} = 20\log_{10}\!\left(\max_{c,n} |x_c[n]|\right)$$
  Also records exact frame timestamp $t_{\mathrm{SPPM}} = n_{\max} / f_s$. Silent tracks yield $-\infty$.
- **Inter-Sample True Peak Level (`TPL`, dBTP, ITU-R BS.1770-4 Annex 2):**
  Computed via `ebur128` (`Mode::TRUE_PEAK`), using a **48-tap Hann-windowed sinc polyphase FIR interpolator**:
  $$w[j] = \frac{1}{2}\left(1 - \cos\left(\frac{2\pi j}{48}\right)\right), \qquad h[j] = w[j] \cdot \mathrm{sinc}\!\left(\frac{j - 24}{F}\right), \qquad 0 \le j < 48$$
  Adaptive oversampling factor $F$ (`tpl_oversampling_factor(f_s)`):
  - $F = 4$ when $f_s < 96{,}000\text{ Hz}$ ($12$ active taps/phase)
  - $F = 2$ when $96{,}000 \le f_s < 192{,}000\text{ Hz}$ ($24$ active taps/phase)
  - $F = 1$ when $f_s \ge 192{,}000\text{ Hz}$ (direct evaluation)
  $$\mathrm{TPL}_c = 20\log_{10}\!\left(\max_k |\tilde{x}_c[k]|\right), \qquad \mathrm{TPL}_{\mathrm{joint}} = \max_c \mathrm{TPL}_c$$

---

## 3. ITU-R BS.1770-4 K-Weighting Filter & Channel Weights (`src/dsp/loudness.rs`, `src/decoder/mod.rs`)

Two cascaded **bilinear-transformed second-order biquad IIR filters** in **Direct Form II Transposed** (with sub-`f64::MIN_POSITIVE` denormal flushing) compute K-weighted output $y_c[n]$ exact at any sample rate $f_s$:

1. **Stage 1 — Acoustic Head High-Shelf** ($f_{0,1} = 1681.974450955533\text{ Hz}$, $G_1 = 3.999843853973347\text{ dB}$, $Q_1 = 0.7071752369554196$):
   $$K_1 = \tan\!\left(\frac{\pi f_{0,1}}{f_s}\right),\; V_h = 10^{G_1 / 20},\; V_b = V_h^{0.4996667741545416},\; a_{0,1} = 1 + \frac{K_1}{Q_1} + K_1^2$$
   $$H_1(z) = \frac{\frac{V_h + V_b K_1 / Q_1 + K_1^2}{a_{0,1}} + \frac{2(K_1^2 - V_h)}{a_{0,1}}z^{-1} + \frac{V_h - V_b K_1 / Q_1 + K_1^2}{a_{0,1}}z^{-2}}{1 + \frac{2(K_1^2 - 1)}{a_{0,1}}z^{-1} + \frac{1 - K_1 / Q_1 + K_1^2}{a_{0,1}}z^{-2}}$$
2. **Stage 2 — RLB High-Pass** ($f_{0,2} = 38.13547087602444\text{ Hz}$, $Q_2 = 0.5003270373238773$):
   $$K_2 = \tan\!\left(\frac{\pi f_{0,2}}{f_s}\right),\; a_{0,2} = 1 + \frac{K_2}{Q_2} + K_2^2$$
   $$H_2(z) = \frac{1 - 2z^{-1} + z^{-2}}{1 + \frac{2(K_2^2 - 1)}{a_{0,2}}z^{-1} + \frac{1 - K_2 / Q_2 + K_2^2}{a_{0,2}}z^{-2}}$$
3. **Channel Power Weights ($G_c$):**
   - `L`, `R`, `C`, `Aux`: $1.0$ ($0\text{ dB}$)
   - `Ls`, `Rs`, `Lrs`, `Rrs`, `Lss`, `Rss`: $1.41$ ($\approx +1.5\text{ dB}$)
   - `LFE`: $0.0$ (excluded from loudness)
4. **Loudness Mapping:**
   $$L(z) = -0.691 + 10\log_{10}(z)\;\text{LUFS}$$

---

## 4. Sliding Momentary / Short-Term, Integrated LUFS & LRA (`src/dsp/loudness.rs`)

- **Exact Sample-Sliding Windows** (no $100\text{ ms}$ hop transient straddling):
  $$N_{100\mathrm{ms}} = \left\lfloor \frac{f_s + 5}{10} \right\rfloor, \qquad W_M = 4\,N_{100\mathrm{ms}}\;(400\text{ ms}), \qquad W_S = 30\,N_{100\mathrm{ms}}\;(3.0\text{ s})$$
  A circular ring buffer (`ring_sq`, length $C \cdot W_S$) updates running sums $S_{M,c}[n]$ and $S_{S,c}[n]$ every frame, recomputing directly from the ring every $W_S$ frames (`resync_sums()`) to eliminate floating-point summation drift.
- **Joint & Per-Channel Maxima (Dual-Mono Scaling):**
  $$\mathrm{MaxM}_{\mathrm{joint}} = -0.691 + 10\log_{10}\!\left(\frac{1}{W_M}\max_{n \ge W_M}\sum_c G_c S_{M,c}[n]\right)$$
  For single channels when $C > 1$, multiply by $\alpha_{\mathrm{dm}} = 2.0$ ($+3.0103\text{ dB}$) so equal L/R signals match joint LUFS (`MaxM_c`, `MaxS_c`).
- **Integrated LUFS (`LUFSi`):** EBU R128 $400\text{ ms}$ blocks ($100\text{ ms}$ step / $75\%$ overlap), absolute gate $\Gamma_a = -70.0\text{ LUFS}$, relative gate $\Gamma_r = L_{\mathrm{abs}} - 10.0\text{ LU}$.
- **Loudness Range (`LRA`):** EBU Tech 3342 over $3.0\text{ s}$ blocks gated at $-70.0\text{ LUFS}$ and $-20.0\text{ LU}$ relative:
  $$\mathrm{LRA} = P_{95}(L_S^{\mathrm{gated}}) - P_{10}(L_S^{\mathrm{gated}})\;\text{LU}$$

---

## 5. Continuous Min. PSR & Track PLR (`src/dsp/loudness.rs`, `src/dsp/stats.rs`)

- **Min. PSR (AES Peak-to-Short-Term Loudness Ratio):**
  Maintains the exact sliding $3.0\text{ s}$ maximum sample magnitude $P_{3\mathrm{s}}[n] = \max_{c,\, 0 \le i < W_S} |x_c[n-i]|$ in amortized $O(1)$ per frame via a monotonic decreasing `VecDeque`. Across all gated active frames ($n \ge W_S$ with joint Short-Term energy $> 10^{(-70.0 + 0.691)/10} \cdot W_S$ and $P_{3\mathrm{s}}[n] > 0$):
  $$r_{\mathrm{PSR}}[n] = \frac{P_{3\mathrm{s}}[n]^2}{\sum_c G_c S_{S,c}[n]}, \qquad \mathrm{Min.\;PSR} = 0.691 + 10\log_{10}\!\left(W_S \cdot \min_n r_{\mathrm{PSR}}[n]\right)\;\text{LU}$$
  Also records exact timestamp $t_{\mathrm{minPSR}}$ and decimated $\approx 0.5\text{ s}$ timeline bins (`<= 200` `TimelinePoint`s).
- **PLR (Peak-to-Integrated-Loudness Ratio):**
  $$P_{\mathrm{ref}} = \begin{cases} 0.0 & \mathrm{TPL}_{\mathrm{joint}} > 0.0 \\ \max(\mathrm{TPL}_{\mathrm{joint}}, \mathrm{SPPM}_{\mathrm{joint}}) & \text{otherwise} \end{cases}, \qquad \mathrm{PLR} = P_{\mathrm{ref}} - \mathrm{LUFS}_i\;\text{LU}$$

---

## 6. AES17 RMS & PMF Dynamic Range (`src/dsp/stats.rs`, `src/dsp/dr_pmf.rs`, `src/dsp/mod.rs`)

- **AES17 Sinusoid-Normalized RMS (dBFS):** Scaled by $2.0$ so a $0\text{ dBFS}$ sine wave reads $0.00\text{ dBFS}$:
  $$\mathrm{RMS}_{\mathrm{AES17},c} = 10\log_{10}\!\left(\frac{2}{N}\sum_{n=0}^{N-1} x_c[n]^2\right)$$
  Combined multi-channel `combined_rms_db()` averages linear power across active non-LFE channels (falling back to all channels if all are LFE):
  $$\mathrm{RMS}_{\mathrm{combined}} = 10\log_{10}\!\left(\frac{1}{|A|}\sum_{c \in A} 10^{\mathrm{RMS}_{\mathrm{AES17},c}/10}\right)$$
- **PMF Dynamic Range (`DR`):**
  Block length $N_{\mathrm{blk}}(f_s)$:
  - $3\,(f_s + 60 \cdot (f_s / 44100))$ when $44100 \mid f_s$ ($132{,}480$ at $44.1\text{ kHz}$, $\approx 3.00408\text{ s}$)
  - $3\,f_s$ otherwise ($144{,}000$ at $48\text{ kHz}$)
  For each block $b \in \{1,\dots,K\}$ of actual frame count $N_b$ (final partial block normalized by $N_b$):
  $$\sigma_{c,b}^2 = \frac{2}{N_b}\sum_{n \in b} x_c[n]^2, \qquad p_{c,b} = \max_{n \in b} |x_c[n]|$$
  Sort $\{\sigma_{c,b}^2\}$ descending, average top $M = \max(1, \lfloor 0.20\,K \rfloor)$ blocks to get $R_{20,c} = \sqrt{\frac{1}{M}\sum_{i=1}^M \sigma_{c,(i)}^2}$; pick 2nd-highest block peak $P_{2,c} = p_{c,(2)}$ (or $p_{c,(1)}$ if $K = 1$):
  $$\mathrm{DR}_c = 20\log_{10}\!\left(\frac{P_{2,c}}{R_{20,c}}\right)$$
  Joint Track DR averages unrounded $\mathrm{DR}_c$ over active non-LFE channels (or all channels if `include_lfe_in_dr = true` or LFE-only). Both `Track DR` and `Official Album DR` average **unrounded** values before `.round()`.

---

## 7. Stereo Correlation, ReplayGain 2.0 & Multi-Track Rollups (`src/dsp/stats.rs`, `src/formatter/folder_log.rs`, `src/visual_ui.rs`)

- **Stereo Phase Correlation ($C \ge 2$):** Mean-centered zero-lag Pearson $r_{LR} \in [-1, 1]$ between $x_0$ (L) and $x_1$ (R):
  $$r_{LR} = \frac{N \sum x_0 x_1 - \left(\sum x_0\right)\left(\sum x_1\right)}{\sqrt{\left(N\sum x_0^2 - \left(\sum x_0\right)^2\right)\left(N\sum x_1^2 - \left(\sum x_1\right)^2\right)}}$$
- **ReplayGain 2.0 Offset:** $\mathrm{RG}_{\mathrm{track}} = -18.0 - \mathrm{LUFS}_i\;\text{dB}$ (relative to $-18.0\text{ LUFS}$ reference).
- **Duration-Weighted Multi-Track Integrated Loudness** (`CompactMastering` header & `Visual View` KPI strip):
  $$\mathrm{LUFS}_{\mathrm{album}} = 10\log_{10}\!\left(\frac{\sum_t d_t \cdot 10^{\mathrm{LUFS}_{i,t}/10}}{\sum_t d_t}\right), \qquad \mathrm{RG}_{\mathrm{album}} = -18.0 - \mathrm{LUFS}_{\mathrm{album}}$$
- **Album Average Bitrate (`Classic DR` & `JSON` metadata):**
  $$\mathrm{Bitrate\;(\text{kbps})} = \mathrm{round}\!\left(\frac{8 \sum_{t=1}^T \mathrm{bytes}_t}{1000 \sum_{t=1}^T (N_t / f_{s,t})}\right)$$

---

## Synchronization Reminder

Whenever modifying **any** constant or equation above, follow [`truepeaks-dsp-math-sync`](../truepeaks-dsp-math-sync/SKILL.md) to keep [`README.md`](../../../README.md), `assets/math/*.svgz`, [`src/math_assets.rs`](../../../src/math_assets.rs), and `render_about_math_details` in [`src/app.rs`](../../../src/app.rs) synchronized.
