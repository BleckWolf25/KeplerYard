import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct PartMaterialUniforms {
    base_color: vec4<f32>,
    accent_color: vec4<f32>,
    flags: vec4<f32>,
    thermal: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> u: PartMaterialUniforms;

fn hash12(p: vec2<f32>) -> f32 {
    let p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    let d = dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * d);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let style = i32(round(u.thermal.z));

    // FX / starfield / pad flat emissive pass (style 99)
    if style >= 90 {
        let rim_fx = 0.85 + 0.15 * (1.0 - abs(uv.x - 0.5) * 2.0);
        return vec4<f32>(u.base_color.rgb * rim_fx * max(u.flags.x * 0.65, 1.0), u.base_color.a);
    }

    // Deployed parachute canopy gores (style 13)
    if style == 13 {
        let gore = step(0.5, fract(uv.x * 6.0));
        let col = mix(u.base_color.rgb, u.accent_color.rgb, gore * 0.82);
        let rib = smoothstep(0.08, 0.01, abs(fract(uv.x * 6.0) - 0.5));
        let seam = smoothstep(0.04, 0.0, abs(uv.y - 0.88));
        let shaded = col * (0.76 + 0.28 * uv.y) * (1.0 - 0.25 * rib - 0.25 * seam);
        return vec4<f32>(shaded, u.base_color.a);
    }

    // 3D cylindrical normal & directional metallic lighting
    let nx = clamp((uv.x - 0.5) * 2.0, -0.99, 0.99);
    let nz = sqrt(max(0.02, 1.0 - nx * nx));
    let normal = normalize(vec3<f32>(nx, 0.12, nz));
    let light_dir = normalize(vec3<f32>(-0.55, 0.35, 0.76));
    let view_dir = vec3<f32>(0.0, 0.0, 1.0);
    let half_vec = normalize(light_dir + view_dir);

    let ndotl = max(dot(normal, light_dir), 0.0);
    let cyl_diff = 0.42 + 0.68 * ndotl + 0.12 * max(nx, 0.0);
    let spec = pow(max(dot(normal, half_vec), 0.0), 18.0) * 0.24;
    let edge_ao = smoothstep(1.0, 0.72, abs(nx));

    let micro = (hash12(floor(uv * vec2<f32>(28.0, 48.0))) - 0.5) * 0.035;
    var rgb = (u.base_color.rgb + micro) * cyl_diff * (0.82 + 0.18 * edge_ao) + vec3<f32>(spec);

    // Part-specific realistic aerospace detail
    if style == 0 {
        // Fuel tanks: top/bottom weld collars, panel rings, stringers & propellant sight gauge
        let cap_mask = step(uv.y, 0.055) + step(0.945, uv.y);
        rgb = mix(rgb, rgb * 0.52, clamp(cap_mask, 0.0, 1.0));

        let ring = smoothstep(0.028, 0.0, abs(fract(uv.y * 4.0) - 0.5));
        let stringer = sin(nx * 26.0) * 0.025;
        rgb = rgb * (1.0 - ring * 0.28) + stringer;

        let band = step(0.84, uv.y) * step(uv.y, 0.91);
        rgb = mix(rgb, u.accent_color.rgb * cyl_diff, band * 0.85);

        let gauge = smoothstep(0.065, 0.025, abs(uv.x - 0.5)) * step(0.10, uv.y) * step(uv.y, 0.82);
        let fuel_ok = step(uv.y, clamp(0.10 + u.flags.z * 0.72, 0.10, 0.82));
        let gauge_col = mix(vec3<f32>(0.12, 0.14, 0.18), u.accent_color.rgb * max(u.flags.x * 0.7, 1.0), fuel_ok);
        rgb = mix(rgb, gauge_col, gauge * 0.88);
    } else if style == 1 {
        // Command pod: heat tiles, upper docking collar, lower skirt & tinted cockpit viewport
        let top_cap = step(0.86, uv.y);
        let bot_skirt = step(uv.y, 0.12);
        rgb = mix(rgb, vec3<f32>(0.14, 0.16, 0.20) * cyl_diff, clamp(top_cap + bot_skirt, 0.0, 1.0));

        let seam_v = smoothstep(0.025, 0.0, abs(fract(uv.x * 4.0) - 0.5)) * 0.22;
        let seam_h = smoothstep(0.025, 0.0, abs(uv.y - 0.34)) * 0.28;
        rgb *= (1.0 - seam_v - seam_h);

        let stripe = step(0.13, uv.y) * step(uv.y, 0.20);
        rgb = mix(rgb, u.accent_color.rgb * cyl_diff, stripe * 0.82);

        // Curved cockpit viewport window
        let win_p = (uv - vec2<f32>(0.5, 0.58)) * vec2<f32>(1.0, 1.35);
        let win_d = length(win_p);
        let frame = smoothstep(0.22, 0.19, win_d);
        let glass = smoothstep(0.16, 0.14, win_d);
        let glint = smoothstep(0.09, 0.02, length(win_p - vec2<f32>(-0.05, 0.04)));
        let glass_col = vec3<f32>(0.06, 0.18, 0.28) + u.accent_color.rgb * (0.35 + 0.7 * glint);
        rgb = mix(rgb, vec3<f32>(0.42, 0.46, 0.52) * cyl_diff, frame);
        rgb = mix(rgb, glass_col, glass);
    } else if style == 2 {
        // Probe core: gold Kapton thermal foil center + dark structural rails + status LED
        let rail = step(uv.y, 0.20) + step(0.80, uv.y);
        let foil = vec3<f32>(0.86, 0.64, 0.16) * (cyl_diff + spec * 1.4 + micro * 3.0);
        rgb = mix(foil, vec3<f32>(0.18, 0.20, 0.25) * cyl_diff, clamp(rail, 0.0, 1.0));
        let led = smoothstep(0.08, 0.03, length((uv - vec2<f32>(0.5, 0.5)) * vec2<f32>(1.4, 1.0)));
        rgb += u.accent_color.rgb * led * 2.2;
    } else if style == 3 {
        // Rocket engine: dark turbopump top + ribbed regenerative nozzle bell + throat glow
        let upper = step(0.66, uv.y);
        let hoops = 0.85 + 0.15 * sin(uv.y * 62.0);
        let soot = smoothstep(0.0, 0.14, uv.y);
        let bell_col = rgb * hoops * mix(0.45, 1.0, soot);
        let pump_col = vec3<f32>(0.14, 0.16, 0.20) * cyl_diff + vec3<f32>(spec * 0.8);
        rgb = mix(bell_col, pump_col, upper);

        let pipe = smoothstep(0.045, 0.015, abs(uv.x - 0.28)) * step(0.58, uv.y);
        rgb = mix(rgb, vec3<f32>(0.55, 0.58, 0.64) * cyl_diff, pipe * 0.75);

        let eng_glow = clamp(u.thermal.w, 0.0, 1.0);
        let throat_mask = (1.0 - smoothstep(0.0, 0.58, uv.y)) * (1.0 - abs(nx) * 0.55);
        rgb += vec3<f32>(1.0, 0.44, 0.10) * eng_glow * throat_mask * 2.3;
    } else if style == 4 {
        // SRB booster: conical top nose, segmented rings, hazard band & bottom nozzle skirt
        let is_noz = step(uv.y, 0.12);
        let is_nose = step(0.86, uv.y);
        let seg = smoothstep(0.025, 0.0, abs(fract(uv.y * 6.0) - 0.5));
        rgb *= (1.0 - seg * 0.35);

        let roll_band = step(0.74, uv.y) * step(uv.y, 0.81) + step(0.16, uv.y) * step(uv.y, 0.21);
        let checker = step(0.5, fract(uv.x * 4.0));
        let band_col = mix(u.accent_color.rgb, vec3<f32>(0.95, 0.95, 0.96), checker * 0.25) * cyl_diff;
        rgb = mix(rgb, band_col, clamp(roll_band, 0.0, 1.0) * 0.9);
        rgb = mix(rgb, vec3<f32>(0.76, 0.78, 0.82) * cyl_diff, is_nose * 0.7);
        rgb = mix(rgb, vec3<f32>(0.18, 0.19, 0.22) * cyl_diff, is_noz);
        rgb += vec3<f32>(1.0, 0.50, 0.14) * clamp(u.thermal.w, 0.0, 1.0) * is_noz * 2.0;
    } else if style == 5 {
        // Stack decoupler: dark collars + diagonal interstage truss + hazard chevron stripe
        let truss = smoothstep(0.18, 0.05, abs(fract(uv.x * 5.0 + uv.y * 2.0) - 0.5));
        let truss_b = smoothstep(0.18, 0.05, abs(fract(uv.x * 5.0 - uv.y * 2.0) - 0.5));
        let frame = step(uv.y, 0.18) + step(0.82, uv.y);
        rgb = mix(rgb * 0.48, rgb * 1.1, clamp(truss + truss_b + frame, 0.0, 1.0));
        let chev = step(0.5, fract(uv.x * 8.0 + uv.y * 2.0)) * step(0.36, uv.y) * step(uv.y, 0.64);
        rgb = mix(rgb, u.accent_color.rgb * cyl_diff, chev * 0.88);
    } else if style == 6 {
        // Radial pylon decoupler: steel strut frame + yellow pyrotechnic bolt
        let bolt = step(0.38, uv.y) * step(uv.y, 0.62);
        rgb = mix(rgb, u.accent_color.rgb * cyl_diff, bolt * 0.85);
    } else if style == 7 {
        // Heat shield: ablative PICA-X tile dome + charring as ablator depletes
        let ablator_frac = clamp(u.thermal.y, 0.0, 1.0);
        let tile = smoothstep(0.04, 0.0, abs(fract(uv.x * 6.0) - 0.5)) * 0.25;
        let char_col = mix(vec3<f32>(0.08, 0.07, 0.07), u.base_color.rgb, ablator_frac);
        rgb = char_col * cyl_diff * (1.0 - tile);
        let rim_s = step(0.78, uv.y);
        rgb = mix(rgb, u.accent_color.rgb * 0.65 * cyl_diff, rim_s * 0.5);
    } else if style == 8 {
        // Aerodynamic fin: reinforced carbon-carbon leading edge + spar lines
        let flat_shade = 0.72 + 0.28 * uv.y;
        rgb = u.base_color.rgb * flat_shade;
        let leading = step(0.84, uv.y);
        let spar = smoothstep(0.03, 0.0, abs(uv.x - 0.5)) * 0.22;
        rgb = mix(rgb, vec3<f32>(0.14, 0.15, 0.18), leading * 0.85) * (1.0 - spar);
        let trim = step(0.10, uv.y) * step(uv.y, 0.18);
        rgb = mix(rgb, u.accent_color.rgb, trim * 0.75);
    } else if style == 9 {
        // Nosecone: bright aero alloy + metallic tip cap + accent ring
        let tip = step(0.86, uv.y);
        let ring = step(0.10, uv.y) * step(uv.y, 0.18);
        rgb = mix(rgb, vec3<f32>(0.45, 0.48, 0.54) * cyl_diff, tip * 0.85);
        rgb = mix(rgb, u.accent_color.rgb * cyl_diff, ring * 0.9);
    } else if style == 10 {
        // Parachute canister: high-vis fiberglass dome + deployment cross strap
        let strap = smoothstep(0.08, 0.02, abs(uv.x - 0.5)) + step(uv.y, 0.16);
        rgb = mix(rgb, u.accent_color.rgb * cyl_diff, clamp(strap, 0.0, 1.0) * 0.80);
    } else if style == 11 {
        // Landing strut: dark shock cylinder + bright chrome piston + titanium footpad
        let flat = 0.78 + 0.22 * (1.0 - abs(nx) * 0.5);
        let foot = step(uv.y, 0.16);
        let piston = step(0.16, uv.y) * step(uv.y, 0.46);
        rgb = u.base_color.rgb * flat;
        rgb = mix(rgb, vec3<f32>(0.78, 0.82, 0.88) * flat, piston * 0.85);
        rgb = mix(rgb, vec3<f32>(0.20, 0.22, 0.26) * flat, foot);
    } else if style == 12 {
        // RCS block: thermal blanket + dark thruster ports
        let port = smoothstep(0.12, 0.05, length(uv - vec2<f32>(0.5, 0.5)));
        rgb = mix(rgb, vec3<f32>(0.10, 0.12, 0.15), port * 0.9);
        let trim = step(0.78, uv.y);
        rgb = mix(rgb, u.accent_color.rgb * cyl_diff, trim * 0.75);
    }

    // VAB Selection Highlight Pulse (u.flags.y)
    let edge_y = abs(uv.y - 0.5) * 2.0;
    let rim = smoothstep(0.84, 0.99, max(abs(nx), edge_y));
    let pulse = 0.75 + 0.25 * sin(u.flags.w * 5.0);
    let highlight = clamp(u.flags.y, 0.0, 1.0);
    rgb += u.accent_color.rgb * rim * highlight * 1.9 * pulse;

    // Flight Reentry Incandescent Heating (u.thermal.x)
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
        let rim_mask = smoothstep(0.55, 0.98, max(abs(nx), edge_y));
        rgb += heat_col * heat * (0.45 + 2.2 * rim_mask);
    }

    return vec4<f32>(rgb, u.base_color.a);
}
