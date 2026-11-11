# TruePeaks Agent Guidelines

## Keep DSP Math, README, and About Screen Synchronized

Whenever any DSP algorithm, mathematical formula, filter coefficient, block length, window function, channel weighting, gating threshold, rounding rule, bit-depth mask, or bitrate calculation is modified (across `src/dsp/`, `src/decoder/`, or `src/formatter/`):

1. **Update [`README.md`](README.md):** Update the corresponding equations, descriptions, and constants in the **Mathematical Specification** section, keeping Wikipedia links accurate for common formulas and DSP concepts (use GitHub-compatible LaTeX such as `\mathrm{...}` — avoid blocked macros like `\operatorname`).
2. **Update the About View in [`src/app.rs`](src/app.rs):** Update `render_about_math_details` (the **DSP & Math Details** tab of the About screen, plus the **Overview** summary row in `render_about_overview` if applicable) so the built-in documentation matches both the code and `README.md`.

## Project Practices

- Keep [`DEVELOPMENT_LOG.md`](DEVELOPMENT_LOG.md) up to date as changes are made (never add it to `.gitignore`).
- Keep automated tests self-contained using synthetic waveforms in temporary directories (`cargo test --release --locked`).
- Ensure `cargo build --release --locked` and `cargo test --release --locked` complete with zero warnings after code changes.
