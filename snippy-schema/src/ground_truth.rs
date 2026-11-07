use serde::{Serialize, Deserialize};

/// Whether the break is a paid commercial spot or an in-network promo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakType {
    Commercial,
    Promo,
}

/// How a [`CommercialBreak`] was created.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakOrigin {
    /// Came directly from `snippy-detect` without human review.
    Detected,
    /// Came from `snippy-detect` but the boundaries were adjusted by the labeller.
    DetectedAdjusted,
    /// Created entirely by the labeller; no detector involvement.
    ManuallyAdded,
}

/// A 2-second block boundary containing an edit cut.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionPoint {
    /// First frame of the 2-second block that contains the cut.
    pub block_start_frame: u64,
    /// The exact frame the labeller placed the marker (frame-accurate cut point).
    pub cut_frame: u64,
    /// Last frame of the 2-second block that contains the cut.
    pub block_end_frame: u64,
}

/// A single labelled commercial or promo break with its start/end transitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommercialBreak {
    /// Unique identifier within this ground-truth file (e.g. `"break-001"`).
    pub id: String,
    pub break_type: BreakType,
    pub start_transition: TransitionPoint,
    pub end_transition: TransitionPoint,
    /// `1.0` = manually verified; `< 1.0` = accepted from detector without adjustment.
    pub confidence: f32,
    pub origin: BreakOrigin,
    /// Free-text notes from the labeller (empty string if none).
    #[serde(default)]
    pub notes: String,
}

/// A complete ground-truth annotation file for one video recording.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundTruth {
    /// Always `1` for this schema version.
    pub schema_version: u32,
    /// Path (or filename) of the annotated video.
    pub video_file: String,
    /// SHA-256 hex digest of the video file, for integrity checking.
    pub video_sha256: String,
    /// Total number of frames in the video.
    pub duration_frames: u64,
    /// Frames per second (e.g. `29.97`).
    pub frame_rate: f64,
    /// FFmpeg-style time-base fraction, e.g. `"1/90000"`.
    pub timebase: String,
    /// Broadcast channel name (e.g. `"ITV1"`).
    pub channel: String,
    /// Broadcast region (e.g. `"London"`).
    pub region: String,
    /// ISO-8601 timestamp of when the recording started (optional).
    pub recorded_at: Option<String>,
    /// ISO-8601 timestamp of when labelling was completed (optional).
    pub labelled_at: Option<String>,
    /// Version string of the labelling application.
    pub labeller_version: String,
    /// `CARGO_PKG_VERSION` of the `snippy-detect` binary whose output was used as seed.
    pub snippy_detect_version: String,
    pub commercial_breaks: Vec<CommercialBreak>,
}

impl GroundTruth {
    /// Construct a minimal ground-truth with sensible defaults.
    pub fn new(
        video_file: String,
        video_sha256: String,
        duration_frames: u64,
        frame_rate: f64,
    ) -> Self {
        Self {
            schema_version: 1,
            video_file,
            video_sha256,
            duration_frames,
            frame_rate,
            timebase: "1/90000".to_string(),
            channel: String::new(),
            region: String::new(),
            recorded_at: None,
            labelled_at: None,
            labeller_version: format!("snippy {}", env!("CARGO_PKG_VERSION")),
            snippy_detect_version: String::new(),
            commercial_breaks: Vec::new(),
        }
    }

    /// Deserialise from a JSON file on disk.
    pub fn load(path: &std::path::Path) -> Result<Self, anyhow::Error> {
        let file = std::fs::File::open(path)?;
        Ok(serde_json::from_reader(file)?)
    }

    /// Serialise to a pretty-printed JSON file on disk.
    pub fn save(&self, path: &std::path::Path) -> Result<(), anyhow::Error> {
        let file = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }

    /// Determine the label for a feature block that spans `[block_start, block_end)` frames.
    ///
    /// Returns `(label, cut_frame)` where `cut_frame` is `Some` only for
    /// [`BlockLabel::Transition`] blocks.
    pub fn label_for_block(
        &self,
        block_start: u64,
        block_end: u64,
    ) -> (BlockLabel, Option<u64>) {
        for brk in &self.commercial_breaks {
            let s = &brk.start_transition;
            let e = &brk.end_transition;

            // Block contains the start-transition cut frame
            if block_start <= s.cut_frame && s.cut_frame < block_end {
                return (BlockLabel::Transition, Some(s.cut_frame));
            }
            // Block contains the end-transition cut frame
            if block_start <= e.cut_frame && e.cut_frame < block_end {
                return (BlockLabel::Transition, Some(e.cut_frame));
            }
            // Block lies entirely inside the break (between the two cut points)
            if block_start >= s.cut_frame && block_end <= e.cut_frame {
                return match brk.break_type {
                    BreakType::Commercial => (BlockLabel::Commercial, None),
                    BreakType::Promo => (BlockLabel::Promo, None),
                };
            }
        }
        (BlockLabel::Content, None)
    }
}

/// Classification label for a 2-second feature block.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockLabel {
    Content = 0,
    Commercial = 1,
    Promo = 2,
    Transition = 3,
}
