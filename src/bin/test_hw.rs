fn main() {
    ffmpeg_next::init().unwrap();
    let file = "/Users/dselck/Documents/Snippy2/test.mp4";
    let mut input = ffmpeg_next::format::input(&file).unwrap();
    let video_stream_idx = input.streams().best(ffmpeg_next::media::Type::Video).unwrap().index();
    let mut codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(
        input.streams().best(ffmpeg_next::media::Type::Video).unwrap().parameters()
    ).unwrap();
    unsafe {
        let mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
        let res = ffmpeg_next::sys::av_hwdevice_ctx_create(
            &mut hw_device_ctx,
            ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
        );
        if res == 0 && !hw_device_ctx.is_null() {
            (*codec_ctx.as_mut_ptr()).hw_device_ctx = ffmpeg_next::sys::av_buffer_ref(hw_device_ctx);
            ffmpeg_next::sys::av_buffer_unref(&mut hw_device_ctx);
            println!("Attached Videotoolbox HW Context");
        }
    }
    
    let mut decoder = codec_ctx.decoder().video().unwrap();
    let mut count = 0;
    for (stream, packet) in input.packets() {
        if stream.index() == video_stream_idx {
            decoder.send_packet(&packet).unwrap();
            let mut frame = ffmpeg_next::util::frame::Video::empty();
            while decoder.receive_frame(&mut frame).is_ok() {
                println!("Decoded frame format: {:?}", frame.format());
                if frame.format() == ffmpeg_next::format::Pixel::VIDEOTOOLBOX {
                    unsafe {
                        let ptr = frame.as_ptr();
                        println!("Data 3: {:?}", (*ptr).data[3]);
                        println!("Is CVPixelBuffer null? {}", (*ptr).data[3].is_null());
                    }
                }
                count += 1;
                if count > 5 { break; }
            }
        }
        if count > 5 { break; }
    }
}
