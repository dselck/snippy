# Configurable Waveform Decibel Scaling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement configurable absolute decibel scaling for the timeline audio waveform viewer to accentuate quiet/near-silent regions.

**Architecture:** 
1. Add a `waveform_db_floor` setting to `PlaybackSettings` in `src/settings.rs` with a default of `-45.0` (dB). Expose it in the commented defaults file.
2. In `src/ui/mod.rs`, replace the dynamic normalization pass with absolute decibel scaling: convert linear RMS values to dB, map the range `[waveform_db_floor, 0.0]` to `[0.0, 1.0]` vertical scale, and default to linear scaling if `waveform_db_floor >= 0.0`.

**Tech Stack:** Rust, egui 0.34.3

---

### Task 1: Update Settings Configuration

**Files:**
- Modify: `src/settings.rs`

- [ ] **Step 1: Add `waveform_db_floor` to `PlaybackSettings` struct and its `Default` implementation**
  Modify the `PlaybackSettings` struct (around lines 59-70) to include the new field, and set its default to `-45.0`.

  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(default)]
  pub struct PlaybackSettings {
      /// Master output volume (0.0 = silent, 1.0 = full volume)
      pub volume: f32,
      /// Decibel floor for the audio waveform viewer (e.g. -45.0).
      /// RMS values below this floor are rendered as flat silence (0% height).
      /// Set to 0.0 or positive to use linear scaling.
      pub waveform_db_floor: f32,
  }

  impl Default for PlaybackSettings {
      fn default() -> Self {
          Self {
              volume: 1.0,
              waveform_db_floor: -45.0,
          }
      }
  }
  ```

- [ ] **Step 2: Add option details to `COMMENTED_DEFAULTS`**
  Modify the `COMMENTED_DEFAULTS` constant (around lines 369-372) to document the configuration option.

  ```rust
  [playback]
  # Master output volume (0.0 = silent, 1.0 = full volume)
  volume = 1.0

  # The decibel floor for the audio waveform viewer (e.g. -45.0).
  # RMS values below this floor are rendered as flat silence (0% height).
  # RMS values at/above 0 dB are rendered at 100% height.
  # Default: -45.0. Set to 0.0 or a positive value to use linear scaling.
  waveform_db_floor = -45.0
  ```

- [ ] **Step 3: Add unit tests for settings serialization and parsing**
  Append a test module at the end of `src/settings.rs` to verify that `waveform_db_floor` parses correctly and defaults correctly.

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_parse_settings_with_waveform_db_floor() {
          let toml_str = r#"
              [playback]
              volume = 0.8
              waveform_db_floor = -50.0
          "#;
          let settings: Settings = toml::from_str(toml_str).unwrap();
          assert_eq!(settings.playback.volume, 0.8);
          assert_eq!(settings.playback.waveform_db_floor, -50.0);
      }

      #[test]
      fn test_default_waveform_db_floor() {
          let settings = Settings::default();
          assert_eq!(settings.playback.waveform_db_floor, -45.0);
      }
  }
  ```

- [ ] **Step 4: Run tests to verify the settings parsing**
  Run: `cargo test --lib settings`
  Expected: PASS

- [ ] **Step 5: Commit settings changes**
  Run:
  ```bash
  git add src/settings.rs
  git commit -m "feat: add configurable waveform_db_floor to settings"
  ```

---

### Task 2: Implement Waveform Decibel Scaling

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Replace dynamic normalization with absolute decibel mapping in waveform renderer**
  Edit `src/ui/mod.rs` to remove the pre-pass scale calculation (`max_amplitude` / `scale`) and instead convert RMS to decibels using the configuration.

  Replace:
  ```rust
                      let first_visible_frame = start_frame.max(0) as usize;
                      let last_visible_frame = (start_frame + visible_range_width as isize)
                          .min(self.playback.total_frames() as isize) as usize;
                      let first_bucket = (first_visible_frame * 4).min(total_buckets);
                      let last_bucket = (last_visible_frame * 4 + 1).min(total_buckets);
                      let max_amplitude = if first_bucket < last_bucket {
                          loudness[first_bucket..last_bucket]
                              .iter()
                              .filter_map(|&amp_opt| amp_opt)
                              .fold(0.0_f32, |a, b| a.max(b))
                      } else {
                          0.0_f32
                      };
                      let scale = if max_amplitude > 0.0 { max_amplitude } else { 1.0 };
  ```

  With:
  ```rust
                      let db_floor = self.settings.playback.waveform_db_floor;
  ```

  And replace the scale calculation inside the drawing loop:
  ```rust
                             let scaled_amplitude = amplitude / scale;
  ```

  With:
  ```rust
                             let scaled_amplitude = if db_floor >= 0.0 {
                                 amplitude
                             } else if amplitude <= 0.0 {
                                 0.0
                             } else {
                                 let db = 20.0 * amplitude.log10();
                                 let pct = (db - db_floor) / -db_floor;
                                 pct.clamp(0.0, 1.0)
                             };
  ```

- [ ] **Step 2: Run cargo check to verify code compiles**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Run the full test suite**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 4: Commit UI changes**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: implement absolute decibel scaling for waveform viewer"
  ```
