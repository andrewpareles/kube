//! Culling: every pass leaves out the instances it cannot show. The camera's passes leave out instances outside the camera's
//! view. Each shadow cascade leaves out casters outside its square, and casters too small for its texels to show. The
//! cascades are squares around one center, each larger than the last, so the cascades a caster casts into always run from
//! the smallest square it reaches to the largest cascade it is wide enough for.

use crate::render_scene::SHADOW_CASCADE_COUNT;
use glam::{Mat4, Vec3, Vec4};
use std::ops::Range;

// TODO: AI-PICKED-VALUE: A cascade leaves out casters narrower than one of its texels, whose shadows the blur spreads into
// faint smudges. At two texels, faraway shadows faded in game 381's overview by up to 22 levels out of 255. At one, its
// pictures match within run-to-run noise.
pub const SHADOW_CASTER_MIN_TEXELS: f32 = 1.0;

// Casters sort into one bucket for each pair of the smallest cascade they reach and the largest cascade they show in.
pub const SHADOW_CASTER_BUCKET_COUNT: usize = SHADOW_CASCADE_COUNT * SHADOW_CASCADE_COUNT;

// Where each bucket's casters start in an instance buffer, and then where the last bucket's end.
pub type ShadowCasterBucketStarts = [u32; SHADOW_CASTER_BUCKET_COUNT + 1];

#[derive(Clone, Copy, Debug)]
pub struct BoundingSphere {
    pub center: Vec3,
    pub radius: f32,
}

// The most a model stretches any direction. The sum of one axis's dot products with every axis bounds it, and is exactly
// that axis's length whenever the axes are perpendicular, as they are for anything only rotated and scaled.
pub fn largest_stretch(model: Mat4) -> f32 {
    let axes = [model.x_axis.truncate(), model.y_axis.truncate(), model.z_axis.truncate()];
    axes.iter()
        .map(|axis| axes.iter().map(|other_axis| axis.dot(*other_axis).abs()).sum::<f32>())
        .fold(0.0, f32::max)
        .sqrt()
}

// The four edges of the camera's picture, as planes facing in. They meet at the camera, so they also leave out everything
// behind it.
pub struct ViewEdges([Vec4; 4]);

impl ViewEdges {
    pub fn of_view_proj(view_proj: Mat4) -> Self {
        let [row_x, row_y, row_w] = [view_proj.row(0), view_proj.row(1), view_proj.row(3)];
        Self([row_w + row_x, row_w - row_x, row_w + row_y, row_w - row_y].map(|edge| edge / edge.truncate().length()))
    }

    pub fn contains(&self, sphere: BoundingSphere) -> bool {
        self.0.iter().all(|edge| edge.truncate().dot(sphere.center) + edge.w >= -sphere.radius)
    }
}

// Each shadow cascade's square, as the rows of its view projection that place a point across and up it, and the narrowest
// caster the cascade draws.
pub struct ShadowCascadeSquares {
    across_and_up_rows: [(Vec4, Vec4); SHADOW_CASCADE_COUNT],
    inverse_half_sizes: [f32; SHADOW_CASCADE_COUNT],
    min_caster_diameters: [f32; SHADOW_CASCADE_COUNT],
}

impl ShadowCascadeSquares {
    pub fn new(view_projections: &[Mat4; SHADOW_CASCADE_COUNT], half_sizes: [f32; SHADOW_CASCADE_COUNT], shadow_map_size: u32) -> Self {
        Self {
            across_and_up_rows: view_projections.map(|view_projection| (view_projection.row(0), view_projection.row(1))),
            inverse_half_sizes: half_sizes.map(|half_size| 1.0 / half_size),
            min_caster_diameters: half_sizes.map(|half_size| SHADOW_CASTER_MIN_TEXELS * half_size * 2.0 / shadow_map_size as f32),
        }
    }

    // The bucket for the cascades a caster casts into, or None when it casts into none.
    pub fn shadow_caster_bucket(&self, sphere: BoundingSphere) -> Option<usize> {
        let point = sphere.center.extend(1.0);
        let smallest_reached = (0..SHADOW_CASCADE_COUNT).find(|&cascade_index| {
            let (across_row, up_row) = self.across_and_up_rows[cascade_index];
            let reach = 1.0 + sphere.radius * self.inverse_half_sizes[cascade_index];
            across_row.dot(point).abs() <= reach && up_row.dot(point).abs() <= reach
        })?;
        let largest_shown = (smallest_reached..SHADOW_CASCADE_COUNT)
            .rev()
            .find(|&cascade_index| 2.0 * sphere.radius >= self.min_caster_diameters[cascade_index])?;
        Some(shadow_caster_bucket_of_cascades(smallest_reached, largest_shown))
    }
}

// Largest cascade shown first, then smallest cascade reached first, so the casters a cascade draws are one run of buckets for
// each largest cascade at or past it.
fn shadow_caster_bucket_of_cascades(smallest_reached: usize, largest_shown: usize) -> usize {
    (SHADOW_CASCADE_COUNT - 1 - largest_shown) * SHADOW_CASCADE_COUNT + smallest_reached
}

// The ranges of casters one cascade draws, with runs that touch joined into one range.
pub fn shadow_caster_ranges(bucket_starts: &ShadowCasterBucketStarts, cascade_index: usize) -> impl Iterator<Item = Range<u32>> + '_ {
    let mut runs = (cascade_index..SHADOW_CASCADE_COUNT)
        .rev()
        .map(move |largest_shown| {
            let first_bucket = shadow_caster_bucket_of_cascades(0, largest_shown);
            let last_bucket = shadow_caster_bucket_of_cascades(cascade_index, largest_shown);
            bucket_starts[first_bucket]..bucket_starts[last_bucket + 1]
        })
        .filter(|run| !run.is_empty())
        .peekable();
    std::iter::from_fn(move || {
        let mut range = runs.next()?;
        while let Some(next_run) = runs.next_if(|next_run| next_run.start == range.end) {
            range.end = next_run.end;
        }
        Some(range)
    })
}
