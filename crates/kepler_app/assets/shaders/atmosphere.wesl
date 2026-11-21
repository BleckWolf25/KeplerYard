import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct PlanetAtmosphereUniforms {
    planet_params: vec4<f32>,
    atmo_params: vec4<f32>,
    surface_color: vec4<f32>,
    atmo_color: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> u: PlanetAtmosphereUniforms;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xy;
    let u_rel = u.planet_params.xy;
    let h_origin = u.planet_params.z;
    let radius = u.planet_params.w;

    // Shifted-origin algebraic distance avoiding 600 km f32 cancellation near launchpad.
    let r_cam = length(u_rel);
    let num = h_origin * (r_cam + radius) + 2.0 * dot(u_rel, p) + dot(p, p);
    let denom = max(length(u_rel + p) + radius, 1.0);
    let alt = num / denom;

    let scale_h = max(u.atmo_params.x, 1.0);
    let max_atmo = max(u.atmo_params.y, 1.0);

    if alt <= 0.0 {
        let depth = clamp(-alt / 4500.0, 0.0, 1.0);
        let core_col = mix(u.surface_color.rgb * 1.15, u.surface_color.rgb * 0.25, depth);
        let horizon_glow = exp(max(alt, -180.0) * 0.03) * u.atmo_color.rgb * 0.45;
        let pad_ticks = step(0.92, fract((u_rel.x + p.x) * 0.05)) * exp(max(alt, -10.0) * 0.4) * 0.15;
        return vec4<f32>(core_col + horizon_glow + vec3<f32>(pad_ticks), 1.0);
    }

    let density = exp(-alt / scale_h);
    let outer_fade = 1.0 - smoothstep(max_atmo * 0.65, max_atmo * 1.35, alt);
    let horizon_edge = exp(-alt / 650.0) * 0.9;
    let atmo_intensity = clamp(density * outer_fade + horizon_edge, 0.0, 2.5);

    let limb = exp(-alt * 0.25) * 1.4;
    let atmo_rgb = u.atmo_color.rgb * (atmo_intensity * 1.15 + limb);
    let alpha = clamp(atmo_intensity * 0.92, 0.0, 1.0) * step(alt, max_atmo * 1.5);
    return vec4<f32>(atmo_rgb, alpha);
}
