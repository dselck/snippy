# Implementation Plan: In-Line Rust Commercial Detection (`snippy-detect`)

This document outlines the implementation plan for building a native Rust commercial detection library and command-line tool (`snippy-detect`) and integrating it with the Snippy video editing application.

---

## Goal Description
The objective is to replace the need for external tools (like Comskip) by developing a pure-Rust commercial detector that can run as a CLI or be linked directly as a library into the Snippy codebase. The design maximizes both accuracy and efficiency by combining in-band digital metadata cues (SCTE-35) with modern machine learning (CLIP + Gradient-Boosted Decision Trees) on sampled keyframes.

---

## User Review Required

> [!IMPORTANT]
> **1. Audio Fingerprinting Trade-Off (De-prioritized for Phase 1)**
> You noted that audio fingerprinting might not be worth the complexity. 
> * **Decision**: We will defer audio fingerprinting (`chromaprint-next` + `sled` database) to a secondary, opt-in phase. Phase 1 will focus exclusively on **SCTE-35 Parsing** (instant, 100% accurate where available) and **Lightweight Visual/Audio ML Inference** (CLIP ONNX + GBDT). This dramatically reduces initial complexity and database management overhead.

> [!IMPORTANT]
> **2. Inter-process Communication Format**
> We need to decide how `snippy-detect` shares cut decision data with the main Snippy editor.
> * **Decision**: We will support **both** formats:
>   * **JSON (Primary Internal)**: A detailed structure that passes timestamps, frame ranges, confidence scores, and features. Perfect for internal programmatic communication.
>   * **EDL (Edit Decision List, Standard)**: Simple, standard format (`[Start] [End] [Cut/Keep]`) supported by major players (Mplayer, MythTV, VLC) for external compatibility.

> [!IMPORTANT]
> **3. Model Distribution Strategy**
> How the pre-trained ONNX models are distributed and loaded.
> * **Decision**: Models will be **loaded as external resource files at runtime** rather than compiled into the binary. This keeps the executable lightweight (~5-10MB instead of 150MB+) and allows modular model updates without rebuilding the application code.

---

## Proposed Changes

We will implement this in a new workspace crate: `snippy-detect`.

```
snippy/
├── Cargo.toml (Workspace)
├── snippy-editor/ (Existing Editor Crate)
└── snippy-detect/ [NEW]
    ├── Cargo.toml
    ├── src/
    │   ├── main.rs (CLI Interface)
    │   ├── lib.rs (Library API Entry)
    │   ├── demux/ (MPEG-TS/MP4 Decoder using ffmpeg-next)
    │   ├── features/ (Visual CLIP & Audio LUFS Extraction)
    │   ├── scte35/ (SCTE-35 Splice Interceptor)
    │   ├── models/ (ONNX Runtime wrappers)
    │   └── output/ (EDL and JSON formatters)
```

### Module Specifications

#### 1. `demux` (FFmpeg Decoder Integration)
* **API**: Decodes streams utilizing `ffmpeg-next`. For efficiency, it samples only keyframes (I-frames) at a configurable interval (e.g. 1 frame every 2 seconds) instead of decoding every single frame.
* **Audio**: Decodes audio packets into a rolling PCM buffer to calculate perceived loudness.

#### 2. `scte35` (Splice Marker Interceptor)
* **API**: Scans the MPEG-TS container sections. If an SCTE-35 PID is present, it intercepts splice packets and decodes `splice_insert` or `time_signal` markers.
* **Flow**: If SCTE-35 markers are present and complete, it skips visual/audio ML decoding entirely and prints the frame-accurate cutlist instantly.

#### 3. `features` (Metric Extraction)
* **Visual**: Feeds decoded keyframes into a pre-compiled ONNX CLIP model (`ViT-B/32` or `MobileNet-CLIP`) to get a 512-dimensional visual embedding vector.
* **Audio**: Computes momentary/short-term LUFS (EBU R128 standard), Zero Crossing Rate (ZCR), and feeds audio waveforms into a lightweight **YAMNet ONNX model** to extract probabilities for 521 semantic audio classes (such as Speech, Music, and Silence).
* **Aggregation**: Groups these visual embeddings and audio class probabilities into segment blocks bounded by scene cuts (histogram spikes) and audio silent intervals.

#### 4. `models` (LightGBM/ONNX Inference)
* **Visual Encoder**: Wraps the CLIP visual encoder in ONNX format via `ort`.
* **Audio Event Classifier**: Wraps the lightweight YAMNet audio model in ONNX format via `ort` (~3.7MB).
* **Classifier**: Aggregates block features and feeds them into a Gradient-Boosted Decision Tree model (LightGBM) to output a commercial probability score.


---

## API & CLI Design

### 1. Library API (`lib.rs`)

A clean, thread-safe Rust API that can be linked directly into your trimmer application:

```rust
pub struct Detector {
    config: Config,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub classification_threshold: f32, // 0.0 to 1.0
    pub min_commercial_break_duration: f32, // in seconds
    pub padding_start_seconds: f32,
    pub padding_end_seconds: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CutSegment {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub start_frame: usize,
    pub end_frame: usize,
    pub confidence: f32,
}

impl Detector {
    pub fn new(config: Config) -> Self;

    /// Analyzes a video file and returns a list of commercial segments
    pub fn detect<P: AsRef<Path>>(&self, path: P) -> Result<Vec<CutSegment>, Error>;
}
```

### 2. CLI Interface (`main.rs`)

Exposes a standard UNIX-style CLI using `clap`:

```bash
# General usage
snippy-detect input.ts --output cutlist.json --threshold 0.85

# Options
# -i, --input     Path to the video file (supports .ts, .mp4, .mkv)
# -o, --output    Path to save cutlist (detects .json or .edl format)
# -t, --threshold Confidence threshold for classification (default: 0.80)
# -p, --padding   Padding to apply to cuts (default: 0.0s)
```

---

## Verification Plan

### 1. Testing against Labeled Datasets
To verify classification accuracy without needing to capture broadcasts locally:
* **Feature Benchmark Testing**: Write unit tests that load the **UCI TV News Channel Commercial Detection Dataset** feature vectors. Verify that our GBDT model correctly classifies the test partition with a precision and recall exceeding **93%**.
* **Visual/Audio Pipeline Testing**: Download the open-source **Italian TV Commercials Dataset (1980–2024)**. Verify that our CLIP keyframe feature extractor correctly aligns visual cuts and classifies ad blocks across different video source ratios.

### 2. Automated SCTE-35 Parser Validation
* Write regression tests using standard SCTE-35 sample files (MPEG-TS streams).
* Verify that the parser extracts exact PTS time signals and generates matches corresponding to the reference cutlists.

### 3. Efficiency Benchmarks
* Profile the execution time on a 1-hour video file.
* **Target**: CPU-only inference should complete in **less than 60 seconds** (60x real-time speed) due to keyframe sampling and GBDT classification.
