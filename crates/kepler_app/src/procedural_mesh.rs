//! @file procedural_mesh.rs
//!
//! @version 1.3.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Multi-ring SFS-inspired procedural 2D rocket meshes with per-ring cylindrical UVs
//!
//! @description
//! Builds curved multi-ring CCW triangle meshes for pods, parabolic De Laval nozzles, solid
//! boosters with nose cones and flared exhaust skirts, rounded tanks, mirrored delta fins,
//! landing struts, parachute canisters, and multi-gore deployed canopies.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use sim_core::PartShape;

// ---------- CONSTANTS

// ---------- TYPES

// ---------- DECLARATIONS / IMPLEMENTATION

pub fn build_part_mesh(shape: &PartShape, mirrored: bool) -> Mesh {
    match *shape {
        PartShape::RoundedRect {
            width,
            height,
            corner_radius,
        } => build_rounded_rect_mesh(width as f32, height as f32, corner_radius as f32),
        PartShape::Cone {
            bottom_width,
            top_width,
            height,
        } => build_capsule_cone_mesh(bottom_width as f32, top_width as f32, height as f32),
        PartShape::Nozzle {
            mount_width,
            throat_width,
            bell_width,
            height,
        } => build_nozzle_mesh(
            mount_width as f32,
            throat_width as f32,
            bell_width as f32,
            height as f32,
        ),
        PartShape::Fin {
            root_chord,
            tip_chord,
            span,
            sweep,
        } => build_fin_mesh(
            root_chord as f32,
            tip_chord as f32,
            span as f32,
            sweep as f32,
            mirrored,
        ),
        PartShape::Dome { width, height } => build_dome_mesh(width as f32, height as f32),
        PartShape::LandingStrut {
            width,
            height,
            foot_span,
        } => build_landing_strut_mesh(width as f32, height as f32, foot_span as f32, mirrored),
        PartShape::Booster {
            width,
            height,
            nozzle_width,
        } => build_booster_mesh(width as f32, height as f32, nozzle_width as f32),
    }
}

pub fn build_quad_mesh(width: f32, height: f32) -> Mesh {
    let hw = width * 0.5;
    let hh = height * 0.5;
    let positions = vec![
        [-hw, hh, 0.0],
        [hw, hh, 0.0],
        [hw, -hh, 0.0],
        [-hw, -hh, 0.0],
    ];
    let uvs = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let indices = vec![0, 3, 2, 0, 2, 1];
    assemble_2d_mesh(positions, uvs, indices)
}

pub fn build_ring_mesh(inner_r: f32, outer_r: f32, segments: usize) -> Mesh {
    let segs = segments.max(12);
    let mut positions = Vec::with_capacity((segs + 1) * 2);
    let mut uvs = Vec::with_capacity((segs + 1) * 2);
    for i in 0..=segs {
        let t = i as f32 / segs as f32;
        let ang = t * core::f32::consts::TAU;
        let (s, c) = ang.sin_cos();
        positions.push([c * inner_r, s * inner_r, 0.0]);
        uvs.push([0.5, 0.2]);
        positions.push([c * outer_r, s * outer_r, 0.0]);
        uvs.push([0.5, 0.8]);
    }
    let mut indices = Vec::with_capacity(segs * 6);
    for i in 0..segs {
        let base = (i * 2) as u32;
        indices.push(base);
        indices.push(base + 1);
        indices.push(base + 3);
        indices.push(base);
        indices.push(base + 3);
        indices.push(base + 2);
    }
    assemble_2d_mesh(positions, uvs, indices)
}

pub fn build_plume_quad(width: f32, length: f32) -> Mesh {
    let hw = width * 0.5;
    let positions = vec![
        [-hw, 0.0, 0.0],
        [hw, 0.0, 0.0],
        [hw, -length, 0.0],
        [-hw, -length, 0.0],
    ];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let indices = vec![0, 3, 2, 0, 2, 1];
    assemble_2d_mesh(positions, uvs, indices)
}

pub fn build_reentry_quad(width: f32, length: f32) -> Mesh {
    let hw = width * 0.5;
    let positions = vec![
        [-hw, 0.35 * length, 0.0],
        [hw, 0.35 * length, 0.0],
        [hw, -0.65 * length, 0.0],
        [-hw, -0.65 * length, 0.0],
    ];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let indices = vec![0, 3, 2, 0, 2, 1];
    assemble_2d_mesh(positions, uvs, indices)
}

/// Multi-gore arched parachute canopy + shroud triangle fan extending upward (`+Y`).
pub fn build_deployed_chute_mesh(canopy_width: f32, riser_height: f32) -> Mesh {
    let hw = canopy_width * 0.5;
    let segs = 16_usize;
    let mut positions = Vec::with_capacity(segs + 2);
    let mut uvs = Vec::with_capacity(segs + 2);
    positions.push([0.0, 0.0, 0.0]);
    uvs.push([0.5, 0.0]);

    for i in 0..=segs {
        let t = i as f32 / segs as f32;
        let ang = core::f32::consts::PI * t;
        let x = -hw * ang.cos();
        let y = riser_height + 0.38 * canopy_width * ang.sin();
        positions.push([x, y, 0.0]);
        uvs.push([t, 1.0]);
    }

    let mut indices = Vec::with_capacity(segs * 3);
    for i in 0..segs {
        indices.push(0);
        indices.push(1 + i as u32);
        indices.push(2 + i as u32);
    }
    assemble_2d_mesh(positions, uvs, indices)
}

/// Builds a multi-ring symmetric mesh from bottom-to-top `(y, width)` slices with cylindrical UVs.
fn build_stacked_strip_mesh(rings: &[(f32, f32)], total_h: f32) -> Mesh {
    let h = total_h.max(0.05);
    let mut positions = Vec::with_capacity(rings.len() * 2);
    let mut uvs = Vec::with_capacity(rings.len() * 2);
    for &(y, w) in rings {
        let hw = (w * 0.5).max(0.01);
        let v = (y / h + 0.5).clamp(0.0, 1.0);
        positions.push([-hw, y, 0.0]);
        uvs.push([0.0, v]);
        positions.push([hw, y, 0.0]);
        uvs.push([1.0, v]);
    }

    let mut indices = Vec::with_capacity(rings.len().saturating_sub(1) * 6);
    for i in 0..rings.len().saturating_sub(1) {
        let b = (i * 2) as u32;
        indices.push(b);
        indices.push(b + 1);
        indices.push(b + 3);
        indices.push(b);
        indices.push(b + 3);
        indices.push(b + 2);
    }
    assemble_2d_mesh(positions, uvs, indices)
}

fn build_rounded_rect_mesh(w: f32, h: f32, radius: f32) -> Mesh {
    let r = radius.min(w * 0.45).min(h * 0.45).max(0.01);
    let hw = w * 0.5;
    let hh = h * 0.5;
    let ix = hw - r;
    let iy = hh - r;

    let corners = [
        (Vec2::new(ix, iy), 0.0_f32),
        (Vec2::new(-ix, iy), core::f32::consts::FRAC_PI_2),
        (Vec2::new(-ix, -iy), core::f32::consts::PI),
        (Vec2::new(ix, -iy), 1.5 * core::f32::consts::PI),
    ];

    let segs_per_corner = 4_usize;
    let mut ring = Vec::with_capacity(corners.len() * (segs_per_corner + 1));
    for (center, start_ang) in corners {
        for s in 0..=segs_per_corner {
            let t = s as f32 / segs_per_corner as f32;
            let ang = start_ang + t * core::f32::consts::FRAC_PI_2;
            ring.push(center + Vec2::new(ang.cos(), ang.sin()) * r);
        }
    }

    fan_polygon(&ring, w, h)
}

fn build_capsule_cone_mesh(bottom_w: f32, top_w: f32, h: f32) -> Mesh {
    let hh = h * 0.5;
    let steps = 8_usize;
    let mut rings = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let y = -hh + t * h;
        let curve = if top_w < bottom_w * 0.4 {
            t.powf(1.22)
        } else {
            t
        };
        let w = bottom_w * (1.0 - curve) + top_w * curve;
        rings.push((y, w));
    }
    build_stacked_strip_mesh(&rings, h)
}

fn build_dome_mesh(w: f32, h: f32) -> Mesh {
    let hh = h * 0.5;
    let steps = 8_usize;
    let mut rings = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let y = -hh + t * h;
        let rw = w * (1.0 - t * t * 0.88).max(0.04).sqrt();
        rings.push((y, rw));
    }
    build_stacked_strip_mesh(&rings, h)
}

fn build_nozzle_mesh(mount_w: f32, throat_w: f32, bell_w: f32, h: f32) -> Mesh {
    let hh = h * 0.5;
    let rings = [
        (-hh, bell_w),
        (-hh + h * 0.18, bell_w * 0.94),
        (-hh + h * 0.38, bell_w * 0.78),
        (-hh + h * 0.56, throat_w * 1.12),
        (-hh + h * 0.66, throat_w),
        (-hh + h * 0.76, mount_w * 0.78),
        (-hh + h * 0.88, mount_w * 0.96),
        (hh, mount_w),
    ];
    build_stacked_strip_mesh(&rings, h)
}

fn build_booster_mesh(w: f32, h: f32, nozzle_w: f32) -> Mesh {
    let hh = h * 0.5;
    let rings = [
        (-hh, nozzle_w),
        (-hh + h * 0.08, nozzle_w * 0.52),
        (-hh + h * 0.13, w * 0.96),
        (-hh + h * 0.85, w),
        (-hh + h * 0.92, w * 0.68),
        (hh, w * 0.18),
    ];
    build_stacked_strip_mesh(&rings, h)
}

fn build_fin_mesh(
    root_chord: f32,
    tip_chord: f32,
    span: f32,
    sweep: f32,
    mirrored: bool,
) -> Mesh {
    let hx = span * 0.5;
    let hy = root_chord * 0.5;
    let tip_top = hy - sweep;
    let tip_bot = tip_top - tip_chord;
    let inv_w = 1.0 / span.max(0.05);
    let inv_h = 1.0 / root_chord.max(0.05);
    let sx = if mirrored { -1.0 } else { 1.0 };

    let pts = [
        [sx * hx, hy, 0.0],
        [sx * hx, -hy, 0.0],
        [-sx * hx, tip_bot, 0.0],
        [-sx * hx, tip_top, 0.0],
    ];
    let uvs = pts
        .iter()
        .map(|p| [p[0] * inv_w + 0.5, p[1] * inv_h + 0.5])
        .collect();
    let indices = if mirrored {
        vec![0, 1, 2, 0, 2, 3]
    } else {
        vec![0, 2, 1, 0, 3, 2]
    };
    assemble_2d_mesh(pts.to_vec(), uvs, indices)
}

fn build_landing_strut_mesh(w: f32, h: f32, foot_span: f32, mirrored: bool) -> Mesh {
    let bound_w = w.max(foot_span);
    let hx = bound_w * 0.5;
    let hy = h * 0.5;
    let s = if mirrored { -1.0 } else { 1.0 };

    let pts = vec![
        [s * hx * 0.45, hy * 0.92, 0.0],
        [s * hx * 0.95, hy * 0.92, 0.0],
        [-s * hx * 0.25, -hy * 0.84, 0.0],
        [-s * hx * 0.72, -hy * 0.84, 0.0],
        [-s * hx * 0.96, -hy * 0.80, 0.0],
        [-s * hx * 0.08, -hy * 0.80, 0.0],
        [-s * hx * 0.08, -hy, 0.0],
        [-s * hx * 0.96, -hy, 0.0],
        [s * hx * 0.65, -hy * 0.15, 0.0],
        [s * hx * 0.95, -hy * 0.15, 0.0],
        [-s * hx * 0.22, -hy * 0.72, 0.0],
        [-s * hx * 0.42, -hy * 0.72, 0.0],
    ];

    let inv_w = 1.0 / bound_w.max(0.05);
    let inv_h = 1.0 / h.max(0.05);
    let uvs = pts
        .iter()
        .map(|p| [p[0] * inv_w + 0.5, p[1] * inv_h + 0.5])
        .collect();

    let mut indices = Vec::with_capacity(18);
    for qi in 0..3_u32 {
        let b = qi * 4;
        if mirrored {
            indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        } else {
            indices.extend_from_slice(&[b, b + 2, b + 1, b, b + 3, b + 2]);
        }
    }
    assemble_2d_mesh(pts, uvs, indices)
}

fn fan_polygon(ring: &[Vec2], bound_w: f32, bound_h: f32) -> Mesh {
    let mut positions = Vec::with_capacity(ring.len() + 1);
    let mut uvs = Vec::with_capacity(ring.len() + 1);
    positions.push([0.0, 0.0, 0.0]);
    uvs.push([0.5, 0.5]);

    let inv_w = 1.0 / bound_w.max(0.05);
    let inv_h = 1.0 / bound_h.max(0.05);
    for pt in ring {
        positions.push([pt.x, pt.y, 0.0]);
        uvs.push([pt.x * inv_w + 0.5, pt.y * inv_h + 0.5]);
    }

    let n = ring.len() as u32;
    let mut indices = Vec::with_capacity(ring.len() * 3);
    for i in 0..n {
        let next = if i + 1 < n { i + 1 } else { 0 };
        indices.push(0);
        indices.push(1 + i);
        indices.push(1 + next);
    }

    assemble_2d_mesh(positions, uvs, indices)
}

fn assemble_2d_mesh(positions: Vec<[f32; 3]>, uvs: Vec<[f32; 2]>, indices: Vec<u32>) -> Mesh {
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ---------- EXPORTS
