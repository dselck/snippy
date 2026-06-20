# Design Specification: Configurable Audio Waveform Decibel Scaling

This document details the design for introducing logarithmic decibel (dB) scaling to the audio waveform rendering in Snippy2, complete with a new configuration option for setting the decibel floor.

## Goal

Accentuate the difference between silence/near-silence and low-level audio in the waveform viewer. We use absolute decibel scaling so that the height of the waveform remains stable and consistent (not changing when new segments load or when scrubbing), while mapping the linear RMS amplitude values logarithmically.

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

We will remove the dynamic normalization pass (which computed a local visible peak `scale` and normalized relative to it) and replace it with absolute decibel mapping.

For each bucket index in the rendering loop:
1. Retrieve the linear RMS value `amplitude`.
2. Map to a `scaled_amplitude` value in $[0.0, 1.0]$:
   - If `db_floor >= 0.0`: Use linear scaling directly (`amplitude`).
   - If `amplitude <= 0.0`: Use `0.0`.
   - Else: 
     $$dB = 20 \cdot \log_{10}(amplitude)$$
     $$\text{scaled\_amplitude} = \frac{dB - db\_floor}{-db\_floor}$$
     Clamp the result to $[0.0, 1.0]$.
3. Draw the bar height proportional to `scaled_amplitude`.

## Alternative Approaches Considered & Rejected

- **Dynamic Normalization + Decibel Scaling**: Normalizing the amplitude relative to the peak of the visible frame before applying dB scaling. This was rejected because the height of the waveform changes/jumps dynamically as parts of the video are scrubbed or populated.
- **Power-law / Square Root scaling**: Applying a square root or cube root. This was rejected because standard decibel scaling provides a more precise and controllable threshold to distinguish between background noise/silence and active audio.
