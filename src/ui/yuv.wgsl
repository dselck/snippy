struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32(1 - i32(in_vertex_index & 1u) * 2);
    let y = f32(1 - i32((in_vertex_index >> 1u) & 1u) * 2);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

@group(0) @binding(0) var t_y: texture_2d<f32>;
@group(0) @binding(1) var t_u: texture_2d<f32>;
@group(0) @binding(2) var t_v: texture_2d<f32>;
@group(0) @binding(3) var s_sampler: sampler;

struct ColorParams {
    y_mult: f32,
    y_offset: f32,
    r_v: f32,
    g_u: f32,
    g_v: f32,
    b_u: f32,
    uv_scale_x: f32,
    uv_scale_y: f32,
}
@group(0) @binding(4) var<uniform> color: ColorParams;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let scaled_uv = vec2<f32>(in.uv.x * color.uv_scale_x, in.uv.y * color.uv_scale_y);
    let y = color.y_mult * (textureSample(t_y, s_sampler, scaled_uv).r - color.y_offset);
    let u = textureSample(t_u, s_sampler, scaled_uv).r - 0.5;
    let v = textureSample(t_v, s_sampler, scaled_uv).r - 0.5;

    let r = y + color.r_v * v;
    let g = y - color.g_u * u - color.g_v * v;
    let b = y + color.b_u * u;

    return vec4<f32>(r, g, b, 1.0);
}

@fragment
fn fs_main_nv12(in: VertexOutput) -> @location(0) vec4<f32> {
    let scaled_uv = vec2<f32>(in.uv.x * color.uv_scale_x, in.uv.y * color.uv_scale_y);
    let y = color.y_mult * (textureSample(t_y, s_sampler, scaled_uv).r - color.y_offset);
    let uv = textureSample(t_u, s_sampler, scaled_uv).rg;
    let u = uv.r - 0.5;
    let v = uv.g - 0.5;

    let r = y + color.r_v * v;
    let g = y - color.g_u * u - color.g_v * v;
    let b = y + color.b_u * u;

    return vec4<f32>(r, g, b, 1.0);
}
