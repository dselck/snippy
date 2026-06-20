# Design Specification: Configurable Audio Waveform Decibel Scaling & Rendering Refinements

This document details the design for introducing logarithmic decibel (dB) scaling to the audio waveform rendering in Snippy2, doubling its visual height, correcting the bottom timeline layout, and refining the rendering algorithm using peak decimation to resolve aliasing ("phasing") issues.

## Goal

- Accentuate the difference between silence/near-silence and low-level audio in the waveform viewer.
- Use absolute decibel scaling so that the height of the waveform remains stable and consistent (not changing when new segments load or when scrubbing), while mapping the linear RMS amplitude values logarithmically.
- Double the visual height of the waveform component (from $24.0$ to $48.0$ pixels) to make it a more prominent visual editing reference.
- Correct the bottom timeline layout so that the filmstrip timeline height is dynamically calculated to accommodate the taller waveform, and scale the timeline panel default size and range appropriately.
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

### 2. Waveform Rendering & Layout Correctness (`src/ui/mod.rs`)

We will:
- Set `timeline_height: 180.0;` as the default timeline deck height in `MainApp::new`.
- Adjust the `timeline_deck` panel constraints to `.size_range(130.0..=300.0)`.
- Set `wave_height = 48.0;`.
- Calculate `reserved_for_waveform = wave_height + 12.0;` and use it to size the filmstrip dynamically:
  `filmstrip_height = (ui.available_height() - reserved_for_waveform - spacing_y).max(30.0);`
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
