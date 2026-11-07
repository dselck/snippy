use std::fs::File;
use std::io::Write;
use std::path::Path;
use crate::CutSegment;
use snippy_schema::{DetectionOutput, DetectedSegment};

/// Write detection results to `path`.
///
/// * `.edl` extension → Comskip/MPlayer EDL text format
/// * anything else    → versioned JSON (`DetectionOutput` envelope)
///
/// `video_file` is embedded in the JSON envelope for traceability; pass an
/// empty string if the caller does not have the path available.
pub fn write_output<P: AsRef<Path>>(
    path: P,
    cuts: &[CutSegment],
    video_file: &str,
) -> Result<(), anyhow::Error> {
    let path_ref = path.as_ref();
    let extension = path_ref
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    if extension == "edl" {
        let mut file = File::create(path_ref)?;
        for cut in cuts {
            // EDL format: [start_seconds] [end_seconds] [action]
            // where 0 means "Cut/Skip"
            writeln!(file, "{:.3}\t{:.3}\t0", cut.start_seconds, cut.end_seconds)?;
        }
    } else {
        // Default to versioned JSON envelope
        let file = File::create(path_ref)?;
        let output = DetectionOutput {
            schema_version: 1,
            video_file: video_file.to_string(),
            snippy_detect_version: env!("CARGO_PKG_VERSION").to_string(),
            segments: cuts
                .iter()
                .map(|c| DetectedSegment {
                    start_seconds: c.start_seconds,
                    end_seconds: c.end_seconds,
                    start_frame: c.start_frame,
                    end_frame: c.end_frame,
                    confidence: c.confidence,
                })
                .collect(),
        };
        serde_json::to_writer_pretty(file, &output)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_write_edl() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_cutlist.edl");

        let cuts = vec![CutSegment {
            start_seconds: 1.25,
            end_seconds: 15.5,
            start_frame: 37,
            end_frame: 465,
            confidence: 0.95,
        }];

        write_output(&file_path, &cuts, "").unwrap();

        let mut content = String::new();
        File::open(&file_path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();

        assert_eq!(content.trim(), "1.250\t15.500\t0");

        let _ = std::fs::remove_file(file_path);
    }

    #[test]
    fn test_write_json() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_cutlist.json");

        let cuts = vec![CutSegment {
            start_seconds: 1.25,
            end_seconds: 15.5,
            start_frame: 37,
            end_frame: 465,
            confidence: 0.95,
        }];

        write_output(&file_path, &cuts, "test_video.ts").unwrap();

        let mut content = String::new();
        File::open(&file_path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();

        let parsed: DetectionOutput = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.segments.len(), 1);
        assert_eq!(parsed.segments[0].start_seconds, 1.25);
        assert_eq!(parsed.segments[0].end_seconds, 15.5);

        let _ = std::fs::remove_file(file_path);
    }
}
