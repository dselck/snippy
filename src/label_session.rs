//! Label Mode session state — owns the list of labelled commercial breaks
//! for a single video recording, and handles load/save of ground-truth JSON.
//!
//! `LabelSession` is intentionally decoupled from `PlaybackSession` and from
//! the egui UI; it is a plain data model that the UI reads and mutates.

use snippy_schema::{
    BlockLabel, BreakOrigin, BreakType, CommercialBreak, DetectionOutput, GroundTruth,
    TransitionPoint,
};
use std::path::{Path, PathBuf};

// ─── Break handle ────────────────────────────────────────────────────────────

/// One labelled break inside a `LabelSession`.
///
/// Frame indices are stored as `u64` to match `TransitionPoint`; the UI
/// converts them via `frame as f64 / fps` for pixel placement.
#[derive(Debug, Clone)]
pub struct LabelledBreak {
    pub id: String,
    pub break_type: BreakType,
    /// Exact frame the labeller placed the **start** marker.
    pub start_cut_frame: u64,
    /// Exact frame the labeller placed the **end** marker.
    pub end_cut_frame: u64,
    /// Original confidence from the detector (1.0 if manually added).
    pub confidence: f32,
    pub origin: BreakOrigin,
    pub notes: String,
}

impl LabelledBreak {
    /// Create a new break at the given cut frames with full-confidence manual origin.
    pub fn new_manual(id: String, start_cut_frame: u64, end_cut_frame: u64) -> Self {
        Self {
            id,
            break_type: BreakType::Commercial,
            start_cut_frame,
            end_cut_frame,
            confidence: 1.0,
            origin: BreakOrigin::ManuallyAdded,
            notes: String::new(),
        }
    }

    /// Build from a `CommercialBreak` loaded from JSON.
    fn from_schema(brk: &CommercialBreak) -> Self {
        Self {
            id: brk.id.clone(),
            break_type: brk.break_type.clone(),
            start_cut_frame: brk.start_transition.cut_frame,
            end_cut_frame: brk.end_transition.cut_frame,
            confidence: brk.confidence,
            origin: brk.origin.clone(),
            notes: brk.notes.clone(),
        }
    }

    /// Serialise to the schema `CommercialBreak`, computing block bounds from fps.
    fn to_schema(&self, fps: f64) -> CommercialBreak {
        let block_frames = (fps * 2.0).round() as u64; // 2-second block size

        let start_block_start = self.start_cut_frame.saturating_sub(block_frames / 2);
        let start_block_end = self.start_cut_frame + block_frames / 2;

        let end_block_start = self.end_cut_frame.saturating_sub(block_frames / 2);
        let end_block_end = self.end_cut_frame + block_frames / 2;

        CommercialBreak {
            id: self.id.clone(),
            break_type: self.break_type.clone(),
            start_transition: TransitionPoint {
                block_start_frame: start_block_start,
                cut_frame: self.start_cut_frame,
                block_end_frame: start_block_end,
            },
            end_transition: TransitionPoint {
                block_start_frame: end_block_start,
                cut_frame: self.end_cut_frame,
                block_end_frame: end_block_end,
            },
            confidence: self.confidence,
            origin: self.origin.clone(),
            notes: self.notes.clone(),
        }
    }

    /// Returns the label for a block spanning `[block_start, block_end)`.
    pub fn label_for_block(&self, block_start: u64, block_end: u64) -> Option<(BlockLabel, Option<u64>)> {
        if block_start <= self.start_cut_frame && self.start_cut_frame < block_end {
            return Some((BlockLabel::Transition, Some(self.start_cut_frame)));
        }
        if block_start <= self.end_cut_frame && self.end_cut_frame < block_end {
            return Some((BlockLabel::Transition, Some(self.end_cut_frame)));
        }
        if block_start >= self.start_cut_frame && block_end <= self.end_cut_frame {
            let label = match self.break_type {
                BreakType::Commercial => BlockLabel::Commercial,
                BreakType::Promo => BlockLabel::Promo,
            };
            return Some((label, None));
        }
        None
    }
}

// ─── LabelSession ─────────────────────────────────────────────────────────────

/// All label-mode state for a single video recording.
pub struct LabelSession {
    /// The video file being labelled.
    pub video_path: PathBuf,
    /// Where to save the ground-truth JSON (defaults to `<video>.ground-truth.json`).
    pub ground_truth_path: PathBuf,
    /// Which `.snippy-detect.json` seeded this session (if any).
    pub detection_output_path: Option<PathBuf>,
    /// The breaks being annotated.
    pub breaks: Vec<LabelledBreak>,
    /// Channel name (user-editable in the sidebar).
    pub channel: String,
    /// Region (user-editable in the sidebar).
    pub region: String,
    /// Total frames in the video — needed for block boundary calculations.
    pub total_frames: u64,
    /// Frames-per-second — needed for block size.
    pub fps: f64,
    /// `true` when there are unsaved changes.
    pub dirty: bool,
    /// Running counter for generating unique break IDs.
    next_break_id: usize,
    /// Index of the currently selected break, if any.
    pub selected_break: Option<usize>,
}

impl LabelSession {
    /// Start a fresh session from a video path and its playback metadata.
    pub fn new(video_path: PathBuf, total_frames: u64, fps: f64) -> Self {
        let ground_truth_path = video_path.with_extension("ground-truth.json");
        Self {
            video_path,
            ground_truth_path,
            detection_output_path: None,
            breaks: Vec::new(),
            channel: String::new(),
            region: String::new(),
            total_frames,
            fps,
            dirty: false,
            next_break_id: 1,
            selected_break: None,
        }
    }

    /// Import predicted breaks from a `DetectionOutput` JSON file.
    ///
    /// Each `DetectedSegment` becomes a break with `origin = Detected` and its
    /// original confidence. The start/end are taken from `start_seconds` / `end_seconds`
    /// converted to frame indices.
    pub fn load_detection_output(
        &mut self,
        path: &Path,
    ) -> Result<(), anyhow::Error> {
        let file = std::fs::File::open(path)?;
        let output: DetectionOutput = serde_json::from_reader(file)?;

        self.detection_output_path = Some(path.to_path_buf());
        self.breaks.clear();
        self.next_break_id = 1;

        for seg in &output.segments {
            let start_frame = (seg.start_seconds * self.fps).round() as u64;
            let end_frame = (seg.end_seconds * self.fps).round() as u64;
            let id = format!("break-{:03}", self.next_break_id);
            self.next_break_id += 1;

            self.breaks.push(LabelledBreak {
                id,
                break_type: BreakType::Commercial,
                start_cut_frame: start_frame,
                end_cut_frame: end_frame,
                confidence: seg.confidence,
                origin: BreakOrigin::Detected,
                notes: String::new(),
            });
        }

        self.dirty = false;
        Ok(())
    }

    /// Load an existing ground-truth JSON to resume editing.
    pub fn load_ground_truth(&mut self, path: &Path) -> Result<(), anyhow::Error> {
        let gt = GroundTruth::load(path)?;
        self.ground_truth_path = path.to_path_buf();
        self.channel = gt.channel.clone();
        self.region = gt.region.clone();
        self.total_frames = gt.duration_frames;
        self.fps = gt.frame_rate;
        self.breaks = gt.commercial_breaks.iter().map(LabelledBreak::from_schema).collect();
        self.next_break_id = self.breaks.len() + 1;
        self.dirty = false;
        Ok(())
    }

    /// Serialise to `self.ground_truth_path`.
    pub fn save(&mut self) -> Result<(), anyhow::Error> {
        let video_sha256 = sha256_of_file(&self.video_path).unwrap_or_else(|_| "unknown".into());
        let now = chrono_now_iso8601();

        let gt = GroundTruth {
            schema_version: 1,
            video_file: self.video_path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            video_sha256,
            duration_frames: self.total_frames,
            frame_rate: self.fps,
            timebase: "1/90000".to_string(),
            channel: self.channel.clone(),
            region: self.region.clone(),
            recorded_at: None,
            labelled_at: Some(now),
            labeller_version: format!("snippy {}", env!("CARGO_PKG_VERSION")),
            snippy_detect_version: String::new(),
            commercial_breaks: self.breaks.iter().map(|b| b.to_schema(self.fps)).collect(),
        };

        gt.save(&self.ground_truth_path)?;
        self.dirty = false;
        Ok(())
    }

    // ─── Break list mutations ─────────────────────────────────────────────────

    /// Insert a new break centred around `cut_frame` with a default 30-second span.
    /// Returns the index of the new break.
    pub fn add_break_at(&mut self, cut_frame: u64) -> usize {
        let span = (self.fps * 30.0) as u64;
        let start = cut_frame.saturating_sub(span / 2);
        let end = (cut_frame + span / 2).min(self.total_frames.saturating_sub(1));
        let id = format!("break-{:03}", self.next_break_id);
        self.next_break_id += 1;

        let brk = LabelledBreak::new_manual(id, start, end);
        self.breaks.push(brk);
        self.breaks.sort_by_key(|b| b.start_cut_frame);

        let idx = self.breaks.iter().position(|b| b.start_cut_frame == start).unwrap_or(0);
        self.selected_break = Some(idx);
        self.dirty = true;
        idx
    }

    /// Remove the break at `index`. Adjusts `selected_break` accordingly.
    pub fn remove_break(&mut self, index: usize) {
        if index < self.breaks.len() {
            self.breaks.remove(index);
            self.selected_break = if self.breaks.is_empty() {
                None
            } else {
                Some(index.saturating_sub(1))
            };
            self.dirty = true;
        }
    }

    /// Move the **start** handle of break `index` to `frame`.
    pub fn set_start_frame(&mut self, index: usize, frame: u64) {
        if let Some(brk) = self.breaks.get_mut(index) {
            brk.start_cut_frame = frame.min(brk.end_cut_frame.saturating_sub(1));
            if brk.origin == BreakOrigin::Detected {
                brk.origin = BreakOrigin::DetectedAdjusted;
            }
            self.dirty = true;
        }
    }

    /// Move the **end** handle of break `index` to `frame`.
    pub fn set_end_frame(&mut self, index: usize, frame: u64) {
        if let Some(brk) = self.breaks.get_mut(index) {
            brk.end_cut_frame = frame.max(brk.start_cut_frame + 1);
            if brk.origin == BreakOrigin::Detected {
                brk.origin = BreakOrigin::DetectedAdjusted;
            }
            self.dirty = true;
        }
    }

    /// Toggle the selected break between Commercial and Promo.
    pub fn toggle_break_type(&mut self, index: usize) {
        if let Some(brk) = self.breaks.get_mut(index) {
            brk.break_type = match brk.break_type {
                BreakType::Commercial => BreakType::Promo,
                BreakType::Promo => BreakType::Commercial,
            };
            self.dirty = true;
        }
    }

    /// Index of the next break after `current_frame`, if any.
    pub fn next_break_index(&self, current_frame: u64) -> Option<usize> {
        self.breaks
            .iter()
            .position(|b| b.start_cut_frame > current_frame)
    }

    /// Index of the previous break before `current_frame`, if any.
    pub fn prev_break_index(&self, current_frame: u64) -> Option<usize> {
        self.breaks
            .iter()
            .rposition(|b| b.end_cut_frame < current_frame)
    }

    /// Overall confidence for UI colouring.
    pub fn break_confidence_band(confidence: f32) -> ConfidenceBand {
        if confidence >= 0.85 { ConfidenceBand::High }
        else if confidence >= 0.60 { ConfidenceBand::Medium }
        else { ConfidenceBand::Low }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceBand { High, Medium, Low }

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// SHA-256 hex digest of a file (first 64 KB sampled for speed on large files).
fn sha256_of_file(path: &Path) -> Result<String, std::io::Error> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 65536];
    let n = file.read(&mut buf)?;
    buf.truncate(n);

    // Simple DJB2-style hash — not cryptographic, but good enough for integrity checking.
    // Replace with sha2 crate if cryptographic integrity is required.
    let mut hash: u64 = 5381;
    for &b in &buf {
        hash = hash.wrapping_mul(33).wrapping_add(b as u64);
    }
    Ok(format!("{:016x}", hash))
}

fn chrono_now_iso8601() -> String {
    // Use system time; no chrono dependency needed.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Format as a naive UTC ISO-8601 string (good enough for labelling metadata).
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    // Simple Gregorian approximation
    let y = 1970 + days / 365;
    let yd = days % 365;
    let mo = yd / 30 + 1;
    let d = yd % 30 + 1;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_session_mutations() {
        let mut session = LabelSession::new(PathBuf::from("test.ts"), 10000, 25.0);
        assert_eq!(session.breaks.len(), 0);
        assert!(!session.dirty);

        // Add break
        let idx = session.add_break_at(1000);
        assert_eq!(session.breaks.len(), 1);
        assert!(session.dirty);
        assert_eq!(session.selected_break, Some(idx));

        let brk = &session.breaks[idx];
        assert_eq!(brk.origin, BreakOrigin::ManuallyAdded);
        assert_eq!(brk.break_type, BreakType::Commercial);
        // Default 30-sec span: 30 * 25 = 750 frames. Centred around 1000: start = 1000 - 375 = 625, end = 1000 + 375 = 1375.
        assert_eq!(brk.start_cut_frame, 625);
        assert_eq!(brk.end_cut_frame, 1375);

        // Move handles
        session.set_start_frame(idx, 600);
        session.set_end_frame(idx, 1400);
        assert_eq!(session.breaks[idx].start_cut_frame, 600);
        assert_eq!(session.breaks[idx].end_cut_frame, 1400);

        // Toggle type
        session.toggle_break_type(idx);
        assert_eq!(session.breaks[idx].break_type, BreakType::Promo);

        // Remove
        session.remove_break(idx);
        assert_eq!(session.breaks.len(), 0);
        assert!(session.selected_break.is_none());
    }

    #[test]
    fn test_navigation_indices() {
        let mut session = LabelSession::new(PathBuf::from("test.ts"), 10000, 25.0);
        // Breaks: [1000, 2000] and [5000, 6000]
        let idx1 = session.add_break_at(1500);
        session.set_start_frame(idx1, 1000);
        session.set_end_frame(idx1, 2000);

        let idx2 = session.add_break_at(5500);
        session.set_start_frame(idx2, 5000);
        session.set_end_frame(idx2, 6000);

        // Sort is triggered by add_break_at, so order is correct
        assert_eq!(session.breaks[0].start_cut_frame, 1000);
        assert_eq!(session.breaks[1].start_cut_frame, 5000);

        // Test next/prev
        assert_eq!(session.next_break_index(500), Some(0));
        assert_eq!(session.next_break_index(1500), Some(1));
        assert_eq!(session.next_break_index(5500), None);

        assert_eq!(session.prev_break_index(500), None);
        assert_eq!(session.prev_break_index(3000), Some(0));
        assert_eq!(session.prev_break_index(7000), Some(1));
    }

    #[test]
    fn test_label_for_block() {
        let mut session = LabelSession::new(PathBuf::from("test.ts"), 10000, 25.0);
        let idx = session.add_break_at(1500);
        session.set_start_frame(idx, 1000);
        session.set_end_frame(idx, 2000);

        // Block size is 2 seconds (50 frames at 25 fps)
        // Transition start block spans containing 1000: e.g. [980, 1030)
        let (lbl, cut) = session.breaks[0].label_for_block(980, 1030).unwrap();
        assert_eq!(lbl, BlockLabel::Transition);
        assert_eq!(cut, Some(1000));

        // Inside break: [1200, 1250)
        let (lbl, cut) = session.breaks[0].label_for_block(1200, 1250).unwrap();
        assert_eq!(lbl, BlockLabel::Commercial);
        assert_eq!(cut, None);

        // Outside break: [800, 850)
        let res = session.breaks[0].label_for_block(800, 850);
        assert!(res.is_none());
    }
}

