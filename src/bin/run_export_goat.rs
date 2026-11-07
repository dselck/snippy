use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use ffmpeg_next as ffmpeg;
use snippy2::media::export::export_video;
use snippy2::media::types::{ExportStatus, VideoMetadata};
use snippy2::ui::ExportRange;

fn scan_metadata(path_str: &str) -> Result<VideoMetadata, anyhow::Error> {
    ffmpeg::init()?;
    let mut input = ffmpeg::format::input(&path_str)?;
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| anyhow::anyhow!("No video stream found"))?;

    let codec_ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
    let decoder = codec_ctx.decoder().video()?;

    let has_audio = input
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .is_some();

    let fps = stream.avg_frame_rate().numerator() as f64
        / stream.avg_frame_rate().denominator() as f64;

    let mut total_frames = stream.frames() as usize;
    if total_frames == 0 {
        let duration_secs = input.duration() as f64 / 1_000_000.0;
        total_frames = (duration_secs * fps).round() as usize;
    }

    let (keyframes, keyframes_pts) = {
        let mut scan = ffmpeg::format::input(&path_str)?;
        let video_stream_idx = stream.index();
        let scan_stream = scan.stream(video_stream_idx).unwrap();
        let scan_time_base = scan_stream.time_base();
        let tb_f64 = scan_time_base.numerator() as f64 / scan_time_base.denominator() as f64;
        let scan_start_time = if scan_stream.start_time() > -1_000_000_000_000 {
            scan_stream.start_time() as f64 * tb_f64
        } else {
            0.0
        };

        let mut kfs = Vec::new();
        let mut kfs_pts = Vec::new();
        for (stream, packet) in scan.packets() {
            if stream.index() == video_stream_idx {
                if packet.is_key() {
                    let pts = packet.pts().unwrap_or(0);
                    let pts_sec = pts as f64 * tb_f64 - scan_start_time;
                    let display_frame_idx = (pts_sec * fps).round() as usize;
                    kfs.push(display_frame_idx);
                    kfs_pts.push(pts);
                }
            }
        }
        if kfs.is_empty() || kfs[0] != 0 {
            kfs.insert(0, 0);
            kfs_pts.insert(0, 0);
        }
        (kfs, kfs_pts)
    };

    Ok(VideoMetadata {
        total_frames,
        width: decoder.width(),
        height: decoder.height(),
        fps,
        has_audio,
        keyframes,
        keyframes_pts,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input_path = "/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv";
    let output_path = "/Users/dselck/Downloads/test_goat_export.mkv";

    // Clean up any previous test_goat_export.mkv
    let _ = std::fs::remove_file(output_path);

    println!("Scanning metadata...");
    let metadata = scan_metadata(input_path)?;
    println!("Metadata scanned: total_frames={}, keyframes={}", metadata.total_frames, metadata.keyframes.len());

    // Print input stream's extradata
    {
        ffmpeg::init()?;
        let input = ffmpeg::format::input(&input_path)?;
        let video_in_stream = input.streams().best(ffmpeg::media::Type::Video).unwrap();
        let extradata = unsafe {
            let raw = video_in_stream.parameters().as_ptr();
            if !(*raw).extradata.is_null() && (*raw).extradata_size > 0 {
                Some(std::slice::from_raw_parts((*raw).extradata, (*raw).extradata_size as usize))
            } else {
                None
            }
        };
        println!("Input video stream extradata: {:?}", extradata.map(|d| d.len()));
        if let Some(d) = extradata {
            println!("First 30 bytes of extradata: {:x?}", &d[..d.len().min(30)]);
            if d.len() >= 23 {
                let num_arrays = d[22] as usize;
                println!("numOfArrays: {}", num_arrays);
                let mut pos = 23;
                for a_idx in 0..num_arrays {
                    if pos + 3 <= d.len() {
                        let nal_type = d[pos] & 0x3F;
                        let num_nalus = u16::from_be_bytes([d[pos+1], d[pos+2]]) as usize;
                        println!("  Array {} type={}: num_nalus={}", a_idx, nal_type, num_nalus);
                        pos += 3;
                        for n_idx in 0..num_nalus {
                            if pos + 2 <= d.len() {
                                let len = u16::from_be_bytes([d[pos], d[pos+1]]) as usize;
                                println!("    NAL {} length={}", n_idx, len);
                                pos += 2;
                                if pos + len <= d.len() {
                                    println!("      Header bytes: {:x?}", &d[pos..pos+len.min(4)]);
                                    pos += len;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let ranges = vec![
        ExportRange { id: 1, in_frame: 15008, out_frame: 16255 },
        ExportRange { id: 2, in_frame: 17127, out_frame: 17784 },
        ExportRange { id: 3, in_frame: 19248, out_frame: 20103 },
        ExportRange { id: 4, in_frame: 21816, out_frame: 22494 },
    ];

    let status = Arc::new(Mutex::new(ExportStatus::Idle));
    let cancel_flag = Arc::new(AtomicBool::new(false));

    println!("Running export...");
    export_video(
        input_path,
        output_path,
        ranges,
        false, // cut_mode = false
        metadata,
        status.clone(),
        cancel_flag,
    )?;

    println!("Export completed successfully!");
    Ok(())
}
