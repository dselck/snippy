//! `snippy-detect extract-features`
//!
//! Runs the full feature-extraction pipeline over a video recording and
//! writes labelled rows to a CSV file, using a ground-truth JSON sidecar
//! produced by Snippy's Label Mode.
//!
//! # Usage
//! ```text
//! snippy-detect extract-features recording.ts recording.ground-truth.json \
//!     [--output training.csv] [--append]
//! ```
//!
//! # CSV schema
//! ```
//! label,cut_frame_offset,duration_s,lufs,zcr,start_s,end_s
//! ```
//! - `label`            : 0=Content 1=Commercial 2=Promo 3=Transition
//! - `cut_frame_offset` : for Transition blocks, the exact cut frame; else empty
//! - `duration_s`       : block duration in seconds
//! - `lufs`             : average loudness (LUFS)
//! - `zcr`              : zero-crossing rate
//! - `start_s`          : block start timestamp
//! - `end_s`            : block end timestamp
//!
//! YAMNet and CLIP columns will be added once ONNX models are wired up.
//! TODO: add yamnet_* and clip_mean / clip_var columns when models are present.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use snippy_schema::{BlockLabel, GroundTruth};

#[derive(Parser, Debug)]
#[command(
    name = "extract-features",
    about = "Extract runtime features from a labelled recording and append to a training CSV"
)]
struct Args {
    /// Path to the video recording (e.g. recording.ts)
    video: PathBuf,

    /// Path to the ground-truth JSON sidecar produced by Snippy Label Mode
    ground_truth: PathBuf,

    /// Output CSV file (default: <video_stem>.training.csv)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Append to an existing CSV instead of overwriting it
    #[arg(short, long)]
    append: bool,
}

fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    // ── Resolve output path ──────────────────────────────────────────────────
    let csv_path = args.output.unwrap_or_else(|| {
        args.video
            .with_extension("training.csv")
    });

    // ── Load ground truth ────────────────────────────────────────────────────
    println!("Loading ground truth from {} …", args.ground_truth.display());
    let gt = GroundTruth::load(&args.ground_truth)
        .with_context(|| format!("Failed to load {}", args.ground_truth.display()))?;

    println!(
        "  {} breaks, fps={:.3}, {} frames",
        gt.commercial_breaks.len(),
        gt.frame_rate,
        gt.duration_frames,
    );

    // ── Decode media ─────────────────────────────────────────────────────────
    println!("Decoding media {} …", args.video.display());
    let media = snippy_detect::demux::decode_media(&args.video)
        .with_context(|| format!("Failed to decode {}", args.video.display()))?;

    // ── Extract features ──────────────────────────────────────────────────────
    let config = snippy_detect::Config {
        classification_threshold: 0.5,
        min_commercial_break_duration: 15.0,
        padding_start_seconds: 0.0,
        padding_end_seconds: 0.0,
        onnx_clip_path: String::new(),
        onnx_yamnet_path: String::new(),
    };

    println!("Extracting features …");
    let blocks = snippy_detect::features::extract_features(&media, &config)
        .context("Feature extraction failed")?;

    println!("  {} blocks extracted", blocks.len());

    // ── Open CSV ─────────────────────────────────────────────────────────────
    let is_new = !csv_path.exists() || !args.append;
    let mut file = OpenOptions::new()
        .create(true)
        .append(args.append)
        .write(!args.append)
        .truncate(!args.append)
        .open(&csv_path)
        .with_context(|| format!("Failed to open {}", csv_path.display()))?;

    // Write header only for new files
    if is_new {
        writeln!(
            file,
            "label,cut_frame_offset,duration_s,lufs,zcr,start_s,end_s"
        )?;
    }

    // ── Label each block and write row ────────────────────────────────────────
    let fps = gt.frame_rate;
    let mut counts = [0usize; 4]; // Content, Commercial, Promo, Transition

    for block in &blocks {
        let duration = block.end_timestamp - block.start_timestamp;
        let start_frame = (block.start_timestamp * fps) as u64;
        let end_frame = (block.end_timestamp * fps) as u64;

        let (label, cut_frame) = gt.label_for_block(start_frame, end_frame);

        let label_int = label as u8;
        let cut_frame_str = cut_frame
            .map(|f| f.to_string())
            .unwrap_or_default();

        writeln!(
            file,
            "{},{},{:.6},{:.4},{:.6},{:.6},{:.6}",
            label_int,
            cut_frame_str,
            duration,
            block.average_lufs,
            block.zcr,
            block.start_timestamp,
            block.end_timestamp,
        )?;

        let idx = label as usize;
        if idx < counts.len() {
            counts[idx] += 1;
        }
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    println!("\n── Feature extraction complete ──");
    println!("  Content    : {}", counts[BlockLabel::Content as usize]);
    println!("  Commercial : {}", counts[BlockLabel::Commercial as usize]);
    println!("  Promo      : {}", counts[BlockLabel::Promo as usize]);
    println!("  Transition : {}", counts[BlockLabel::Transition as usize]);
    println!("  Total rows : {}", counts.iter().sum::<usize>());
    println!("  Saved to   : {}", csv_path.display());

    Ok(())
}
