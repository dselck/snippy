# snippy-schema/src

## Overview
Shared data models and serialization formats for ground truth video annotations, commercial break transitions, and detection output segments.

## Interface
- `[lib.rs:ground_truth]` - Re-exports all ground truth models, transitions, and block labels
- `[lib.rs:detection_output]` - Re-exports detection segment and output envelopes
- `[ground_truth.rs:BreakType]` - Commercial break type enum: `Commercial`, `Promo`
- `[ground_truth.rs:BreakOrigin]` - Break creation origin enum: `Detected`, `DetectedAdjusted`, `ManuallyAdded`
- `[ground_truth.rs:TransitionPoint]` - Struct holding 2-second block start/end frames and exact cut frame
- `[ground_truth.rs:CommercialBreak]` - Struct representing a labelled commercial break with transitions, confidence, origin, and notes
- `[ground_truth.rs:GroundTruth]` - Complete ground-truth recording annotation struct
- `[ground_truth.rs:GroundTruth::new]` - Minimal `GroundTruth` constructor with defaults (`schema_version: 1`, `timebase: "1/90000"`)
- `[ground_truth.rs:GroundTruth::load]` - Deserializes `GroundTruth` instance from a JSON file on disk
- `[ground_truth.rs:GroundTruth::save]` - Serializes `GroundTruth` instance to pretty-printed JSON file on disk
- `[ground_truth.rs:GroundTruth::label_for_block]` - Evaluates a 2-second frame range against commercial breaks, returning `(BlockLabel, Option<u64>)`
- `[ground_truth.rs:BlockLabel]` - `#[repr(u8)]` classification enum: `Content (0)`, `Commercial (1)`, `Promo (2)`, `Transition (3)`
- `[detection_output.rs:DetectedSegment]` - Single detected commercial segment with timestamps, frames, and confidence
- `[detection_output.rs:DetectionOutput]` - Versioned wrapper struct for `snippy-detect` output segments

## Invariants
- `[ground_truth.rs:GroundTruth::schema_version]` - Always `1` for this schema version
- `[detection_output.rs:DetectionOutput::schema_version]` - Always `1` for this schema version
- `[ground_truth.rs:GroundTruth::new]` - Sets default timebase to `"1/90000"` and formats `labeller_version` using `CARGO_PKG_VERSION`
- `[ground_truth.rs:GroundTruth::label_for_block]` - Returns `BlockLabel::Transition` if `cut_frame` is within `[block_start, block_end)`; `Commercial`/`Promo` only when block boundaries fall entirely inside break cut points; otherwise `Content`
- `[ground_truth.rs:BlockLabel]` - Explicit `u8` discriminant values: `Content=0`, `Commercial=1`, `Promo=2`, `Transition=3`

## Side Effects
- `[ground_truth.rs:GroundTruth::load]` - Reads from filesystem path
- `[ground_truth.rs:GroundTruth::save]` - Writes pretty-printed JSON to filesystem path

## Verification
- Run: `cargo test -p snippy-schema`
