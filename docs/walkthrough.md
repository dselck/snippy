# Walkthrough: Inline Commercial Detection (`snippy-detect`)

This document summarizes the changes made to implement the native Rust TV commercial detection tool and library (`snippy-detect`), integrated as a Cargo workspace member.

---

## 1. Structural Changes & Sub-Crate
* **Workspace Config**: Restructured the project into a Cargo workspace in the root [Cargo.toml](file:///Users/dselck/Documents/Snippy2/Cargo.toml) to isolate the ML dependencies of `snippy-detect` from the main `snippy2` application.
* **Sub-Crate**: Created [snippy-detect/Cargo.toml](file:///Users/dselck/Documents/Snippy2/snippy-detect/Cargo.toml) configuring dependencies for video decoding (`ffmpeg-next`), ONNX models (`ort` with `ndarray` features), and container metadata parsing (`scte35-reader`, `mpeg2ts-reader`).
* **Pipeline Integration**: Implemented the core engine orchestrator in [lib.rs](file:///Users/dselck/Documents/Snippy2/snippy-detect/src/lib.rs) and the CLI tool args parser in [main.rs](file:///Users/dselck/Documents/Snippy2/snippy-detect/src/main.rs).

---

## 2. Pipeline Modules Implemented

### A. SCTE-35 digital splice parser ([scte35.rs](file:///Users/dselck/Documents/Snippy2/snippy-detect/src/scte35.rs))
* Decodes SCTE-35 section tables (`SpliceInsert` and `TimeSignal` / `SegmentationDescriptor`) directly from raw MPEG-TS containers.
* Chronologically aligns ad-start and ad-end boundaries (handling both duration-specified breaks and standalone open/close tags) to construct frame-accurate cutpoints.

### B. Frame & audio demuxer ([demux.rs](file:///Users/dselck/Documents/Snippy2/snippy-detect/src/demux.rs))
* Sequentially parses packets using `ffmpeg-next`.
* Decodes video keyframes (I-frames) at a sampled interval of 2.0s, scaling them to RGB24.
* Decodes and resamples audio packets to a uniform 16 kHz mono F32 format using `ResamplerContext`.

### C. Feature extractor ([features.rs](file:///Users/dselck/Documents/Snippy2/snippy-detect/src/features.rs))
* Implements the **EBU R128 K-weighting filter** (Stage 1 pre-filter shelving and Stage 2 RLB high-pass filter) using a cascade of custom Biquad filters.
* Computes Momentary/Average LUFS (Loudness Units relative to Full Scale) and Zero Crossing Rate (ZCR) over the audio samples for each temporal block.

### D. ONNX Model Inference & GBDT Classifier ([models.rs](file:///Users/dselck/Documents/Snippy2/snippy-detect/src/models.rs))
* Loads pre-trained visual CLIP, audio YAMNet, and LightGBM classification ONNX models dynamically using `ort` sessions at runtime.
* Packs features into multi-dimensional tensors using `ndarray::ArrayD` and runs inference.
* Implements a rules-based classification algorithm (`predict_commercial_probability`) as a robust fallback if the LightGBM ONNX model is missing.
* Groups contiguous commercial frames, checks minimum duration requirements, applies start/end padding, and maps timings back to video frame indices.

### E. Format Export Writers ([output.rs](file:///Users/dselck/Documents/Snippy2/snippy-detect/src/output.rs))
* Implements standard Edit Decision List (EDL) writer: `[Start] [End] 0` format (skip action).
* Implements a pretty-printed JSON metadata writer.

---

## 3. Verification & Testing

All components are covered by a suite of 9 unit tests:
1. `models::tests::test_predict_commercial_probability`: Verifies that the rules-based GBDT fallback correctly distinguishes ad metrics from content.
2. `models::tests::test_classify_segments_merging_and_padding`: Verifies that contiguous ad blocks are merged and padding offsets are correctly applied.
3. `features::tests::test_zcr_and_lufs`: Validates K-weighting filter shapes, LUFS, and ZCR mathematical correctness on a generated 1000Hz sine wave.
4. `scte35::tests::test_get_pts`: Validates 33-bit PTS adjustments and overflow wrapping.
5. `scte35::tests::test_process_events_duration`: Verifies SCTE-35 duration-specified splice calculations.
6. `scte35::tests::test_process_events_start_end_matching`: Verifies SCTE-35 start/end event boundary matching.
7. `scte35::tests::test_process_events_multiple`: Validates sequence handling of multiple interleaved SCTE-35 markers.
8. `output::tests::test_write_edl`: Validates correct `.edl` file writing formatting.
9. `output::tests::test_write_json`: Validates correct `.json` serialization.

To run tests:
```bash
cargo test -p snippy-detect
```
