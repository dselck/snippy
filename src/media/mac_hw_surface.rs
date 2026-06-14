#![allow(unexpected_cfgs, clippy::type_complexity, clippy::too_many_arguments)]
use ffmpeg_next as ffmpeg;
use objc2_metal::MTLDevice;

/// Helper function to create a wgpu::Texture from a VideoToolbox hardware frame.
/// 
/// # Safety
/// This function relies on bypassing wgpu's safe abstraction to map an underlying
/// Apple IOSurface directly into a wgpu::Texture. It is highly unsafe because:
/// 1. We assume the wgpu device is using the Metal backend.
/// 2. We assume the AVFrame contains a valid CVPixelBufferRef.
/// 3. The caller must ensure the AVFrame remains alive as long as the wgpu::Texture is used,
///    or manually retain/release the CFRef.
pub unsafe fn create_texture_from_videotoolbox_frame(
    device: &wgpu::Device,
    frame: &ffmpeg::util::frame::Video,
) -> Option<(wgpu::Texture, wgpu::Texture)> {
    unsafe {
        if frame.format() != ffmpeg::format::Pixel::VIDEOTOOLBOX {
            return None;
        }

        let ptr = frame.as_ptr();
        let cv_pixel_buffer = (*ptr).data[3] as core_video_sys::CVPixelBufferRef;
        if cv_pixel_buffer.is_null() {
            return None;
        }

        let iosurface = core_video_sys::CVPixelBufferGetIOSurface(cv_pixel_buffer);
        if iosurface.is_null() {
            return None;
        }

        let cv_format = core_video_sys::CVPixelBufferGetPixelFormatType(cv_pixel_buffer);
        let is_10bit = cv_format == 0x78343230 /* 'x420' */ || cv_format == 0x78663230 /* 'xf20' */;
        let is_8bit = cv_format == 0x34323076 /* '420v' */ || cv_format == 0x34323066 /* '420f' */;
        
        if !is_10bit && !is_8bit {
            return None; // Unsupported pixel format, fail gracefully rather than decoding garbage
        }

        let plane_0_width = core_video_sys::CVPixelBufferGetWidthOfPlane(cv_pixel_buffer, 0) as u64;
        let plane_0_height = core_video_sys::CVPixelBufferGetHeightOfPlane(cv_pixel_buffer, 0) as u64;
        let plane_1_width = core_video_sys::CVPixelBufferGetWidthOfPlane(cv_pixel_buffer, 1) as u64;
        let plane_1_height = core_video_sys::CVPixelBufferGetHeightOfPlane(cv_pixel_buffer, 1) as u64;

        let mtl_format_y = if is_10bit { objc2_metal::MTLPixelFormat::R16Unorm } else { objc2_metal::MTLPixelFormat::R8Unorm };
        let mtl_format_uv = if is_10bit { objc2_metal::MTLPixelFormat::RG16Unorm } else { objc2_metal::MTLPixelFormat::RG8Unorm };

        let wgpu_format_y = if is_10bit { wgpu::TextureFormat::R16Unorm } else { wgpu::TextureFormat::R8Unorm };
        let wgpu_format_uv = if is_10bit { wgpu::TextureFormat::Rg16Unorm } else { wgpu::TextureFormat::Rg8Unorm };

        let hal_device = device.as_hal::<wgpu_hal::api::Metal>()?;
        let mtl_device = hal_device.raw_device();
        
        let iosurface_ref = &*(iosurface as *const objc2_io_surface::IOSurfaceRef);

        // Plane 0: Y plane
        let desc_y = objc2_metal::MTLTextureDescriptor::new();
        desc_y.setPixelFormat(mtl_format_y);
        desc_y.setWidth(plane_0_width as usize);
        desc_y.setHeight(plane_0_height as usize);
        desc_y.setUsage(objc2_metal::MTLTextureUsage::ShaderRead);
        let mtl_texture_y = mtl_device.newTextureWithDescriptor_iosurface_plane(&desc_y, iosurface_ref, 0)?;

        // Plane 1: UV plane (NV12 / P010)
        let desc_uv = objc2_metal::MTLTextureDescriptor::new();
        desc_uv.setPixelFormat(mtl_format_uv);
        desc_uv.setWidth(plane_1_width as usize);
        desc_uv.setHeight(plane_1_height as usize);
        desc_uv.setUsage(objc2_metal::MTLTextureUsage::ShaderRead);
        let mtl_texture_uv = mtl_device.newTextureWithDescriptor_iosurface_plane(&desc_uv, iosurface_ref, 1)?;

        let hal_texture_y = wgpu_hal::metal::Device::texture_from_raw(
            mtl_texture_y,
            wgpu_format_y,
            objc2_metal::MTLTextureType::Type2D,
            1, 1,
            wgpu_hal::CopyExtent { width: plane_0_width as u32, height: plane_0_height as u32, depth: 1 }
        );
        let hal_texture_uv = wgpu_hal::metal::Device::texture_from_raw(
            mtl_texture_uv,
            wgpu_format_uv,
            objc2_metal::MTLTextureType::Type2D,
            1, 1,
            wgpu_hal::CopyExtent { width: plane_1_width as u32, height: plane_1_height as u32, depth: 1 }
        );
        
        let tex_y = device.create_texture_from_hal::<wgpu_hal::api::Metal>(
            hal_texture_y,
            &wgpu::TextureDescriptor {
                label: Some("zero_copy_y_plane"),
                size: wgpu::Extent3d { width: plane_0_width as u32, height: plane_0_height as u32, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu_format_y,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }
        );

        let tex_uv = device.create_texture_from_hal::<wgpu_hal::api::Metal>(
            hal_texture_uv,
            &wgpu::TextureDescriptor {
                label: Some("zero_copy_uv_plane"),
                size: wgpu::Extent3d { width: plane_1_width as u32, height: plane_1_height as u32, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu_format_uv,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }
        );

        Some((tex_y, tex_uv))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_texture_from_videotoolbox() {
        // Only run this test on macOS
        #[cfg(target_os = "macos")]
        {
            ffmpeg::init().unwrap();
            
            // 1. Initialize WGPU Device
            let instance = wgpu::Instance::default();
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("Failed to find wgpu adapter");
            let mut required_features = wgpu::Features::empty();
            if adapter.features().contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM) {
                required_features |= wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
            }
            let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features,
                ..Default::default()
            }))
                .expect("Failed to create wgpu device");

            // 2. Decode a test frame using VideoToolbox
            let test_file = format!("{}/test-files/bbb_sunflower_2160p_60fps_normal.mp4", env!("CARGO_MANIFEST_DIR"));
            let mut input = ffmpeg::format::input(&test_file).expect("Failed to open test file");
            let video_stream_idx = input.streams().best(ffmpeg::media::Type::Video).unwrap().index();
            let mut codec_ctx = ffmpeg::codec::context::Context::from_parameters(
                input.streams().best(ffmpeg::media::Type::Video).unwrap().parameters()
            ).unwrap();
            
            unsafe {
                let mut hw_device_ctx: *mut ffmpeg::sys::AVBufferRef = std::ptr::null_mut();
                let res = ffmpeg::sys::av_hwdevice_ctx_create(
                    &mut hw_device_ctx,
                    ffmpeg::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    0,
                );
                assert_eq!(res, 0, "Failed to create VideoToolbox hardware context");
                (*codec_ctx.as_mut_ptr()).hw_device_ctx = ffmpeg::sys::av_buffer_ref(hw_device_ctx);
                ffmpeg::sys::av_buffer_unref(&mut hw_device_ctx);
            }
            
            let mut decoder = codec_ctx.decoder().video().unwrap();
            for (stream, packet) in input.packets() {
                if stream.index() == video_stream_idx {
                    decoder.send_packet(&packet).unwrap();
                    let mut frame = ffmpeg::util::frame::Video::empty();
                    while decoder.receive_frame(&mut frame).is_ok() {
                        if frame.format() == ffmpeg::format::Pixel::VIDEOTOOLBOX {
                            
                            // 3. Test the surface conversion
                            let textures = unsafe { create_texture_from_videotoolbox_frame(&device, &frame) };
                            
                            assert!(textures.is_some(), "Failed to create wgpu::Textures from VideoToolbox frame");
                            let (tex_y, tex_uv) = textures.unwrap();
                            assert!(tex_y.width() >= frame.width());
                            assert!(tex_y.height() >= frame.height());
                            assert!(tex_uv.width() >= frame.width() / 2);
                            assert!(tex_uv.height() >= frame.height() / 2);
                            
                            return; // Success
                        }
                    }
                }
            }
            
            panic!("Never decoded a hardware frame from test.mp4");
        }
    }
}
