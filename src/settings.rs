use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ZoomUnit {
    Seconds,
    Frames,
}

impl Default for ZoomUnit {
    fn default() -> Self {
        ZoomUnit::Seconds
    }
}

impl ZoomUnit {
    pub fn label(&self) -> &'static str {
        match self {
            ZoomUnit::Seconds => "seconds",
            ZoomUnit::Frames => "frames",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ZoomBehavior {
    Linear,
    Exponential,
}

impl Default for ZoomBehavior {
    fn default() -> Self {
        ZoomBehavior::Exponential
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlayheadMode {
    CenterAnchored,
    FreeTravelling,
}

impl Default for PlayheadMode {
    fn default() -> Self {
        PlayheadMode::CenterAnchored
    }
}

impl PlayheadMode {
    pub fn label(&self) -> &'static str {
        match self {
            PlayheadMode::CenterAnchored => "Center Locked",
            PlayheadMode::FreeTravelling => "Free Moving",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[serde(alias = "dark")]
    VideoEditorDark,
    SleekMidnight,
    HighContrastOled,
    ClassicIndigoDark,
    VideoEditorLight,
    ClassicWarmLight,
    CustomDynamic,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::VideoEditorDark
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CustomMode {
    Dark,
    Light,
    Custom,
}

impl Default for CustomMode {
    fn default() -> Self {
        CustomMode::Dark
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ColorHarmony {
    Complementary,
    SplitComplementary,
    Analogous,
    Triadic,
    Tetradic,
    Monochromatic,
}

impl Default for ColorHarmony {
    fn default() -> Self {
        ColorHarmony::Complementary
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticStyle {
    Classic,
    Harmonized,
}

impl Default for SemanticStyle {
    fn default() -> Self {
        SemanticStyle::Classic
    }
}

// ---------------------------------------------------------------------------
// Setting groups
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackSettings {
    /// Master output volume (0.0 = silent, 1.0 = full volume)
    pub volume: f32,
    /// Decibel floor for the audio waveform viewer (e.g. -45.0).
    /// RMS values below this floor are rendered as flat silence (0% height).
    /// Set to 0.0 or positive to use linear scaling.
    pub waveform_db_floor: f32,
    /// Timeline playhead navigation mode (Center-locked scrolling vs Free-moving)
    pub playhead_mode: PlayheadMode,
    /// Selected audio stream index (None = use primary/best audio stream)
    pub selected_audio_track: Option<usize>,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            waveform_db_floor: -45.0,
            playhead_mode: PlayheadMode::CenterAnchored,
            selected_audio_track: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HardwareSettings {
    /// Enable GPU-accelerated video decoding (VideoToolbox / D3D11 / VAAPI)
    pub playback_hw_accel: bool,
    /// Enable GPU-accelerated thumbnail decoding (same backends)
    pub thumbnails_hw_accel: bool,
}

impl Default for HardwareSettings {
    fn default() -> Self {
        Self {
            playback_hw_accel: true,
            thumbnails_hw_accel: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThumbnailSettings {
    /// Number of parallel worker threads in the Thumbnail Decoder Pool
    pub decoder_threads: usize,
    /// Maximum entries in the Thumbnail LRU Cache
    pub lru_cache_size: usize,
}

impl Default for ThumbnailSettings {
    fn default() -> Self {
        Self {
            decoder_threads: 4,
            lru_cache_size: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedSettings {
    /// Presenter lookahead queue capacity (frames buffered ahead of playhead)
    pub presenter_lookahead_depth: usize,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            presenter_lookahead_depth: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppearanceSettings {
    pub theme: Theme,
    pub custom_theme_mode: CustomMode,
    pub custom_theme_hue: f32,
    pub custom_theme_chroma: f32,
    
    // Custom overrides
    pub custom_bg_lightness: f32,
    pub custom_bg_chroma: f32,
    pub custom_accent_lightness: f32,
    pub custom_neutral_hue: f32,
    pub custom_harmony: ColorHarmony,
    pub custom_secondary_chroma: f32,
    pub custom_semantic_style: SemanticStyle,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: Theme::VideoEditorDark,
            custom_theme_mode: CustomMode::Dark,
            custom_theme_hue: 250.0,
            custom_theme_chroma: 0.15,
            custom_bg_lightness: 0.08,
            custom_bg_chroma: 0.015,
            custom_accent_lightness: 0.65,
            custom_neutral_hue: 250.0,
            custom_harmony: ColorHarmony::Complementary,
            custom_secondary_chroma: 0.10,
            custom_semantic_style: SemanticStyle::Classic,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineSettings {
    /// Starting Zoom Interval value when a file is opened
    pub default_zoom_value: f64,
    pub default_zoom_unit: ZoomUnit,
    /// Minimum Zoom Interval (most zoomed in)
    pub min_zoom_value: f64,
    pub min_zoom_unit: ZoomUnit,
    /// Maximum Zoom Interval (most zoomed out)
    pub max_zoom_value: f64,
    pub max_zoom_unit: ZoomUnit,
    /// Zoom curve shape: exponential or linear
    pub zoom_behavior: ZoomBehavior,
}

impl Default for TimelineSettings {
    fn default() -> Self {
        Self {
            default_zoom_value: 5.0,
            default_zoom_unit: ZoomUnit::Seconds,
            min_zoom_value: 1.0,
            min_zoom_unit: ZoomUnit::Frames,
            max_zoom_value: 30.0,
            max_zoom_unit: ZoomUnit::Seconds,
            zoom_behavior: ZoomBehavior::Exponential,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// Recently opened files, newest first (capped at MAX_RECENT_FILES)
    pub recent_files: Vec<String>,
    /// Default directory for the file picker. Empty = OS default.
    pub default_open_directory: String,
    /// Default snippet edit mode: true = Cut mode, false = Keep mode
    pub default_cut_mode: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            recent_files: vec![],
            default_open_directory: String::new(),
            default_cut_mode: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Root struct
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub playback: PlaybackSettings,
    pub hardware: HardwareSettings,
    pub thumbnails: ThumbnailSettings,
    pub advanced: AdvancedSettings,
    pub appearance: AppearanceSettings,
    pub timeline: TimelineSettings,
    pub general: GeneralSettings,
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

const MAX_RECENT_FILES: usize = 10;

/// Returns the path to the settings file in the OS config directory.
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("snippy").join("settings.toml"))
}

#[derive(Debug)]
pub enum LoadError {
    Io(String),
    Parse(String),
}

impl LoadError {
    pub fn message(&self) -> String {
        match self {
            LoadError::Io(e) => format!("Could not read settings file: {e}"),
            LoadError::Parse(e) => format!(
                "Settings file was invalid and has been renamed to settings.toml.bak. \
                 Defaults restored. Error: {e}"
            ),
        }
    }
}

impl Settings {
    /// Load Settings from disk. On first launch writes a default commented file.
    /// On a corrupt file, renames it to `.bak`, writes fresh defaults, and returns the error.
    /// Individual missing fields fall back to their coded defaults via `#[serde(default)]`.
    pub fn load() -> (Self, Option<LoadError>) {
        let path = match config_path() {
            Some(p) => p,
            None => return (Self::default(), None),
        };

        if !path.exists() {
            let settings = Self::default();
            let _ = Self::write_commented_defaults(&path);
            return (settings, None);
        }

        match std::fs::read_to_string(&path) {
            Err(e) => {
                let settings = Self::default();
                let _ = Self::write_commented_defaults(&path);
                (settings, Some(LoadError::Io(e.to_string())))
            }
            Ok(contents) => match toml::from_str::<Settings>(&contents) {
                Ok(s) => (s, None),
                Err(e) => {
                    // Rename the bad file so the user can recover their edits.
                    let bak = path.with_extension("toml.bak");
                    let _ = std::fs::rename(&path, &bak);
                    let settings = Self::default();
                    let _ = Self::write_commented_defaults(&path);
                    (settings, Some(LoadError::Parse(e.to_string())))
                }
            },
        }
    }

    /// Write Settings to disk (called on Settings modal close).
    pub fn save(&self) {
        if let Some(path) = config_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(toml_str) = toml::to_string_pretty(self) {
                let _ = std::fs::write(&path, toml_str);
            }
        }
    }

    /// Write a human-readable, fully-commented default file to `path`.
    fn write_commented_defaults(path: &PathBuf) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, COMMENTED_DEFAULTS)
    }

    /// Add `path` to recent files, deduplicating and capping at MAX_RECENT_FILES.
    pub fn add_recent_file(&mut self, path: &str) {
        self.general.recent_files.retain(|p| p != path);
        self.general.recent_files.insert(0, path.to_string());
        self.general.recent_files.truncate(MAX_RECENT_FILES);
    }

    // -----------------------------------------------------------------------
    // Zoom helpers
    // -----------------------------------------------------------------------

    /// Convert a value+unit pair to seconds given the video fps.
    pub fn zoom_to_seconds(value: f64, unit: &ZoomUnit, fps: f64) -> f64 {
        match unit {
            ZoomUnit::Seconds => value,
            ZoomUnit::Frames => value / fps.max(1.0),
        }
    }

    /// Compute the normalized zoom slider position [0.0..=1.0] from an interval in seconds.
    /// 0.0 = most zoomed out (max interval), 1.0 = most zoomed in (min interval).
    pub fn interval_to_zoom_level(&self, interval_sec: f64, fps: f64) -> f32 {
        let min = Self::zoom_to_seconds(
            self.timeline.min_zoom_value,
            &self.timeline.min_zoom_unit,
            fps,
        )
        .max(1e-6);
        let max = Self::zoom_to_seconds(
            self.timeline.max_zoom_value,
            &self.timeline.max_zoom_unit,
            fps,
        );
        if (max - min).abs() < 1e-9 {
            return 0.0;
        }
        let level = match self.timeline.zoom_behavior {
            ZoomBehavior::Exponential => (interval_sec / max).ln() / (min / max).ln(),
            ZoomBehavior::Linear => (interval_sec - max) / (min - max),
        };
        level.clamp(0.0, 1.0) as f32
    }

    /// Compute the interval in seconds for a given normalized zoom slider position.
    pub fn zoom_level_to_interval(&self, zoom_level: f32, fps: f64) -> f64 {
        let min = Self::zoom_to_seconds(
            self.timeline.min_zoom_value,
            &self.timeline.min_zoom_unit,
            fps,
        )
        .max(1e-6);
        let max = Self::zoom_to_seconds(
            self.timeline.max_zoom_value,
            &self.timeline.max_zoom_unit,
            fps,
        );
        match self.timeline.zoom_behavior {
            ZoomBehavior::Exponential => max * (min / max).powf(zoom_level as f64),
            ZoomBehavior::Linear => max + (min - max) * zoom_level as f64,
        }
    }

    /// Default zoom level (0.0..=1.0) derived from the configured default Zoom Interval.
    pub fn default_zoom_level(&self, fps: f64) -> f32 {
        let default_sec = Self::zoom_to_seconds(
            self.timeline.default_zoom_value,
            &self.timeline.default_zoom_unit,
            fps,
        );
        self.interval_to_zoom_level(default_sec, fps)
    }
}

// ---------------------------------------------------------------------------
// Commented default file contents
// ---------------------------------------------------------------------------

const COMMENTED_DEFAULTS: &str = r#"# Snippy Settings
# This file is auto-generated. Edit it to customise Snippy's behaviour.
# Common options are also available from Settings (⌘,) inside the app.
# After editing this file, restart Snippy for the changes to take effect.

[playback]
# Master output volume (0.0 = silent, 1.0 = full volume)
volume = 1.0

# The decibel floor for the audio waveform viewer (e.g. -45.0).
# RMS values below this floor are rendered as flat silence (0% height).
# RMS values at/above 0 dB are rendered at 100% height.
# Default: -45.0. Set to 0.0 or a positive value to use linear scaling.
waveform_db_floor = -45.0

[hardware]
# Enable GPU-accelerated video decoding.
# Uses VideoToolbox on macOS, D3D11 on Windows, VAAPI on Linux.
# Disabling this forces software decoding (slower, but more compatible).
playback_hw_accel = true

# Enable GPU-accelerated thumbnail decoding (same backends as above).
# Can be toggled independently of playback_hw_accel to reduce GPU load.
thumbnails_hw_accel = true

[thumbnails]
# Number of parallel background threads in the Thumbnail Decoder Pool.
# Higher values improve filmstrip load speed on multi-core machines.
# Recommended: 2–8. Default: 4.
decoder_threads = 4

# Maximum number of thumbnail textures kept in the Thumbnail LRU Cache.
# Higher values consume more GPU memory. Default: 300.
lru_cache_size = 300

[advanced]
# Number of decoded video frames buffered in the presenter lookahead queue.
# Larger values smooth playback at the cost of higher latency after seeks.
# Recommended: 2–8. Default: 4.
presenter_lookahead_depth = 4

[appearance]
# UI theme. Currently only "dark" is supported.
theme = "dark"

[timeline]
# Starting Zoom Interval when a file is opened.
# Expressed as how much video one filmstrip thumbnail step represents.
default_zoom_value = 5.0
default_zoom_unit = "seconds"  # "seconds" or "frames"

# Minimum Zoom Interval (most zoomed in — fewest frames visible at once).
min_zoom_value = 1.0
min_zoom_unit = "frames"  # "seconds" or "frames"

# Maximum Zoom Interval (most zoomed out — most frames visible at once).
max_zoom_value = 30.0
max_zoom_unit = "seconds"  # "seconds" or "frames"

# Zoom curve shape applied by the filmstrip slider.
# "exponential" gives finer control when zoomed in (recommended for editing).
# "linear" gives uniform steps across the full range.
zoom_behavior = "exponential"  # "exponential" or "linear"

[general]
# Recently opened files — managed automatically (newest first, max 10).
recent_files = []

# Default directory opened by the file picker. Leave empty to use the OS default.
default_open_directory = ""
"#;

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

