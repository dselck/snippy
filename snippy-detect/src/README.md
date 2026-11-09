# snippy-detect/src

## Overview
Core commercial detection engine library and CLI, providing SCTE-35 marker parsing, audio/video demuxing and keyframe decoding, BS.1770 LUFS and ZCR feature extraction, GBDT and ONNX classification, and EDL/JSON export.

## Interface
- `[lib.rs:Detector]` - Pipeline coordinator executing SCTE-35 probe, media decoding, feature extraction, and ML classification
- `[lib.rs:Config]` - Detection configuration (`classification_threshold`, `min_commercial_break_duration`, `padding`, ONNX paths)
- `[lib.rs:CutSegment]` - Represents detected commercial interval with timestamps, frames, and confidence
- `[main.rs:main]` - CLI entry point running commercial detection on video files with EDL or JSON export
- `[scte35.rs:parse_scte35]` - Extracts SCTE-35 digital cue splice markers from MPEG-TS streams
- `[demux.rs:decode_media]` - Decodes video keyframes (RGB24) and resamples audio to 16 kHz mono PCM
- `[demux.rs:MediaData]` - Container for decoded keyframes and 16 kHz audio PCM
- `[features.rs:extract_features]` - Computes K-weighted LUFS, zero crossing rate, and time blocks
- `[features.rs:BlockFeatures]` - Extracted audio/visual block feature metrics
- `[models.rs:classify_segments]` - Merges high-probability blocks into padded commercial segments
- `[models.rs:NanogbmModel]` - Pure Rust GBDT model classifier wrapper
- `[models.rs:ClipModel]` - ONNX Runtime visual feature encoder
- `[models.rs:YamnetModel]` - ONNX Runtime audio classifier
- `[models.rs:predict_commercial_probability]` - Fallback heuristic commercial probability estimator
- `[output.rs:write_output]` - Exports cut segments to Comskip EDL or versioned DetectionOutput JSON
- `bin`: [./bin/README.md](./bin/README.md) - Feature extraction and GBDT model training CLI utilities

## Invariants
- `[lib.rs:Detector::detect]` - SCTE-35 cue markers take immediate precedence; if found, media decode and ML inference are skipped
- `[scte35.rs:process_events]` - MPEG-TS PTS clock assumes 90 kHz time base; PTS wrapping masks at 33 bits (`& 0x1_FFFF_FFFF`)
- `[scte35.rs:parse_scte35]` - Cue markers output `CutSegment` with confidence `1.0`; non-existent files return `Ok(None)`
- `[demux.rs:decode_media]` - Audio is resampled to 16,000 Hz single-channel (mono) packed f32 PCM; video keyframes are throttled to `>= 2.0s` intervals
- `[features.rs:extract_features]` - Audio loudness uses 2-stage ITU-R BS.1770-4 K-weighting filters; energy below `1e-10` clamps to `-70.0` LUFS
- `[models.rs:classify_segments]` - Blocks with probability `>= classification_threshold` are merged; breaks shorter than `min_commercial_break_duration` are discarded; start padding is clamped to `>= 0.0s`
- `[output.rs:write_output]` - File extension `.edl` outputs 3-column tab-delimited text; any other extension outputs `schema_version: 1` JSON envelope

## Side Effects
- `[demux.rs:decode_media]` - Reads input video/audio file; initializes FFmpeg decoders, hardware transfer contexts, and resamplers
- `[scte35.rs:parse_scte35]` - Reads MPEG-TS stream in 188-byte packet chunks
- `[models.rs:ClipModel::new, YamnetModel::new]` - Loads ONNX model files from disk; allocates ONNX runtime sessions
- `[models.rs:NanogbmModel::new]` - Loads model JSON from `resources/models/classifier.json`
- `[output.rs:write_output]` - Creates and writes cutlist files to disk
- `[models.rs:classify_segments]` - Emits fallback warnings to stderr if GBDT model fails

## Verification
- Run: `cargo test -p snippy-detect`
