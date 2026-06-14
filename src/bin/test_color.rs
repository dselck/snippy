fn main() {
    ffmpeg_next::init().unwrap();
    let mut input = ffmpeg_next::format::input("/Users/dselck/Documents/Snippy2/test-files/bbb_sunflower_2160p_60fps_normal.mp4").unwrap();
    let video_stream_idx = input.streams().best(ffmpeg_next::media::Type::Video).unwrap().index();
    let codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(input.streams().best(ffmpeg_next::media::Type::Video).unwrap().parameters()).unwrap();
    let mut decoder = codec_ctx.decoder().video().unwrap();
    for (stream, packet) in input.packets() {
        if stream.index() == video_stream_idx {
            decoder.send_packet(&packet).unwrap();
            let mut frame = ffmpeg_next::util::frame::Video::empty();
            if decoder.receive_frame(&mut frame).is_ok() {
                println!("Color space: {:?}", frame.color_space());
                println!("Color range: {:?}", frame.color_range());
                println!("Format: {:?}", frame.format());
                break;
            }
        }
    }
}
