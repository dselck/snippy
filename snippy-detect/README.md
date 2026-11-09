# snippy-detect

## Overview
Crate root and package blueprint for Snippy's automated TV commercial detector, coordinating SCTE-35 digital cue parsing, FFmpeg media demuxing, BS.1770 LUFS/ZCR extraction, GBDT classification, and training tools.

## Interface
- `[build.rs:main]` - Build script resolving `ort` C++ runtime shared library links on Linux
- `src`: [./src/README.md](./src/README.md) - Core detection engine library, pipeline modules, and main detection CLI

## Invariants
- `[build.rs:main]` - On Linux systems missing `libstdc++.so`, creates a compatibility symlink from `libstdc++.so.6` in `OUT_DIR`
- `[Cargo.toml]` - Compiles under Rust edition 2024 with workspace dependency `snippy-schema`
- `[Detector]` - Commercial detection prioritizes explicit SCTE-35 cue markers over ML inference

## Side Effects
- `[build.rs:main]` - Creates filesystem symlinks in `OUT_DIR` and emits `cargo:rustc-link-search` directives
- `[snippy-detect]` - Disk I/O across media files, ONNX/GBDT models, training CSVs, and EDL/JSON cutlists; CPU and hardware acceleration usage for video decoding and model inference

## Verification
- Run: `cargo test -p snippy-detect`
