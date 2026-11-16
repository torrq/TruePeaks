# TruePeaks Agent Guidelines

## Keep DSP Math, README, and About Screen Synchronized

Whenever any DSP algorithm, mathematical formula, filter coefficient, block length, window function, channel weighting, gating threshold, rounding rule, bit-depth mask, or bitrate calculation is modified (across `src/dsp/`, `src/decoder/`, or `src/formatter/`):

1. **Update [`README.md`](README.md):** Update the corresponding equations, descriptions, and constants in the **Mathematical Specification** section, keeping Wikipedia links accurate for common formulas and DSP concepts (use GitHub-compatible LaTeX: column-0 ```` ```math ```` blocks with `\\[4pt]` instead of bare trailing `\\`, inline `` $`...`$ `` delimiters with `\lt`/`\gt`, avoid wrapping inline math in `*...*` or `[...]`, and use `\mathrm{...}` instead of `\operatorname{...}`).
2. **Regenerate Math SVGs & Update the About View:** Run `python scripts/render_math_svgs.py` to regenerate the pre-baked `assets/math/*.svgz` assets and [`src/math_assets.rs`](src/math_assets.rs) directly from [`README.md`](README.md), and update `render_about_math_details` / `render_about_overview` in [`src/app.rs`](src/app.rs) (prose summaries, Wikipedia links, and Overview line) so the built-in documentation matches both the code and `README.md`.

## Vector Icons & Windows PE Resource Embedding

- **Application Icon ([`assets/icon.svg`](assets/icon.svg) -> [`assets/icon.ico`](assets/icon.ico)):** Whenever [`assets/icon.svg`](assets/icon.svg) is modified, run `python scripts/render_icon.py` to regenerate [`assets/icon.ico`](assets/icon.ico). At compile time on Windows, [`build.rs`](build.rs) converts `assets/icon.ico` into a 32-bit Windows PE `.res` stream linked into `truepeaks.exe`, while at runtime [`src/main.rs`](src/main.rs) rasterizes `assets/icon.svg` directly via `resvg`.
- **UI Vector Icons ([`assets/back_arrow.svg`](assets/back_arrow.svg), [`assets/forward_arrow.svg`](assets/forward_arrow.svg)):** Inline directional UI icons are stored as standalone SVGs in `assets/` and rasterized at native DPI via [`MathSvgCache::ui_icon_texture`](src/math_svg.rs) rather than relying on Unicode arrow glyphs not covered by `egui`'s proportional font.

## Workspace Skills (`.agents/skills/`)

Project-specific agent workflows are packaged under [`.agents/skills/`](.agents/skills/):

- **[`truepeaks-dsp-math-sync`](.agents/skills/truepeaks-dsp-math-sync/SKILL.md):** Runbook for keeping `README.md` math blocks, `assets/math/*.svgz`, `src/math_assets.rs`, `src/app.rs`, and `assets/icon.svg` / `assets/icon.ico` synchronized.
- **[`truepeaks-release-workflow`](.agents/skills/truepeaks-release-workflow/SKILL.md):** Runbook for bumping versions in `Cargo.toml` / `Cargo.lock`, updating `DEVELOPMENT_LOG.md`, running `--locked` release builds and tests with zero warnings, and pushing `v*` tags to trigger [`.github/workflows/release.yml`](.github/workflows/release.yml).

## Project Practices

- Keep [`DEVELOPMENT_LOG.md`](DEVELOPMENT_LOG.md) up to date as changes are made (never add it to `.gitignore`).
- Keep automated tests self-contained using synthetic waveforms in temporary directories (`cargo test --release --locked`).
- Ensure `cargo build --release --locked` and `cargo test --release --locked` complete with zero warnings after code changes.
