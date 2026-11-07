use std::path::PathBuf;
use clap::Parser;
use snippy_detect::{Config, Detector};

#[derive(Parser, Debug)]
#[command(author, version, about = "Inline TV commercial detector for Snippy")]
struct Args {
    /// Input video file (MPEG-TS, MP4, MKV)
    #[arg(short, long)]
    input: PathBuf,

    /// Output file path (EDL or JSON format)
    #[arg(short, long)]
    output: PathBuf,

    /// Classification confidence threshold (0.0 to 1.0)
    #[arg(short, long, default_value_t = 0.80)]
    threshold: f32,

    /// Minimum commercial break duration in seconds
    #[arg(long, default_value_t = 15.0)]
    min_duration: f32,

    /// Path to ONNX CLIP model file
    #[arg(long, default_value = "resources/models/clip_vit_b32.onnx")]
    clip_model: String,

    /// Path to ONNX YAMNet model file
    #[arg(long, default_value = "resources/models/yamnet.onnx")]
    yamnet_model: String,
}

fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    let config = Config {
        classification_threshold: args.threshold,
        min_commercial_break_duration: args.min_duration,
        padding_start_seconds: 0.0,
        padding_end_seconds: 0.0,
        onnx_clip_path: args.clip_model,
        onnx_yamnet_path: args.yamnet_model,
    };

    println!("Scanning {} for commercials...", args.input.display());
    let detector = Detector::new(config);
    let cuts = detector.detect(&args.input)?;

    println!("Found {} commercial segments.", cuts.len());
    
    // Save output
    snippy_detect::output::write_output(&args.output, &cuts, &args.input.to_string_lossy())?;
    println!("Saved results to {}", args.output.display());

    Ok(())
}
