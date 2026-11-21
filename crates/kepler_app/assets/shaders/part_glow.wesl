import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct PartMaterialUniforms {
    base_color: vec4<f32>,
    accent_color: vec4<f32>,
    flags: vec4<f32>,
    thermal: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> u: PartMaterialUniforms;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let edge_x = abs(uv.x - 0.5) * 2.0;
    let edge_y = abs(uv.y - 0.5) * 2.0;

    let shade = 0.78 + 0.28 * (1.0 - edge_x * 0.65);
    let ablator_char = mix(0.42, 1.0, clamp(u.thermal.y, 0.0, 1.0));
    var rgb = u.base_color.rgb * shade * ablator_char;

    let stripe = smoothstep(0.10, 0.03, abs(uv.x - 0.5));
    let fuel_ok = step(uv.y, clamp(u.flags.z, 0.0, 1.0));
    let accent_glow = u.accent_color.rgb * u.flags.x;
    rgb += accent_glow * stripe * 0.42 * (0.25 + 0.75 * fuel_ok);

    let rim = smoothstep(0.86, 0.99, max(edge_x, edge_y));
    let pulse = 0.75 + 0.25 * sin(u.flags.w * 5.0);
    let highlight = clamp(u.flags.y, 0.0, 1.0);
    rgb += u.accent_color.rgb * rim * (0.32 + highlight * 1.85 * pulse);

    let heat = clamp(u.thermal.x, 0.0, 1.0);
    if heat > 0.01 {
        let hot_low = vec3<f32>(0.95, 0.16, 0.02);
        let hot_mid = vec3<f32>(1.00, 0.54, 0.08);
        let hot_hi = vec3<f32>(1.00, 0.92, 0.65);
        let heat_col = mix(
            mix(hot_low, hot_mid, smoothstep(0.0, 0.55, heat)),
            hot_hi,
            smoothstep(0.55, 1.0, heat),
        );
        let rim_mask = smoothstep(0.55, 0.98, max(edge_x, edge_y));
        rgb += heat_col * heat * (0.45 + 2.2 * rim_mask);
    }

    return vec4<f32>(rgb, u.base_color.a);
}
