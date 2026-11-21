import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct PartMaterialUniforms {
    base_color: vec4<f32>,
    accent_color: vec4<f32>,
    flags: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> u: PartMaterialUniforms;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let edge_x = abs(uv.x - 0.5) * 2.0;
    let edge_y = abs(uv.y - 0.5) * 2.0;

    let shade = 0.78 + 0.28 * (1.0 - edge_x * 0.65);
    var rgb = u.base_color.rgb * shade;

    let stripe = smoothstep(0.10, 0.03, abs(uv.x - 0.5));
    let fuel_ok = step(uv.y, clamp(u.flags.z, 0.0, 1.0));
    let accent_glow = u.accent_color.rgb * u.flags.x;
    rgb += accent_glow * stripe * 0.42 * (0.25 + 0.75 * fuel_ok);

    let rim = smoothstep(0.86, 0.99, max(edge_x, edge_y));
    let pulse = 0.75 + 0.25 * sin(u.flags.w * 5.0);
    let highlight = u.flags.y;
    rgb += u.accent_color.rgb * rim * (0.35 + highlight * 1.8 * pulse);

    return vec4<f32>(rgb, u.base_color.a);
}
