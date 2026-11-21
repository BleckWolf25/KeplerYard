import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct PlanetAtmosphereUniforms {
    planet_params: vec4<f32>,
    atmo_params: vec4<f32>,
    surface_color: vec4<f32>,
    atmo_color: vec4<f32>,
    sun_and_detail: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> u: PlanetAtmosphereUniforms;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xy;
    let u_rel = u.planet_params.xy;
    let h_origin = u.planet_params.z;
    let radius = u.planet_params.w;

    let r_cam = length(u_rel);
    let num = h_origin * (r_cam + radius) + 2.0 * dot(u_rel, p) + dot(p, p);
    let denom = max(length(u_rel + p) + radius, 1.0);
    let alt = num / denom;

    let scale_h = u.atmo_params.x;
    let max_atmo = u.atmo_params.y;

    let world_rel = u_rel + p;
    let radial_dir = normalize(world_rel + vec2<f32>(0.0, 0.0001));
    let sun_dir = normalize(u.sun_and_detail.xy + vec2<f32>(0.0001, 0.0));
    let ndotl = dot(radial_dir, sun_dir);
    let day = smoothstep(-0.18, 0.34, ndotl);
    let twilight = exp(-abs(ndotl) * 10.0);
    let theta = atan2(world_rel.y, world_rel.x);

    if alt <= 0.0 {
        let depth = clamp(-alt / max(radius * 0.12, 4500.0), 0.0, 1.0);
        var surf = u.surface_color.rgb;

        if max_atmo > 1.0 {
            let cont = sin(theta * 5.0 + 0.7) * 0.5 + sin(theta * 13.0 - 1.2) * 0.35;
            let land_mask = smoothstep(0.10, 0.38, cont) * (1.0 - smoothstep(1500.0, 16000.0, -alt));
            let ocean_col = u.surface_color.rgb * vec3<f32>(0.85, 1.05, 1.35);
            let land_col = vec3<f32>(0.16, 0.27, 0.22);
            surf = mix(ocean_col, land_col, land_mask);
            let pad_zone = 1.0 - smoothstep(0.0, 0.00018, abs(theta - 1.5707963));
            let pad_depth = 1.0 - smoothstep(0.0, 22.0, -alt);
            surf = mix(surf, vec3<f32>(0.38, 0.42, 0.48), pad_zone * pad_depth * 0.75);
        } else {
            let crater_a = sin(theta * 24.0 + 1.1) * cos(alt * 0.00025);
            let crater_b = sin(theta * 61.0 - 0.5) * 0.5;
            let regolith = clamp(0.85 + 0.18 * (crater_a + crater_b), 0.58, 1.22);
            surf *= regolith;
        }

        let lit_surf = mix(surf * 0.14, surf * 1.22, day);
        let core_col = mix(lit_surf, u.surface_color.rgb * 0.12, depth);

        if max_atmo <= 1.0 {
            let rim = exp(max(alt, -1800.0) * 0.0025) * u.atmo_color.rgb * (0.10 + 0.35 * day);
            return vec4<f32>(core_col + rim, 1.0);
        }

        let twilight_tint = vec3<f32>(1.0, 0.48, 0.22) * twilight * 0.30;
        let horizon_glow = exp(max(alt, -180.0) * 0.03) * (u.atmo_color.rgb * (0.12 + 0.45 * day) + twilight_tint);
        return vec4<f32>(core_col + horizon_glow, 1.0);
    }

    if max_atmo <= 1.0 {
        let limb = exp(-alt * 0.003) * (0.10 + 0.30 * day) * step(alt, 2500.0);
        return vec4<f32>(u.atmo_color.rgb * limb, limb);
    }

    let density = exp(-alt / max(scale_h, 1.0));
    let outer_fade = 1.0 - smoothstep(max_atmo * 0.65, max_atmo * 1.35, alt);
    let horizon_edge = exp(-alt / 650.0) * 0.9;
    let sun_scatter = 0.20 + 0.85 * day;
    let atmo_intensity = clamp((density * outer_fade + horizon_edge) * sun_scatter, 0.0, 2.5);

    let sunset_col = vec3<f32>(1.0, 0.45, 0.18);
    let scatter_col = mix(u.atmo_color.rgb, sunset_col, twilight * 0.58);
    let limb = exp(-alt * 0.25) * 1.4 * sun_scatter;
    let atmo_rgb = scatter_col * (atmo_intensity * 1.15 + limb);
    let alpha = clamp(atmo_intensity * 0.92, 0.0, 1.0) * step(alt, max_atmo * 1.5);
    return vec4<f32>(atmo_rgb, alpha);
}
