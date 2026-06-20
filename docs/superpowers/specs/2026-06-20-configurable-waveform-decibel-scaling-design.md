# Design Specification: Configurable Audio Waveform Decibel Scaling & Rendering Refinements

This document details the design for introducing logarithmic decibel (dB) scaling to the audio waveform rendering in Snippy2, tripling its visual height, and refining the rendering algorithm using peak decimation to resolve aliasing ("phasing") issues.

## Goal

- Accentuate the difference between silence/near-silence and low-level audio in the waveform viewer.
- Use absolute decibel scaling so that the height of the waveform remains stable and consistent (not changing when new segments load or when scrubbing), while mapping the linear RMS amplitude values logarithmically.
- Triple the visual height of the waveform component (from $24.0$ to $72.0$ pixels) to make it a more prominent visual editing reference.
- Prevent the waveform from aliasing or "phasing" around the screen when zoomed out by replacing point/nearest-neighbor sampling with a peak decimation (maximum amplitude aggregation) over the bucket range corresponding to each pixel column.

## Proposed Changes

### 1. Settings Module (`src/settings.rs`)

We will add a new setting `waveform_db_floor` to the `PlaybackSettings` struct:

```rust
pub struct PlaybackSettings {
    pub volume: f32,
    pub waveform_db_floor: f32,
}
```

- **Default value**: `-45.0` (decibels).
- **Behavior**:
  - Values `< 0.0` apply decibel scaling with the specified value acting as the silence floor (e.g. `-45.0` dB is mapped to $0\%$ height).
  - Values `>= 0.0` disable decibel scaling and fall back to linear scaling (`0.0` to `1.0`).
- **TOML Representation**: We will document and expose this setting in the commented defaults file.

### 2. Waveform Rendering (`src/ui/mod.rs`)

We will:
- Set `wave_height = 72.0;`.
- Calculate the fractional frame range mapped to each pixel column `i` to determine the corresponding range of loudness buckets.
- Aggregated bucket range: `[b_start_d.floor() as isize, b_end_d.ceil() as isize]`.
- Loop over the buckets in this range and find the maximum RMS amplitude (`max_amp`).
- Use the center frame of the column range to check if the frame is in a cut region (`self.is_frame_cut`).
- Scale `max_amp` according to `waveform_db_floor`.
- Draw the vertical line segment from the bottom up to the calculated peak height.

## Alternative Approaches Considered & Rejected

- **Dynamic Normalization + Decibel Scaling**: Normalizing the amplitude relative to the peak of the visible frame before applying dB scaling. This was rejected because the height of the waveform changes/jumps dynamically as parts of the video are scrubbed or populated.
- **Power-law / Square Root scaling**: Applying a square root or cube root. This was rejected because standard decibel scaling provides a more precise and controllable threshold to distinguish between background noise/silence and active audio.
- **Nearest-Neighbor Point Sampling**: Sampling a single bucket index per pixel. This was rejected because it causes severe aliasing (shimmering/phasing) when zoomed out.
