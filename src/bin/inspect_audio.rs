use ffmpeg_next as ffmpeg;

fn main() -> Result<(), anyhow::Error> {
    ffmpeg::init()?;
    let path = snippy2::bench::TestMediaLocator::resolve()?;
    let mut input = ffmpeg::format::input(&path)?;

    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .ok_or_else(|| anyhow::anyhow!("No audio stream found"))?;
    let audio_stream_idx = stream.index();
    let time_base = stream.time_base();
    let tb_f64 = time_base.numerator() as f64 / time_base.denominator() as f64;

    let codec_ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = codec_ctx.decoder().audio()?;
    
    println!("Stream start_time: {:?}", stream.start_time());

    // Seek to 4.0 seconds
    let seek_time = 4.0;
    let seek_pts = (seek_time * 1_000_000.0) as i64;
    println!("Seeking to seek_pts: {}", seek_pts);
    input.seek(seek_pts, ..seek_pts + 1)?;
    decoder.flush();

    println!("Demuxing first 10 audio packets after seek:");
    let mut packet_count = 0;
    let mut frame_count = 0;
    for (stream, packet) in input.packets() {
        if stream.index() == audio_stream_idx {
            packet_count += 1;
            println!("  Packet {}: pts={:?}, dts={:?}, duration={}", 
                     packet_count, packet.pts(), packet.dts(), packet.duration());
            
            if decoder.send_packet(&packet).is_ok() {
                let mut frame = ffmpeg::util::frame::Audio::empty();
                while decoder.receive_frame(&mut frame).is_ok() {
                    frame_count += 1;
                    let frame_pts = frame.pts();
                    let pts_sec = frame_pts.map(|pts| pts as f64 * tb_f64);
                    println!("    Frame {}: pts={:?} ({:?} sec), samples={}, rate={}", 
                             frame_count, frame_pts, pts_sec, frame.samples(), frame.rate());
                }
            }
            if packet_count >= 10 {
                break;
            }
        }
    }

    Ok(())
}
