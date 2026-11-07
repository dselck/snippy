use std::path::Path;
use serde::{Serialize, Deserialize};

pub mod scte35;
pub mod demux;
pub mod features;
pub mod models;
pub mod output;

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub classification_threshold: f32,
    pub min_commercial_break_duration: f32,
    pub padding_start_seconds: f32,
    pub padding_end_seconds: f32,
    pub onnx_clip_path: String,
    pub onnx_yamnet_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CutSegment {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub start_frame: usize,
    pub end_frame: usize,
    pub confidence: f32,
}

pub struct Detector {
    config: Config,
}

impl Detector {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn detect<P: AsRef<Path>>(&self, path: P) -> Result<Vec<CutSegment>, anyhow::Error> {
        // 1. Intercept SCTE-35 markers first
        if let Ok(Some(segments)) = scte35::parse_scte35(&path) {
            if !segments.is_empty() {
                return Ok(segments);
            }
        }

        // 2. Decode keyframes and audio PCM
        let media_data = demux::decode_media(&path)?;

        // 3. Extract features (CLIP embeddings & YAMNet audio class ratios)
        let block_features = features::extract_features(&media_data, &self.config)?;

        // 4. Run classification using GBDT / ONNX
        let cuts = models::classify_segments(block_features, &self.config)?;

        Ok(cuts)
    }
}
