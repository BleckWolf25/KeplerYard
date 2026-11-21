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
    let pressure = clamp(u.params.z, 0.0, 1.0);
    let seed = u.params.w * 7.13;

    let n1 = value_noise(vec2<f32>(x * 3.5 + seed, y * 7.0 - t * 22.0));
    let n2 = value_noise(vec2<f32>(x * 7.0 + 3.1 - seed, y * 14.0 - t * 38.0));
    let turb = (n1 * 0.65 + n2 * 0.35 - 0.5) * (0.28 + 0.16 * (1.0 - pressure)) * (0.2 + y);

    let vac_flare = mix(1.85, 0.85, pressure);
    let half_w = mix(0.42, 0.90 * vac_flare, pow(y, 0.72)) * (0.55 + 0.45 * throttle);
    let radial = abs(x + turb) / max(half_w, 0.05);

    let shock_amp = mix(0.08, 0.90, pressure);
    let shock = pow(max(0.0, cos((y * 12.0 - t * 8.0) * 3.14159)), 6.0) * exp(-y * 2.2) * shock_amp;

    let axial_fade = (1.0 - smoothstep(0.15, 1.0, y)) * smoothstep(0.0, 0.04, y);
    let radial_fade = exp(-radial * radial * mix(1.75, 2.8, pressure));
    let energy = axial_fade * radial_fade * (0.72 + shock) * throttle;

    let vac_tint = vec3<f32>(0.68, 0.74, 1.0);
    let outer_col = mix(vac_tint * u.outer_color.rgb, u.outer_color.rgb, pressure);
    let col = mix(outer_col, u.core_color.rgb, clamp(radial_fade * 0.8 + shock * 0.65, 0.0, 1.0));
    let hdr_col = col * (energy * mix(2.2, 3.2, pressure));
    return vec4<f32>(hdr_col, clamp(energy * 1.4, 0.0, 1.0));
}
