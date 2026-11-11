# TruePeaks Agent Guidelines

## Keep DSP Math, README, and About Screen Synchronized

Whenever any DSP algorithm, mathematical formula, filter coefficient, block length, window function, channel weighting, gating threshold, rounding rule, bit-depth mask, or bitrate calculation is modified (across `src/dsp/`, `src/decoder/`, or `src/formatter/`):

1. **Update [`README.md`](README.md):** Update the corresponding equations, descriptions, and constants in the **Mathematical Specification** section, keeping Wikipedia links accurate for common formulas and DSP concepts (use GitHub-compatible LaTeX: column-0 ```` ```math ```` blocks with `\\[4pt]` instead of bare trailing `\\`, inline `` $`...`$ `` delimiters with `\lt`/`\gt`, avoid wrapping inline math in `*...*` or `[...]`, and use `\mathrm{...}` instead of `\operatorname{...}`).
2. **Regenerate Math SVGs & Update the About View:** Run `python scripts/render_math_svgs.py` to regenerate the pre-baked `assets/math/*.svgz` assets and [`src/math_assets.rs`](src/math_assets.rs) directly from [`README.md`](README.md), and update `render_about_math_details` / `render_about_overview` in [`src/app.rs`](src/app.rs) (prose summaries, Wikipedia links, and Overview line) so the built-in documentation matches both the code and `README.md`.

## Project Practices

- Keep [`DEVELOPMENT_LOG.md`](DEVELOPMENT_LOG.md) up to date as changes are made (never add it to `.gitignore`).
- Keep automated tests self-contained using synthetic waveforms in temporary directories (`cargo test --release --locked`).
- Ensure `cargo build --release --locked` and `cargo test --release --locked` complete with zero warnings after code changes.
