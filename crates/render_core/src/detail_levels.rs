//! Simpler copies of a geometry, for drawing it with fewer triangles when it looks small. A copy is a new list of
//! triangles over the same vertices, so it sits in the geometry's index buffer right after the full geometry's triangles.
//! Each frame, every instance draws with the simplest copy whose error would be too small to see from the camera.

use crate::renderer::GpuGeometryBuffer;
use glam::{Mat4, Vec3};
use meshoptimizer_rs::{Attributes, Positions, SimplifyOptions, SimplifySettings, VertexFlags, Workspace, simplify_scale, simplify_with_attributes};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

pub struct DetailLevel {
    pub index_range: Range<u32>, // this copy's triangles in the geometry's index buffer
    pub error: f32,              // how far this copy's surface can be from the full geometry's, in the geometry's own units
}

// TODO: AI-PICKED-VALUE: two simpler copies, each aiming for a quarter of the triangles of the one before it.
const SIMPLER_DETAIL_LEVEL_COUNT: usize = 2;
const SIMPLER_DETAIL_LEVEL_TRIANGLE_SHARE: f32 = 0.25;
// TODO: AI-PICKED-VALUE: a copy has to drop at least half of the triangles of the one before it to be worth its own draws.
const SIMPLER_DETAIL_LEVEL_MAX_TRIANGLE_SHARE: f32 = 0.5;
// TODO: AI-PICKED-VALUE: an instance draws with a copy once that copy's error would cover at most this many pixels on screen.
// At a whole pixel, game 381 lost thin dark bars (window frames, ladder rungs) on faraway buildings. At a quarter, its
// pictures only differ in faint texture shading at the far ends of hay bales.
const DETAIL_LEVEL_MAX_ERROR_PIXELS: f32 = 0.25;

pub fn bounding_radius_around_origin(positions: impl IntoIterator<Item = [f32; 3]>) -> f32 {
    positions.into_iter().map(|position| Vec3::from(position).length()).fold(0.0, f32::max)
}

// The full geometry's indices followed by every simpler copy's, ready for one index buffer, and the copies, simplest last.
pub fn build_simpler_detail_levels(positions: &[[f32; 3]], uvs: &[[f32; 2]], indices: &[u32]) -> (Vec<u32>, Vec<DetailLevel>) {
    let mut indices_with_simpler_detail_levels = indices.to_vec();
    let mut simpler_detail_levels = Vec::new();
    let Ok(geometry_size) = simplify_scale(Positions::from_packed(positions)) else {
        return (indices_with_simpler_detail_levels, simpler_detail_levels);
    };
    // Vertices that share a position may merge across the line between them, as a flat shaded mesh needs, unless their texture
    // coordinates differ there: merging across a texture seam would smear the picture.
    let mut first_uv_of_position: HashMap<[u32; 3], [f32; 2]> = HashMap::new();
    let mut texture_seam_positions: HashSet<[u32; 3]> = HashSet::new();
    for (position, uv) in positions.iter().zip(uvs) {
        let position_bits = position.map(f32::to_bits);
        if *first_uv_of_position.entry(position_bits).or_insert(*uv) != *uv {
            texture_seam_positions.insert(position_bits);
        }
    }
    let vertex_flags: Vec<VertexFlags> = positions
        .iter()
        .map(|position| match texture_seam_positions.contains(&position.map(f32::to_bits)) {
            true => VertexFlags::PROTECT,
            false => VertexFlags::EMPTY,
        })
        .collect();
    let Ok(no_attributes) = Attributes::from_interleaved(&[], positions.len(), 0, 0, 0) else {
        return (indices_with_simpler_detail_levels, simpler_detail_levels);
    };
    let mut workspace = Workspace::default();
    let mut previous_index_count = indices.len();
    for detail_level in 1..=SIMPLER_DETAIL_LEVEL_COUNT {
        // Every copy simplifies the full geometry, so each one's error is measured against the full geometry.
        let settings = SimplifySettings {
            target_index_count: (indices.len() as f32 * SIMPLER_DETAIL_LEVEL_TRIANGLE_SHARE.powi(detail_level as i32)) as usize,
            target_error: 1.0,
            options: SimplifyOptions::PERMISSIVE,
        };
        let Ok(simplified) = simplify_with_attributes(
            indices,
            Positions::from_packed(positions),
            no_attributes,
            &[],
            Some(&vertex_flags),
            settings,
            &mut workspace,
        ) else {
            break;
        };
        if simplified.indices.len() as f32 > previous_index_count as f32 * SIMPLER_DETAIL_LEVEL_MAX_TRIANGLE_SHARE {
            break;
        }
        previous_index_count = simplified.indices.len();
        let start = indices_with_simpler_detail_levels.len() as u32;
        indices_with_simpler_detail_levels.extend_from_slice(&simplified.indices);
        simpler_detail_levels.push(DetailLevel {
            index_range: start..indices_with_simpler_detail_levels.len() as u32,
            error: simplified.error * geometry_size,
        });
    }
    (indices_with_simpler_detail_levels, simpler_detail_levels)
}

// The simplest detail level whose error would cover at most DETAIL_LEVEL_MAX_ERROR_PIXELS, judged where the instance's
// bounding sphere comes nearest the camera. Level 0 is the full geometry, and level n is simpler_detail_levels[n - 1].
pub fn detail_level_for_instance(
    simpler_detail_levels: &[DetailLevel],
    bounding_radius: f32,
    model: &[[f32; 4]; 4],
    camera_position: Vec3,
    pixels_per_unit_at_unit_distance: f32,
) -> usize {
    let model = Mat4::from_cols_array_2d(model);
    let scale = model
        .x_axis
        .truncate()
        .length_squared()
        .max(model.y_axis.truncate().length_squared())
        .max(model.z_axis.truncate().length_squared())
        .sqrt();
    let nearest_distance = model.w_axis.truncate().distance(camera_position) - bounding_radius * scale;
    simpler_detail_levels
        .iter()
        .rposition(|detail_level| detail_level.error * scale * pixels_per_unit_at_unit_distance <= DETAIL_LEVEL_MAX_ERROR_PIXELS * nearest_distance)
        .map_or(0, |simpler_index| simpler_index + 1)
}

impl GpuGeometryBuffer {
    pub fn index_range_for_detail_level(&self, detail_level: usize) -> Range<u32> {
        match detail_level {
            0 => 0..self.num_indices,
            _ => self.simpler_detail_levels[detail_level - 1].index_range.clone(),
        }
    }
}
