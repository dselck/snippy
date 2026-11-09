use std::ffi::CString;

#[derive(Debug, Clone)]
pub struct HwDeviceInfo {
    pub backend_name: &'static str,
    pub hw_type: ffmpeg_next::sys::AVHWDeviceType,
    pub hw_pix_fmt: ffmpeg_next::sys::AVPixelFormat,
    pub device_path: Option<String>,
}

/// Checks if a pixel format corresponds to a hardware surface format.
pub fn is_hw_pixel_format(fmt: ffmpeg_next::format::Pixel) -> bool {
    matches!(
        fmt,
        ffmpeg_next::format::Pixel::VAAPI
            | ffmpeg_next::format::Pixel::CUDA
            | ffmpeg_next::format::Pixel::QSV
            | ffmpeg_next::format::Pixel::VULKAN
            | ffmpeg_next::format::Pixel::VDPAU
            | ffmpeg_next::format::Pixel::VIDEOTOOLBOX
            | ffmpeg_next::format::Pixel::D3D11
    )
}

/// Universal callback passed to `libavcodec` to select the hardware pixel format
/// corresponding to the active `hw_device_ctx`.
///
/// If the hardware device cannot decode the specific stream format (e.g. an unsupported
/// bit-depth profile or codec variant), this automatically falls back to `avcodec_default_get_format`,
/// ensuring seamless software decode without errors.
pub unsafe extern "C" fn generic_get_hw_format(
    ctx: *mut ffmpeg_next::sys::AVCodecContext,
    pix_fmts: *const ffmpeg_next::sys::AVPixelFormat,
) -> ffmpeg_next::sys::AVPixelFormat {
    unsafe {
        if pix_fmts.is_null() {
            return ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_NONE;
        }
        if ctx.is_null() {
            return if *pix_fmts == ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_NONE {
                ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_NONE
            } else {
                *pix_fmts
            };
        }
        if (*ctx).hw_device_ctx.is_null() {
            return ffmpeg_next::sys::avcodec_default_get_format(ctx, pix_fmts);
        }

        let hw_device_ctx = (*ctx).hw_device_ctx as *mut ffmpeg_next::sys::AVBufferRef;
        if hw_device_ctx.is_null() || (*hw_device_ctx).data.is_null() {
            return ffmpeg_next::sys::avcodec_default_get_format(ctx, pix_fmts);
        }

        let hw_device_data = (*hw_device_ctx).data as *mut ffmpeg_next::sys::AVHWDeviceContext;
        let hw_type = (*hw_device_data).type_;

        let target_pix_fmt = match hw_type {
            ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI => {
                ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_VAAPI
            }
            ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA => {
                ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_CUDA
            }
            ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_QSV => {
                ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_QSV
            }
            ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VULKAN => {
                ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_VULKAN
            }
            ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VDPAU => {
                ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_VDPAU
            }
            ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX => {
                ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX
            }
            ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA => {
                ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_D3D11
            }
            _ => ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_NONE,
        };

        let mut p = pix_fmts;
        while *p != ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_NONE {
            if *p == target_pix_fmt {
                return *p;
            }
            p = p.add(1);
        }

        // Escape hatch: hardware surface not acceptable for this stream, use software format.
        ffmpeg_next::sys::avcodec_default_get_format(ctx, pix_fmts)
    }
}

unsafe fn attach_hw_device_ctx(
    codec_ctx: &mut ffmpeg_next::codec::context::Context,
    mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef,
) { unsafe {
    let ptr = codec_ctx.as_mut_ptr();
    if !(*ptr).hw_device_ctx.is_null() {
        ffmpeg_next::sys::av_buffer_unref(&mut (*ptr).hw_device_ctx);
    }
    (*ptr).hw_device_ctx = ffmpeg_next::sys::av_buffer_ref(hw_device_ctx);
    (*ptr).get_format = Some(generic_get_hw_format);
    ffmpeg_next::sys::av_buffer_unref(&mut hw_device_ctx);
}}

/// Discovers and attaches the best available hardware device context to `codec_ctx`.
///
/// On Linux, it probes in prioritized order:
/// 1. VA-API (via default display session, then scanning `/dev/dri/renderD*`)
/// 2. Intel QuickSync (QSV)
/// 3. NVIDIA CUDA / NVDEC
/// 4. Vulkan Video
/// 5. VDPAU
///
/// On macOS, it initializes VideoToolbox.
/// On Windows, it initializes D3D11VA (with QSV fallback).
///
/// Returns `Some(HwDeviceInfo)` if a hardware context was successfully initialized,
/// or `None` if all backends failed (triggering the software escape hatch).
pub fn probe_and_attach_hw_context(
    codec_ctx: &mut ffmpeg_next::codec::context::Context,
) -> Option<HwDeviceInfo> {
    #[cfg(target_os = "macos")]
    {
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
                attach_hw_device_ctx(codec_ctx, hw_device_ctx);
                return Some(HwDeviceInfo {
                    backend_name: "VideoToolbox",
                    hw_type: ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
                    hw_pix_fmt: ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX,
                    device_path: None,
                });
            }
        }
        None
    }

    #[cfg(target_os = "windows")]
    {
        unsafe {
            let mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
            let res = ffmpeg_next::sys::av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            );
            if res == 0 && !hw_device_ctx.is_null() {
                attach_hw_device_ctx(codec_ctx, hw_device_ctx);
                return Some(HwDeviceInfo {
                    backend_name: "D3D11VA",
                    hw_type: ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
                    hw_pix_fmt: ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_D3D11,
                    device_path: None,
                });
            }

            // Fallback to QSV on Windows
            let mut qsv_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
            let res = ffmpeg_next::sys::av_hwdevice_ctx_create(
                &mut qsv_device_ctx,
                ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_QSV,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            );
            if res == 0 && !qsv_device_ctx.is_null() {
                attach_hw_device_ctx(codec_ctx, qsv_device_ctx);
                return Some(HwDeviceInfo {
                    backend_name: "Intel QuickSync (QSV)",
                    hw_type: ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_QSV,
                    hw_pix_fmt: ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_QSV,
                    device_path: None,
                });
            }
        }
        None
    }

    #[cfg(target_os = "linux")]
    {
        // ── Tier 1: VA-API ───────────────────────────────────────────────────────
        // First try default display context (Wayland / X11 connection)
        unsafe {
            let mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
            let res = ffmpeg_next::sys::av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            );
            if res == 0 && !hw_device_ctx.is_null() {
                attach_hw_device_ctx(codec_ctx, hw_device_ctx);
                return Some(HwDeviceInfo {
                    backend_name: "VA-API (Default Display)",
                    hw_type: ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                    hw_pix_fmt: ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_VAAPI,
                    device_path: None,
                });
            }
        }

        // If default display context failed, scan /dev/dri/renderD* device nodes
        if let Ok(entries) = std::fs::read_dir("/dev/dri") {
            let mut render_nodes = Vec::new();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("renderD") {
                    render_nodes.push(entry.path());
                }
            }
            render_nodes.sort();

            for node in render_nodes {
                let node_str = node.to_string_lossy().to_string();
                if let Ok(c_path) = CString::new(node_str.clone()) {
                    unsafe {
                        let mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
                        let res = ffmpeg_next::sys::av_hwdevice_ctx_create(
                            &mut hw_device_ctx,
                            ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                            c_path.as_ptr(),
                            std::ptr::null_mut(),
                            0,
                        );
                        if res == 0 && !hw_device_ctx.is_null() {
                            attach_hw_device_ctx(codec_ctx, hw_device_ctx);
                            return Some(HwDeviceInfo {
                                backend_name: "VA-API",
                                hw_type: ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                                hw_pix_fmt: ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_VAAPI,
                                device_path: Some(node_str),
                            });
                        }
                    }
                }
            }
        }

        // ── Tier 2: Intel QuickSync (QSV) ────────────────────────────────────────
        unsafe {
            let mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
            let res = ffmpeg_next::sys::av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_QSV,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            );
            if res == 0 && !hw_device_ctx.is_null() {
                attach_hw_device_ctx(codec_ctx, hw_device_ctx);
                return Some(HwDeviceInfo {
                    backend_name: "Intel QuickSync (QSV)",
                    hw_type: ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_QSV,
                    hw_pix_fmt: ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_QSV,
                    device_path: None,
                });
            }
        }

        // ── Tier 3: NVIDIA CUDA / NVDEC ──────────────────────────────────────────
        unsafe {
            let mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
            let res = ffmpeg_next::sys::av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            );
            if res == 0 && !hw_device_ctx.is_null() {
                attach_hw_device_ctx(codec_ctx, hw_device_ctx);
                return Some(HwDeviceInfo {
                    backend_name: "CUDA (NVIDIA)",
                    hw_type: ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
                    hw_pix_fmt: ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_CUDA,
                    device_path: None,
                });
            }
        }

        // ── Tier 4: Vulkan Video ─────────────────────────────────────────────────
        unsafe {
            let mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
            let res = ffmpeg_next::sys::av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VULKAN,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            );
            if res == 0 && !hw_device_ctx.is_null() {
                attach_hw_device_ctx(codec_ctx, hw_device_ctx);
                return Some(HwDeviceInfo {
                    backend_name: "Vulkan Video",
                    hw_type: ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VULKAN,
                    hw_pix_fmt: ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_VULKAN,
                    device_path: None,
                });
            }
        }

        // ── Tier 5: VDPAU ────────────────────────────────────────────────────────
        unsafe {
            let mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
            let res = ffmpeg_next::sys::av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VDPAU,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            );
            if res == 0 && !hw_device_ctx.is_null() {
                attach_hw_device_ctx(codec_ctx, hw_device_ctx);
                return Some(HwDeviceInfo {
                    backend_name: "VDPAU",
                    hw_type: ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VDPAU,
                    hw_pix_fmt: ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_VDPAU,
                    device_path: None,
                });
            }
        }

        // Tier 6: All hardware backends unavailable, fall back to CPU software decode
        None
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_hw_pixel_format() {
        assert!(is_hw_pixel_format(ffmpeg_next::format::Pixel::VAAPI));
        assert!(is_hw_pixel_format(ffmpeg_next::format::Pixel::CUDA));
        assert!(is_hw_pixel_format(ffmpeg_next::format::Pixel::QSV));
        assert!(is_hw_pixel_format(ffmpeg_next::format::Pixel::VULKAN));
        assert!(is_hw_pixel_format(ffmpeg_next::format::Pixel::VDPAU));
        assert!(is_hw_pixel_format(ffmpeg_next::format::Pixel::VIDEOTOOLBOX));
        assert!(is_hw_pixel_format(ffmpeg_next::format::Pixel::D3D11));

        assert!(!is_hw_pixel_format(ffmpeg_next::format::Pixel::YUV420P));
        assert!(!is_hw_pixel_format(ffmpeg_next::format::Pixel::NV12));
        assert!(!is_hw_pixel_format(ffmpeg_next::format::Pixel::P010LE));
        assert!(!is_hw_pixel_format(ffmpeg_next::format::Pixel::P010BE));
        assert!(!is_hw_pixel_format(ffmpeg_next::format::Pixel::RGB24));
        assert!(!is_hw_pixel_format(ffmpeg_next::format::Pixel::RGBA));
    }

    #[test]
    fn test_probe_and_attach_hw_context() {
        let _ = ffmpeg_next::init();
        if let Ok(input) = ffmpeg_next::format::input(&std::path::Path::new("/tmp/test_h264.mp4")) {
            if let Some(stream) = input.streams().best(ffmpeg_next::media::Type::Video) {
                if let Ok(mut ctx) = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters()) {
                    let info = probe_and_attach_hw_context(&mut ctx);
                    #[cfg(target_os = "linux")]
                    {
                        if let Some(info) = info {
                            println!(
                                "Successfully probed HW backend: {} (path: {:?})",
                                info.backend_name, info.device_path
                            );
                            unsafe {
                                let ptr = ctx.as_mut_ptr();
                                assert!(!(*ptr).hw_device_ctx.is_null(), "hw_device_ctx must be non-null");
                                assert!((*ptr).get_format.is_some(), "get_format callback must be set");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_generic_get_hw_format_fallback() {
        let _ = ffmpeg_next::init();
        let fmts = [
            ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_YUV420P,
            ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_NONE,
        ];
        unsafe {
            // Null context should safely fall back to default get_format
            let res = generic_get_hw_format(std::ptr::null_mut(), fmts.as_ptr());
            assert_eq!(res, ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_YUV420P);

            // Null pix_fmts should safely return NONE without crashing
            let res_null = generic_get_hw_format(std::ptr::null_mut(), std::ptr::null());
            assert_eq!(res_null, ffmpeg_next::sys::AVPixelFormat::AV_PIX_FMT_NONE);
        }
    }
}
