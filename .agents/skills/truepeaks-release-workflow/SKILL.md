---
name: truepeaks-release-workflow
description: >-
  Use this skill when bumping the TruePeaks version, verifying release builds and tests,
  updating DEVELOPMENT_LOG.md, and pushing a v* tag to GitHub to publish a release.
---

# TruePeaks Release Workflow

Follow this checklist whenever preparing and publishing a new `TruePeaks` release (`vX.Y.Z`).

## Steps

1. **Bump Version in [`Cargo.toml`](../../../Cargo.toml):**
   - Update `version = "X.Y.Z"` under `[package]` in [`Cargo.toml`](../../../Cargo.toml).
2. **Update [`DEVELOPMENT_LOG.md`](../../../DEVELOPMENT_LOG.md) & [`README.md`](../../../README.md):**
   - Record all architectural changes, UI refinements, and test results in [`DEVELOPMENT_LOG.md`](../../../DEVELOPMENT_LOG.md).
   - Ensure [`README.md`](../../../README.md) reflects any new user-facing features or settings. If any ` ```math ` blocks in `README.md` changed, run `python scripts/render_math_svgs.py`.
3. **Build & Run the Self-Contained Release Test Suite:**
   - Run both commands (note `Cargo.toml` version bumps update `Cargo.lock` on the first build, after which `--locked` must pass cleanly with zero warnings):
     ```powershell
     cargo build --release
     cargo build --release --locked
     cargo test --release --locked
     ```
   - Confirm **0 compiler warnings** and **0 failed tests**.
4. **Stage, Commit, Tag & Push:**
   - Check `git status --short` and stage only tracked/intended project files (never commit untracked scratch artifacts or local PDFs).
   - Commit on `main`, create annotated or lightweight tag `vX.Y.Z`, and push both `main` and `vX.Y.Z` to `origin`:
     ```powershell
     git commit -m "Release vX.Y.Z: <summary>"
     git tag vX.Y.Z
     git push origin main vX.Y.Z
     ```
   - Pushing `vX.Y.Z` triggers [`.github/workflows/release.yml`](../../../.github/workflows/release.yml) to build Windows (`x86_64`), Linux (`x86_64` `glibc 2.28+`), and macOS Universal (`arm64 + x86_64` `.app`) archives with `SHA256SUMS.txt` and publish the GitHub Release.
