use serde::{Serialize, Deserialize};

/// A single detected commercial segment, as output by `snippy-detect`.
/// Mirrors the internal `CutSegment` but lives in the shared schema crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedSegment {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub start_frame: usize,
    pub end_frame: usize,
    pub confidence: f32,
}

/// Versioned envelope wrapping all `snippy-detect` JSON output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionOutput {
    /// Always 1 for this version of the schema.
    pub schema_version: u32,
    /// Path (or name) of the video file that was analysed.
    pub video_file: String,
    /// `CARGO_PKG_VERSION` of the `snippy-detect` binary that produced this file.
    pub snippy_detect_version: String,
    pub segments: Vec<DetectedSegment>,
}
