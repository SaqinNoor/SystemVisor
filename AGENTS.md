# SystemVisor — Agent guide

## Quick start

```powershell
cargo build --release          # produces target/release/systemvisor.exe
cargo run --release            # run the TUI
```

Binary: `src/main.rs` (mods: `telemetry`, `audio_visualizer`, `media_controls`, `art_processor`). Auxiliary stub: `src/bin/check_cpal.rs`.

## Platform & toolchain

- **Windows-only.** Uses WASAPI (cpal), WinRT GSMTC (windows crate), PowerShell WMI for GPU queries.
- **Rust edition 2024** — requires Rust 1.85+. Verify with `rustc --version`.
- **`Cargo.lock` is gitignored** (atypical for apps). If you add a dependency, the lockfile will be missing from the repo — that's expected.
- No tests, no lint config, no rustfmt config exist.

## Architecture

- **Threading:** 5 background threads communicating over `crossbeam-channel` select loop:
  - `telemetry` (500ms tick, sysinfo + PowerShell GPU)
  - `audio` (~60Hz, WASAPI loopback capture → FFT → 16 log bands)
  - `media` (1s tick, GSMTC polling + MusicBrainz year resolution)
  - `art` (album art downsampling via `image` crate)
  - `input` (50ms poll, crossterm events)
- **GPU metrics** are obtained via `powershell.exe Get-CimInstance` subprocess calls — may be slow; run in the background telemetry thread.
- **Zero-emoji** UI policy — all terminal glyphs use Unicode block elements and ASCII only.
- **Terminal restoration:** `TerminalGuard` drop guard + custom panic hook (via `panic-hook` crate) restores terminal on crash.

## Commands

| Action | Command |
|---|---|
| Build (debug) | `cargo build` |
| Build (release) | `cargo build --release --locked` |
| Run | `cargo run --release` |
| Run auxiliary binary | `cargo run --bin check_cpal` |
| Check compiles | `cargo check` |
| Clean | `cargo clean` |

There is no test, lint, or format step. CI only runs on `v*` tag push (`.github/workflows/release.yml`).

## Design docs

Future feature blueprints live in root Markdown files. These are **planning documents**, not implementation specs — some described features may not be built yet:
- `design_doc.md` — GPU telemetry, dynamic themes, viewport toggling, layout flipping
- `incremental.md` — media timeline bar, Floyd-Steinberg dithering
- `lyric_doc.md` — live synced lyrics subsystem (LRC parser, lrclib.net API)

## Conventions

- GPU rendering uses true-color `Color::Rgb` throughout.
- Album art is a `Vec<Vec<(u8,u8,u8)>>` RGB matrix; height is doubled (`height*2`) for half-block character rendering.
- Network throughput uses delta-based measurement (two consecutive reads).
- Audio FFT: 2048-point, Hann window, 50% overlap, 16 musically-calibrated bands (40 Hz–16 kHz), AGC with rolling peak decay.
