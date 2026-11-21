import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct ReentryUniforms {
    params: vec4<f32>,
    plasma_color: vec4<f32>,
    wake_color: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> u: ReentryUniforms;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let intensity = clamp(u.params.x, 0.0, 2.0);
    if intensity <= 0.01 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    let x = (in.uv.x - 0.5) * 2.0;
    let y = in.uv.y;
    let t = u.params.y;

    let bow = exp(-abs(y - 0.14 - 0.10 * x * x) * 18.0);
    let streak = 0.72 + 0.28 * sin(x * 16.0 + y * 14.0 - t * 28.0);
    let envelope = exp(-x * x * 1.8) * (1.0 - smoothstep(0.22, 1.0, y)) * smoothstep(0.0, 0.08, y);

    let energy = (bow * 1.45 + envelope * streak * 0.9) * intensity;
    let col = mix(u.wake_color.rgb, u.plasma_color.rgb, clamp(bow * 1.2, 0.0, 1.0));
    return vec4<f32>(col * (energy * 2.4), clamp(energy, 0.0, 0.92));
}
