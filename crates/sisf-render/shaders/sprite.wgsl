// SISF 批量 quad 管线。
// group(0) = 每帧：viewport 投影栈（8 层）+ 时间
// group(1) = 每 batch：纹理 + 采样器
// 顶点缓冲 0 = 单位 quad，顶点缓冲 1 = 每实例 80B stride（见 src/vertex.rs）

struct FrameU {
    proj: array<mat4x4<f32>, 8>,
    time: f32,
};

@group(0) @binding(0)
var<uniform> frame: FrameU;

@group(1) @binding(0)
var tex: texture_2d<f32>;
@group(1) @binding(1)
var tex_sampler: sampler;

struct VsIn {
    @location(0) corner: vec2<f32>,
    @location(1) pos: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) uv0: vec2<f32>,
    @location(4) uv1: vec2<f32>,
    @location(5) color: vec4<f32>,
    @location(6) rot: f32,
    @location(7) tone: vec4<f32>,
    @location(8) layer: f32,
    @location(9) mask: f32,
};

struct FsIn {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) tone: vec4<f32>,
    @location(3) mask: f32,
};

// Rgba/tone 都按 sRGB 数值域书写，而 sRGB 格式的渲染目标要求着色器输出线性值，
// 所以色值必须与 Renderer::to_color（清屏色）走同一条换算，否则同一画面里两者不一致。
fn s2l(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@vertex
fn vs_main(i: VsIn) -> FsIn {
    var o: FsIn;
    // size 的符号即翻转：负宽 = 左右翻，负高 = 上下翻。与"交换 uv0/uv1"等价，
    // 但翻转是逐分量的，可以组合（同时为负 = 180° 旋转，结果与几何一致）。
    let mirror = select(vec2<f32>(1.0), vec2<f32>(-1.0), i.size < vec2<f32>(0.0));
    let half = (i.corner - vec2<f32>(0.5)) * abs(i.size);
    let c = cos(i.rot);
    let s = sin(i.rot);
    let rotated = vec2<f32>(c * half.x - s * half.y, s * half.x + c * half.y);
    let li = u32(clamp(i.layer, 0.0, 7.0));
    o.clip = frame.proj[li] * vec4<f32>(i.pos + rotated * mirror, 0.0, 1.0);
    o.uv = mix(i.uv0, i.uv1, i.corner);
    o.color = i.color;
    o.tone = i.tone;
    o.mask = i.mask;
    return o;
}

@fragment
fn fs_main(i: FsIn) -> @location(0) vec4<f32> {
    // 纹理按解码时预乘（T4 保证），tint 的 rgb 不带 alpha，故 alpha 单独乘一次。
    // mask>0.5：R8Unorm 覆盖率图集（T6 字形）。覆盖率同时当 alpha 和 rgb 的系数——
    // 输出必须仍是**预乘**的，写成 `vec4(1,1,1,c)` 会让 rgb 与 alpha 脱钩，
    // 半覆盖的像素按 over 混合后直接饱和成实心方块（golden 里一眼可见）。
    var sampled = textureSample(tex, tex_sampler, i.uv);
    if (i.mask > 0.5) {
        let c = sampled.r;
        sampled = vec4<f32>(c, c, c, c);
    }
    let tint = vec3<f32>(s2l(i.color.r), s2l(i.color.g), s2l(i.color.b));
    let alpha = sampled.a * i.color.a;
    var rgb = sampled.rgb * tint * i.color.a;
    // tone 偏移随 alpha 缩放，保持"输出仍为预乘"；否则半透明像素会被染色。
    rgb = rgb + i.tone.rgb * alpha;
    let lum = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    rgb = mix(rgb, vec3<f32>(lum), vec3<f32>(i.tone.a));
    return vec4<f32>(rgb, alpha);
}
