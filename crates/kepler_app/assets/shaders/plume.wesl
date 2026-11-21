import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct PlumeUniforms {
    params: vec4<f32>,
    core_color: vec4<f32>,
    outer_color: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> u: PlumeUniforms;

fn hash21(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    return fract(dot(q, q + 45.32));
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let throttle = clamp(u.params.x, 0.0, 1.0);
    if throttle <= 0.005 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    let uv = in.uv;
    let y = uv.y;
    let x = (uv.x - 0.5) * 2.0;

    let t = u.params.y;
    let n1 = value_noise(vec2<f32>(x * 3.5, y * 7.0 - t * 22.0));
    let n2 = value_noise(vec2<f32>(x * 7.0 + 3.1, y * 14.0 - t * 38.0));
    let turb = (n1 * 0.65 + n2 * 0.35 - 0.5) * 0.35 * (0.2 + y);

    let half_w = mix(0.45, 0.92, pow(y, 0.7)) * (0.55 + 0.45 * throttle);
    let radial = abs(x + turb) / max(half_w, 0.05);

    let shock = pow(max(0.0, cos((y * 12.0 - t * 8.0) * 3.14159)), 6.0) * exp(-y * 2.2);

    let axial_fade = (1.0 - smoothstep(0.15, 1.0, y)) * smoothstep(0.0, 0.04, y);
    let radial_fade = exp(-radial * radial * 2.6);
    let energy = axial_fade * radial_fade * (0.7 + 0.75 * shock) * throttle;

    let col = mix(u.outer_color.rgb, u.core_color.rgb, clamp(radial_fade * 0.8 + shock * 0.6, 0.0, 1.0));
    let hdr_col = col * (energy * (2.6 + u.params.z));
    return vec4<f32>(hdr_col, clamp(energy * 1.4, 0.0, 1.0));
}
