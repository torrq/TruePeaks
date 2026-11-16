---
name: truepeaks-dsp-math-sync
description: >-
  Use this skill whenever any DSP algorithm, mathematical formula, filter coefficient,
  block length, window function, channel weighting, gating threshold, rounding rule,
  bit-depth mask, bitrate calculation, or SVG application icon in TruePeaks is modified.
---

# TruePeaks DSP Math & SVG Asset Synchronization

Keep the DSP implementation (`src/dsp/`, `src/decoder/`, `src/formatter/`), [`README.md`](../../../README.md), pre-baked MathJax `.svgz` assets (`assets/math/*.svgz` & [`src/math_assets.rs`](../../../src/math_assets.rs)), the in-app **About -> DSP & Math Details** screen ([`src/app.rs`](../../../src/app.rs)), and vector icon assets (`assets/*.svg`) synchronized.

## 1. Updating Mathematical Formulas (`README.md` -> `.svgz` -> `src/app.rs`)

1. **Edit [`README.md`](../../../README.md) (`Mathematical Specification`, Sections `1..8`):**
   - Use unindented column-0 ```` ```math ```` blocks for display equations (use `\\[4pt]` instead of bare trailing `\\` before newlines).
   - Use `` $`...`$ `` delimiters for inline math, with `\lt` and `\gt` instead of `<` and `>`, `\mathrm{...}` instead of `\operatorname{...}`, and never wrap inline math inside `*...*` or `[...]` link text.
2. **Regenerate Pre-Baked MathJax `.svgz` Assets:**
   - Run the generator script from the repository root:
     ```powershell
     python scripts/render_math_svgs.py
     ```
   - This extracts all ` ```math ` blocks from `README.md` Sections `1..8`, renders them via headless Chrome + MathJax v3 at `18.0 pt/em`, writes deterministic `assets/math/s<sec>_<idx>.svgz` files, and regenerates [`src/math_assets.rs`](../../../src/math_assets.rs) (`SECTION_1` .. `SECTION_8`).
   - Also keep the audio mathematics reference in [`truepeaks-audio-dsp-math`](../truepeaks-audio-dsp-math/SKILL.md) updated to match any modified formulas or constants.
3. **Synchronize [`src/app.rs`](../../../src/app.rs):**
   - Update prose descriptions and Wikipedia reference links in `render_about_math_details` and `render_about_overview` in [`src/app.rs`](../../../src/app.rs) if any section titles, summaries, or references changed.
4. **Verify Rasterization:**
   - Run:
     ```powershell
     cargo test --release --locked test_baked_math_svg_assets_rasterize_cleanly
     ```

## 2. Updating the Application Icon (`assets/icon.svg` -> `assets/icon.ico` & `assets/icon.icns`)

1. Edit [`assets/icon.svg`](../../../assets/icon.svg) (`512x512` `viewBox`, using `<rect>` and `<path>` elements).
2. Regenerate the multi-resolution Windows `.ico` (`16, 24, 32, 48, 64` 32-bit BGRA DIB frames + `256` PNG frame) and macOS `.icns`:
   ```powershell
   python scripts/render_icon.py
   ```
3. Verify both runtime `resvg` rasterization and Windows PE `.res` embedding via [`build.rs`](../../../build.rs):
   ```powershell
   cargo test --release --locked test_app_icon_svg_rasterizes_cleanly
   cargo build --release --locked --bin truepeaks
   ```
