use eframe::wgpu;
use crate::media::YuvPlanes;

pub struct YuvRenderer {
    pub pipeline: wgpu::RenderPipeline,
    pub nv12_pipeline: wgpu::RenderPipeline,
    pub sampler: wgpu::Sampler,
    // holding y, u, v textures
    pub y_texture: Option<wgpu::Texture>,
    pub u_texture: Option<wgpu::Texture>,
    pub v_texture: Option<wgpu::Texture>,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub bind_group: Option<wgpu::BindGroup>,
    pub color_buffer: wgpu::Buffer,
    pub is_nv12: bool,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ColorUniforms {
    pub y_mult: f32,
    pub y_offset: f32,
    pub r_v: f32,
    pub g_u: f32,
    pub g_v: f32,
    pub b_u: f32,
    pub uv_scale_x: f32,
    pub uv_scale_y: f32,
}

impl ColorUniforms {
    pub fn new(space: ffmpeg_next::color::Space, range: ffmpeg_next::color::Range, uv_scale_x: f32, uv_scale_y: f32) -> Self {
        let is_full = matches!(range, ffmpeg_next::color::Range::JPEG);
        let y_mult = if is_full { 1.0 } else { 1.164383 };
        let y_offset = if is_full { 0.0 } else { 0.062745 };
        
        let r_v;
        let g_u;
        let g_v;
        let b_u;

        match space {
            ffmpeg_next::color::Space::BT470BG | ffmpeg_next::color::Space::SMPTE170M | ffmpeg_next::color::Space::SMPTE240M => {
                // BT.601
                if is_full {
                    r_v = 1.402;
                    g_u = 0.344136;
                    g_v = 0.714136;
                    b_u = 1.772;
                } else {
                    r_v = 1.596027;
                    g_u = 0.391762;
                    g_v = 0.812968;
                    b_u = 2.017232;
                }
            }
            ffmpeg_next::color::Space::BT2020NCL | ffmpeg_next::color::Space::BT2020CL => {
                // BT.2020
                if is_full {
                    r_v = 1.4746;
                    g_u = 0.1645;
                    g_v = 0.5713;
                    b_u = 1.8814;
                } else {
                    r_v = 1.6786;
                    g_u = 0.1873;
                    g_v = 0.6504;
                    b_u = 2.1417;
                }
            }
            _ => {
                // BT.709
                if is_full {
                    r_v = 1.5748;
                    g_u = 0.1873;
                    g_v = 0.4681;
                    b_u = 1.8556;
                } else {
                    r_v = 1.7927;
                    g_u = 0.2132;
                    g_v = 0.5329;
                    b_u = 2.1124;
                }
            }
        }
        
        Self {
            y_mult,
            y_offset,
            r_v,
            g_u,
            g_v,
            b_u,
            uv_scale_x,
            uv_scale_y,
        }
    }
}

impl YuvRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("yuv.wgsl"));

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yuv_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<ColorUniforms>() as u64),
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yuv_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yuv_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let nv12_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nv12_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main_nv12"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let color_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("color_uniform_buffer"),
            size: std::mem::size_of::<ColorUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            nv12_pipeline,
            sampler,
            y_texture: None,
            u_texture: None,
            v_texture: None,
            bind_group_layout,
            bind_group: None,
            color_buffer,
            is_nv12: false,
        }
    }

    pub fn update_textures_from_hw_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hw_frame: std::sync::Arc<crate::media::types::HwFrameWrapper>,
        color_space: ffmpeg_next::color::Space,
        color_range: ffmpeg_next::color::Range,
    ) {
        #[cfg(target_os = "macos")]
        {
            let textures = unsafe { crate::media::mac_hw_surface::create_texture_from_videotoolbox_frame(device, &hw_frame.frame) };
            if let Some((tex_y, tex_uv)) = textures {
                self.is_nv12 = true;

                let y_view = tex_y.create_view(&wgpu::TextureViewDescriptor::default());
                let uv_view = tex_uv.create_view(&wgpu::TextureViewDescriptor::default());

                self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("nv12_bind_group"),
                    layout: &self.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&y_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&uv_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&uv_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: self.color_buffer.as_entire_binding(),
                        },
                    ],
                }));

                let uv_scale_x = hw_frame.frame.width() as f32 / tex_y.width() as f32;
                let uv_scale_y = hw_frame.frame.height() as f32 / tex_y.height() as f32;

                self.y_texture = Some(tex_y);
                self.u_texture = Some(tex_uv);
                self.v_texture = None;
                
                let uniforms = ColorUniforms::new(color_space, color_range, uv_scale_x, uv_scale_y);
                queue.write_buffer(&self.color_buffer, 0, bytemuck::cast_slice(&[uniforms]));
                
                queue.on_submitted_work_done(move || {
                    drop(hw_frame);
                });
            }
        }

        #[cfg(target_os = "linux")]
        {
            self.update_textures_from_linux_hw_frame(device, queue, &hw_frame.frame, color_space, color_range);
        }
    }

    #[cfg(target_os = "linux")]
    pub fn update_textures_from_linux_hw_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &ffmpeg_next::util::frame::Video,
        color_space: ffmpeg_next::color::Space,
        color_range: ffmpeg_next::color::Range,
    ) {
        let width = frame.width();
        let height = frame.height();
        let uv_width = width / 2;
        let uv_height = height / 2;

        let is_10bit = frame.format() == ffmpeg_next::format::Pixel::P010LE
            || frame.format() == ffmpeg_next::format::Pixel::P010BE;
        let (y_format, uv_format) = if is_10bit {
            (wgpu::TextureFormat::R16Unorm, wgpu::TextureFormat::Rg16Unorm)
        } else {
            (wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm)
        };

        let needs_recreate = match (&self.y_texture, &self.u_texture) {
            (Some(y), Some(uv)) => {
                y.width() != width
                    || y.height() != height
                    || y.format() != y_format
                    || uv.width() != uv_width
                    || uv.height() != uv_height
                    || uv.format() != uv_format
                    || !self.is_nv12
            }
            _ => true,
        };

        if needs_recreate {
            let tex_y = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("linux_hw_y_texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: y_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            let tex_uv = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("linux_hw_uv_texture"),
                size: wgpu::Extent3d {
                    width: uv_width,
                    height: uv_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: uv_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            let y_view = tex_y.create_view(&wgpu::TextureViewDescriptor::default());
            let uv_view = tex_uv.create_view(&wgpu::TextureViewDescriptor::default());

            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("linux_nv12_bind_group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&y_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&uv_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&uv_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: self.color_buffer.as_entire_binding(),
                    },
                ],
            }));

            self.y_texture = Some(tex_y);
            self.u_texture = Some(tex_uv);
            self.v_texture = None;
            self.is_nv12 = true;
        }

        let uniforms = ColorUniforms::new(color_space, color_range, 1.0, 1.0);
        queue.write_buffer(&self.color_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        // Upload Plane 0: Y
        if let Some(tex) = &self.y_texture {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                frame.data(0),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.stride(0) as u32),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }

        // Upload Plane 1: UV interleaved
        if let Some(tex) = &self.u_texture {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                frame.data(1),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.stride(1) as u32),
                    rows_per_image: Some(uv_height),
                },
                wgpu::Extent3d {
                    width: uv_width,
                    height: uv_height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    pub fn update_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        planes: &YuvPlanes,
        color_space: ffmpeg_next::color::Space,
        color_range: ffmpeg_next::color::Range,
    ) {
        let was_nv12 = self.is_nv12;
        self.is_nv12 = false;
        let y_width = planes.width;
        let y_height = planes.height;
        let uv_width = y_width / 2;
        let uv_height = y_height / 2;

        let needs_recreate = match (&self.y_texture, &self.v_texture) {
            (Some(tex), Some(_)) => {
                tex.width() != y_width
                    || tex.height() != y_height
                    || tex.format() != wgpu::TextureFormat::R8Unorm
                    || was_nv12
            }
            _ => true,
        };

        if needs_recreate {
            self.y_texture = Some(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("y_texture"),
                size: wgpu::Extent3d {
                    width: y_width,
                    height: y_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }));

            self.u_texture = Some(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("u_texture"),
                size: wgpu::Extent3d {
                    width: uv_width,
                    height: uv_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }));

            self.v_texture = Some(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("v_texture"),
                size: wgpu::Extent3d {
                    width: uv_width,
                    height: uv_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }));

            let y_view = self.y_texture.as_ref().unwrap().create_view(&wgpu::TextureViewDescriptor::default());
            let u_view = self.u_texture.as_ref().unwrap().create_view(&wgpu::TextureViewDescriptor::default());
            let v_view = self.v_texture.as_ref().unwrap().create_view(&wgpu::TextureViewDescriptor::default());

            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yuv_bind_group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&y_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&u_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&v_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: self.color_buffer.as_entire_binding(),
                    },
                ],
            }));
        }

        let uniforms = ColorUniforms::new(color_space, color_range, 1.0, 1.0);
        queue.write_buffer(&self.color_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        // Upload Y plane
        if let Some(tex) = &self.y_texture {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &planes.y,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(planes.y_stride),
                    rows_per_image: Some(y_height),
                },
                wgpu::Extent3d {
                    width: y_width,
                    height: y_height,
                    depth_or_array_layers: 1,
                },
            );
        }

        // Upload U plane
        if let Some(tex) = &self.u_texture {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &planes.u,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(planes.u_stride),
                    rows_per_image: Some(uv_height),
                },
                wgpu::Extent3d {
                    width: uv_width,
                    height: uv_height,
                    depth_or_array_layers: 1,
                },
            );
        }

        // Upload V plane
        if let Some(tex) = &self.v_texture {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &planes.v,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(planes.v_stride),
                    rows_per_image: Some(uv_height),
                },
                wgpu::Extent3d {
                    width: uv_width,
                    height: uv_height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    pub fn render<'rp>(&self, render_pass: &mut wgpu::RenderPass<'rp>) {
        if let Some(bind_group) = &self.bind_group {
            if self.is_nv12 {
                render_pass.set_pipeline(&self.nv12_pipeline);
            } else {
                render_pass.set_pipeline(&self.pipeline);
            }
            render_pass.set_bind_group(0, bind_group, &[]);
            render_pass.draw(0..4, 0..1);
        }
    }

    pub fn video_size(&self) -> Option<(u32, u32)> {
        self.y_texture.as_ref().map(|t| (t.width(), t.height()))
    }
}
