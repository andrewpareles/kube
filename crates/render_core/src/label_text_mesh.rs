// The only file that knows epaint exists. Text goes in, plain vertices and a plain distance field atlas come out,
// so the renderer never deals with fonts and an epaint upgrade can only break here.
//
// Every letter is laid out and rasterized once, at LABEL_FONT_SIZE, and turned into a distance field: each texel holds how
// far it is from the letter's edge. The shader draws the edge from that at any size, softened across exactly one screen
// pixel, so a label stays sharp from far away to right up close.

use crate::fonts::build_font_definitions;
use crate::render_scene::{FontAtlas, LabelGeometry, VertexContent};
use epaint::emath::Align;
use epaint::text::{FontFamily, FontId, Fonts, Glyph, LayoutJob, TextFormat};
use epaint::{AlphaFromCoverage, Color32, Galley};
use game_types::object_types::{LabelHorizontalAlign, LabelInfo};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex};

// TODO: AI-PICKED-VALUE: Labels are laid out and rasterized at this one size, and every size on screen is drawn from it.
const LABEL_FONT_SIZE: f32 = 64.0;
// TODO: AI-PICKED-VALUE: How far a letter's distance field reaches past its edge, in atlas texels. Keep this synced with
// LABEL_DISTANCE_FIELD_RADIUS in scene.wgsl. Small text needs this much room for its softened edge.
const DISTANCE_FIELD_RADIUS: usize = 8;
// TODO: AI-PICKED-VALUE: The atlas is this wide and grows taller as letters arrive.
const DISTANCE_ATLAS_WIDTH: usize = 1024;
const DISTANCE_ATLAS_START_HEIGHT: usize = 256;
// Each coarser level averages 2x2 texels of the one before. The last of these averages 8x8 blocks, which still fit inside
// the DISTANCE_FIELD_RADIUS of empty space around every letter, so no letter blends into its neighbor.
const DISTANCE_ATLAS_MIP_LEVEL_COUNT: usize = 4;
// TODO: AI-PICKED-VALUE: Letters rasterize at this many times the size the atlas stores them. Their distances are measured at
// that detail and then shrunk to the atlas's size, so a letter drawn far bigger than the atlas keeps smooth curves.
const RASTER_DETAIL: usize = 4;
const LABEL_PIXELS_PER_POINT: f32 = RASTER_DETAIL as f32;
const LABEL_MAX_TEXTURE_SIDE: usize = 8192;
// How many frames a piece of text can go unused before its vertex data is dropped.
const IDLE_GEOMETRY_FRAME_LIMIT: u64 = 240;
const BACKGROUND_PADDING_X_RATIO: f32 = 0.30;
const BACKGROUND_PADDING_Y_RATIO: f32 = 0.12;
// Coplanar with the glyphs on purpose: a label writes no depth until both are drawn, so what puts the glyphs
// on top is being drawn second. An offset would only give the two surfaces depths to disagree about.
const BACKGROUND_DEPTH_OFFSET: f32 = 0.0;

#[derive(Clone, PartialEq, Eq, Hash)]
struct LabelGeometryKey {
    text: String,
    wrap_width_bits: u32,
    horizontal_align: LabelHorizontalAlign,
    atlas_version: u64,
}

// One cache for the process: building a font stack is expensive and every world label uses it.
static LABEL_TEXT_MESH_CACHE: LazyLock<Mutex<LabelTextMeshCache>> = LazyLock::new(|| Mutex::new(LabelTextMeshCache::new()));

// Lays every label out first, folds the resulting glyphs into the atlas, and only then builds geometry,
// so no label can hold UVs into an atlas that has since been rebuilt. One mesh per input, in order.
pub fn label_meshes_of_label_infos(label_infos: &[LabelInfo]) -> Vec<LabelMesh> {
    let mut cache = match LABEL_TEXT_MESH_CACHE.lock() {
        Ok(cache) => cache,
        Err(poisoned) => poisoned.into_inner(),
    };
    cache.begin_frame();
    for label_info in label_infos {
        cache.add_letters_of_label(label_info);
    }
    cache.finish_frame_atlas();
    label_infos.iter().map(|label_info| cache.label_mesh(label_info)).collect()
}

// -------------------------- Distance fields --------------------------

// Mapbox's TinySDF way of turning coverage into a distance field (https://github.com/mapbox/tiny-sdf): each pixel's squared
// distance to the nearest pixel inside the letter, and to the nearest one outside it, from two distance transforms. A partly
// covered pixel starts that far from the edge already. The coverage is RASTER_DETAIL times the atlas's size, and each atlas
// texel takes the distance at the middle of its block of raster pixels. The result has DISTANCE_FIELD_RADIUS of empty space
// on every side, and stores 0.5 on the edge, more inside, and less outside, reaching 0 and 1 at DISTANCE_FIELD_RADIUS away.
// Returns the distances with their width and height.
fn distance_field_of_coverage(coverage: &[u8], raster_width: usize, raster_height: usize) -> (Vec<u8>, usize, usize) {
    const FAR: f64 = 1e20;

    // Felzenszwalb and Huttenlocher's distance transform along one line of the grid: each value becomes the smallest
    // value at any other spot plus the squared distance to that spot.
    fn transform_line(grid: &mut [f64], offset: usize, stride: usize, length: usize) {
        let mut values = vec![0.0; length];
        let mut parabola_spots = vec![0usize; length];
        let mut boundaries = vec![0.0; length + 1];
        values[0] = grid[offset];
        boundaries[0] = -FAR;
        boundaries[1] = FAR;
        let mut last = 0;
        for spot in 1..length {
            values[spot] = grid[offset + spot * stride];
            let crossing_with = |other: usize| (values[spot] - values[other] + (spot * spot) as f64 - (other * other) as f64) / (spot - other) as f64 / 2.0;
            let mut crossing = crossing_with(parabola_spots[last]);
            // The first boundary is far to the left, so this never steps back past the first parabola.
            while crossing <= boundaries[last] {
                last -= 1;
                crossing = crossing_with(parabola_spots[last]);
            }
            last += 1;
            parabola_spots[last] = spot;
            boundaries[last] = crossing;
            boundaries[last + 1] = FAR;
        }
        let mut parabola = 0;
        for spot in 0..length {
            while boundaries[parabola + 1] < spot as f64 {
                parabola += 1;
            }
            let distance = spot as f64 - parabola_spots[parabola] as f64;
            grid[offset + spot * stride] = values[parabola_spots[parabola]] + distance * distance;
        }
    }
    fn transform(grid: &mut [f64], width: usize, height: usize) {
        for x in 0..width {
            transform_line(grid, x, width, height);
        }
        for y in 0..height {
            transform_line(grid, y * width, 1, width);
        }
    }

    let width = raster_width.div_ceil(RASTER_DETAIL) + 2 * DISTANCE_FIELD_RADIUS;
    let height = raster_height.div_ceil(RASTER_DETAIL) + 2 * DISTANCE_FIELD_RADIUS;
    let padded_width = width * RASTER_DETAIL;
    let padded_height = height * RASTER_DETAIL;
    let padding = DISTANCE_FIELD_RADIUS * RASTER_DETAIL;
    let mut squared_distance_outside = vec![FAR; padded_width * padded_height];
    let mut squared_distance_inside = vec![0.0; padded_width * padded_height];
    for y in 0..raster_height {
        for x in 0..raster_width {
            let index = (y + padding) * padded_width + x + padding;
            (squared_distance_outside[index], squared_distance_inside[index]) = match coverage[y * raster_width + x] {
                0 => (FAR, 0.0),
                u8::MAX => (0.0, FAR),
                partial_coverage => {
                    let distance_outside_edge = 0.5 - partial_coverage as f64 / 255.0;
                    (distance_outside_edge.max(0.0).powi(2), (-distance_outside_edge).max(0.0).powi(2))
                }
            };
        }
    }
    transform(&mut squared_distance_outside, padded_width, padded_height);
    transform(&mut squared_distance_inside, padded_width, padded_height);
    // The middle of a block of RASTER_DETAIL raster pixels lies between its two middle rows and columns.
    let middle = RASTER_DETAIL / 2;
    let distances = (0..height)
        .flat_map(|y| (0..width).map(move |x| (y, x)))
        .map(|(y, x)| {
            let mut raster_distance_outside_edge = 0.0;
            for raster_y in y * RASTER_DETAIL + middle - 1..=y * RASTER_DETAIL + middle {
                for raster_x in x * RASTER_DETAIL + middle - 1..=x * RASTER_DETAIL + middle {
                    let index = raster_y * padded_width + raster_x;
                    raster_distance_outside_edge += (squared_distance_outside[index].sqrt() - squared_distance_inside[index].sqrt()) / 4.0;
                }
            }
            let distance_outside_edge = raster_distance_outside_edge / RASTER_DETAIL as f64;
            ((0.5 - distance_outside_edge / (2 * DISTANCE_FIELD_RADIUS) as f64).clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect();
    (distances, width, height)
}

// Letters packed in rows, each with its DISTANCE_FIELD_RADIUS of empty space. The top left corner holds a block that is deep
// inside everywhere, which the background panel samples.
struct DistanceFieldAtlas {
    height: usize,
    distances: Vec<u8>,
    row_x: usize,
    row_y: usize,
    row_height: usize,
    // Keyed by where epaint rasterized the letter, as [min x, min y, max x, max y]. Valued as [x, y, width, height] here.
    rect_of_raster_rect: HashMap<[u16; 4], [usize; 4]>,
}

impl DistanceFieldAtlas {
    fn new() -> Self {
        let block_side = 2 * DISTANCE_FIELD_RADIUS;
        let mut distances = vec![0; DISTANCE_ATLAS_WIDTH * DISTANCE_ATLAS_START_HEIGHT];
        for y in 0..block_side {
            distances[y * DISTANCE_ATLAS_WIDTH..y * DISTANCE_ATLAS_WIDTH + block_side].fill(u8::MAX);
        }
        Self {
            height: DISTANCE_ATLAS_START_HEIGHT,
            distances,
            row_x: block_side,
            row_y: 0,
            row_height: block_side,
            rect_of_raster_rect: HashMap::new(),
        }
    }

    fn insert(&mut self, raster_rect: [u16; 4], distances: &[u8], width: usize, height: usize) {
        if self.row_x + width > DISTANCE_ATLAS_WIDTH {
            self.row_x = 0;
            self.row_y += self.row_height;
            self.row_height = 0;
        }
        while self.row_y + height > self.height {
            self.height *= 2;
            self.distances.resize(DISTANCE_ATLAS_WIDTH * self.height, 0);
        }
        for y in 0..height {
            let start = (self.row_y + y) * DISTANCE_ATLAS_WIDTH + self.row_x;
            self.distances[start..start + width].copy_from_slice(&distances[y * width..(y + 1) * width]);
        }
        self.rect_of_raster_rect.insert(raster_rect, [self.row_x, self.row_y, width, height]);
        self.row_x += width;
        self.row_height = self.row_height.max(height);
    }

    fn font_atlas(&self, version: u64) -> FontAtlas {
        let mut mip_levels = vec![self.distances.clone()];
        let (mut width, mut height) = (DISTANCE_ATLAS_WIDTH, self.height);
        for _ in 1..DISTANCE_ATLAS_MIP_LEVEL_COUNT {
            let finer = &mip_levels[mip_levels.len() - 1];
            let (coarser_width, coarser_height) = (width / 2, height / 2);
            let mut coarser = Vec::with_capacity(coarser_width * coarser_height);
            for y in 0..coarser_height {
                for x in 0..coarser_width {
                    let top = 2 * y * width + 2 * x;
                    let bottom = top + width;
                    let sum = finer[top] as u32 + finer[top + 1] as u32 + finer[bottom] as u32 + finer[bottom + 1] as u32;
                    coarser.push(((sum + 2) / 4) as u8);
                }
            }
            mip_levels.push(coarser);
            (width, height) = (coarser_width, coarser_height);
        }
        FontAtlas {
            version,
            width: DISTANCE_ATLAS_WIDTH as u32,
            height: self.height as u32,
            mip_levels,
        }
    }
}

fn raster_rect_of_glyph(glyph: &Glyph) -> [u16; 4] {
    [glyph.uv_rect.min[0], glyph.uv_rect.min[1], glyph.uv_rect.max[0], glyph.uv_rect.max[1]]
}

// -------------------------- Label layout and geometry --------------------------

pub struct LabelTextMeshCache {
    fonts: Fonts,
    distance_field_atlas: DistanceFieldAtlas,
    is_distance_field_atlas_changed: bool,
    atlas: Arc<FontAtlas>,
    geometry_of_key: HashMap<LabelGeometryKey, CachedLabelGeometry>,
    frame_index: u64,
}

struct CachedLabelGeometry {
    geometry: Arc<LabelGeometry>,
    last_used_frame_index: u64,
}

pub struct LabelMesh {
    pub geometry: Arc<LabelGeometry>,
    pub atlas: Arc<FontAtlas>,
    pub scale_per_point: f32,
}

impl LabelTextMeshCache {
    pub fn new() -> Self {
        // Coverage straight from the outline, so a half covered pixel sits exactly on the letter's edge.
        let fonts = Fonts::new(LABEL_MAX_TEXTURE_SIDE, AlphaFromCoverage::Linear, build_font_definitions());
        let distance_field_atlas = DistanceFieldAtlas::new();
        let atlas = Arc::new(distance_field_atlas.font_atlas(0));
        Self {
            fonts,
            distance_field_atlas,
            is_distance_field_atlas_changed: false,
            atlas,
            geometry_of_key: HashMap::new(),
            frame_index: 0,
        }
    }

    // epaint starts its atlas over once it is nearly full, which moves every letter in it, so the distance fields start
    // over too. Its atlas only ever fills up otherwise, so a lower fill ratio means it started over.
    pub fn begin_frame(&mut self) {
        self.frame_index += 1;
        let fill_ratio_before = self.fonts.font_atlas_fill_ratio();
        self.fonts.begin_pass(LABEL_MAX_TEXTURE_SIDE, AlphaFromCoverage::Linear);
        if self.fonts.font_atlas_fill_ratio() < fill_ratio_before {
            self.distance_field_atlas = DistanceFieldAtlas::new();
            self.is_distance_field_atlas_changed = true;
        }
    }

    // Pass 1: laying out is what rasterizes letters into epaint's atlas. Each letter not seen before gets its distance
    // field made from that coverage. epaint memoizes the layout, so pass 2 is a lookup.
    pub fn add_letters_of_label(&mut self, label_info: &LabelInfo) {
        let galley = self.layout_galley_rect(label_info);
        for placed_row in &galley.rows {
            for glyph in &placed_row.row.glyphs {
                let raster_rect = raster_rect_of_glyph(glyph);
                if glyph.uv_rect.is_nothing() || self.distance_field_atlas.rect_of_raster_rect.contains_key(&raster_rect) {
                    continue;
                }
                let [min_x, min_y, max_x, max_y] = raster_rect.map(usize::from);
                let font_image = self.fonts.texture_atlas().image();
                let font_image_width = font_image.width();
                let coverage: Vec<u8> = (min_y..max_y)
                    .flat_map(|y| (min_x..max_x).map(move |x| (y, x)))
                    .map(|(y, x)| font_image.pixels[y * font_image_width + x].a())
                    .collect();
                let (distances, width, height) = distance_field_of_coverage(&coverage, max_x - min_x, max_y - min_y);
                self.distance_field_atlas.insert(raster_rect, &distances, width, height);
                self.is_distance_field_atlas_changed = true;
            }
        }
    }

    // Geometry is keyed by atlas version, since growing the atlas moves every letter's UVs, so anything built against an
    // older atlas is rebuilt.
    pub fn finish_frame_atlas(&mut self) {
        if self.is_distance_field_atlas_changed {
            self.atlas = Arc::new(self.distance_field_atlas.font_atlas(self.atlas.version + 1));
            self.is_distance_field_atlas_changed = false;
        }

        // Idle geometry lingers rather than being dropped: several views share this cache, and evicting
        // whatever this frame's caller did not ask for would make them rebuild each other's text.
        let atlas_version = self.atlas.version;
        let frame_index = self.frame_index;
        self.geometry_of_key.retain(|key, cached_label_geometry| {
            key.atlas_version == atlas_version && frame_index.saturating_sub(cached_label_geometry.last_used_frame_index) <= IDLE_GEOMETRY_FRAME_LIMIT
        });
    }

    pub fn label_mesh(&mut self, label_info: &LabelInfo) -> LabelMesh {
        let key = self.geometry_key(label_info);
        let scale_per_point = scale_per_point_for(label_info);
        let frame_index = self.frame_index;
        if let Some(cached_label_geometry) = self.geometry_of_key.get_mut(&key) {
            cached_label_geometry.last_used_frame_index = frame_index;
            return LabelMesh {
                geometry: cached_label_geometry.geometry.clone(),
                atlas: self.atlas.clone(),
                scale_per_point,
            };
        }

        let geometry = Arc::new(self.build_geometry(label_info, &key));
        self.geometry_of_key.insert(
            key,
            CachedLabelGeometry {
                geometry: geometry.clone(),
                last_used_frame_index: frame_index,
            },
        );
        LabelMesh {
            geometry,
            atlas: self.atlas.clone(),
            scale_per_point,
        }
    }

    fn geometry_key(&self, label_info: &LabelInfo) -> LabelGeometryKey {
        LabelGeometryKey {
            text: label_info.text.clone(),
            wrap_width_bits: wrap_width_for(label_info).to_bits(),
            horizontal_align: label_info.horizontalAlign,
            atlas_version: self.atlas.version,
        }
    }

    fn layout_galley_rect(&mut self, label_info: &LabelInfo) -> Arc<Galley> {
        let mut layout_job = LayoutJob::single_section(
            label_info.text.clone(),
            TextFormat::simple(FontId::new(LABEL_FONT_SIZE, FontFamily::Proportional), Color32::WHITE),
        );
        layout_job.wrap.max_width = wrap_width_for(label_info);
        layout_job.halign = match label_info.horizontalAlign {
            LabelHorizontalAlign::Left => Align::Min,
            LabelHorizontalAlign::Center => Align::Center,
            LabelHorizontalAlign::Right => Align::Max,
        };
        // GUI pixel rounding is for crisp screen text; in the world it just quantizes layout.
        layout_job.round_output_to_gui = false;
        self.fonts.with_pixels_per_point(LABEL_PIXELS_PER_POINT).layout_job(layout_job)
    }

    fn build_geometry(&mut self, label_info: &LabelInfo, key: &LabelGeometryKey) -> LabelGeometry {
        let galley = self.layout_galley_rect(label_info);
        let atlas_width = self.atlas.width.max(1) as f32;
        let atlas_height = self.atlas.height.max(1) as f32;

        // epaint lays out with y growing downward and the block starting at y = 0. The world wants y up
        // and the label centered on the object it follows, so flip y and recenter on the block's middle.
        let galley_rect = galley.rect;
        let center_x = galley_rect.center().x;
        let center_y = galley_rect.center().y;
        let to_world = |x: f32, y: f32| -> (f32, f32) { (x - center_x, -(y - center_y)) };

        fn push_quad(
            vertices: &mut Vec<VertexContent>,
            indices: &mut Vec<u32>,
            (min_x, min_y): (f32, f32),
            (max_x, max_y): (f32, f32),
            uv_min: [f32; 2],
            uv_max: [f32; 2],
            z: f32,
        ) {
            let index_offset = vertices.len() as u32;
            for (x, y, u, v) in [
                (min_x, min_y, uv_min[0], uv_max[1]),
                (max_x, min_y, uv_max[0], uv_max[1]),
                (max_x, max_y, uv_max[0], uv_min[1]),
                (min_x, max_y, uv_min[0], uv_min[1]),
            ] {
                vertices.push(VertexContent {
                    position: [x, y, z],
                    normal: [0.0, 0.0, 1.0],
                    uv: [u, v],
                    tangent: [1.0, 0.0, 0.0, 1.0],
                });
            }
            indices.extend([0, 1, 2, 0, 2, 3].map(|index| index + index_offset));
        }
        let mut vertices: Vec<VertexContent> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        // Panel first so the draw can address it as a leading index range. It samples the block at the atlas's top left
        // corner, which is deep inside everywhere.
        let background_padding_x = LABEL_FONT_SIZE * BACKGROUND_PADDING_X_RATIO;
        let background_padding_y = LABEL_FONT_SIZE * BACKGROUND_PADDING_Y_RATIO;
        let inside_uv = [DISTANCE_FIELD_RADIUS as f32 / atlas_width, DISTANCE_FIELD_RADIUS as f32 / atlas_height];
        push_quad(
            &mut vertices,
            &mut indices,
            to_world(galley_rect.min.x - background_padding_x, galley_rect.max.y + background_padding_y),
            to_world(galley_rect.max.x + background_padding_x, galley_rect.min.y - background_padding_y),
            inside_uv,
            inside_uv,
            -BACKGROUND_DEPTH_OFFSET,
        );
        let background_index_count = indices.len() as u32;

        // Each letter's quad starts at its raster's corner moved out by the distance field's empty space, which its edge
        // softens into when it is drawn small, and spans its distance field at one atlas texel per point.
        let padding = DISTANCE_FIELD_RADIUS as f32;
        for placed_row in &galley.rows {
            let row_origin = placed_row.pos;
            let row_vertices = &placed_row.row.visuals.mesh.vertices;
            for glyph in &placed_row.row.glyphs {
                if glyph.uv_rect.is_nothing() {
                    continue;
                }
                let Some(&[atlas_x, atlas_y, atlas_rect_width, atlas_rect_height]) =
                    self.distance_field_atlas.rect_of_raster_rect.get(&raster_rect_of_glyph(glyph))
                else {
                    continue;
                };
                let corners = &row_vertices[glyph.first_vertex as usize..glyph.first_vertex as usize + 4];
                let min_x = corners.iter().map(|vertex| vertex.pos.x).fold(f32::INFINITY, f32::min) - padding;
                let min_y = corners.iter().map(|vertex| vertex.pos.y).fold(f32::INFINITY, f32::min) - padding;
                let max_x = min_x + atlas_rect_width as f32;
                let max_y = min_y + atlas_rect_height as f32;
                push_quad(
                    &mut vertices,
                    &mut indices,
                    to_world(row_origin.x + min_x, row_origin.y + max_y),
                    to_world(row_origin.x + max_x, row_origin.y + min_y),
                    [atlas_x as f32 / atlas_width, atlas_y as f32 / atlas_height],
                    [
                        (atlas_x + atlas_rect_width) as f32 / atlas_width,
                        (atlas_y + atlas_rect_height) as f32 / atlas_height,
                    ],
                    0.0,
                );
            }
        }

        LabelGeometry {
            cache_key: hash_of_key(key),
            vertices,
            indices,
            background_index_count,
        }
    }
}

fn scale_per_point_for(label_info: &LabelInfo) -> f32 {
    let line_height = if label_info.lineHeight.is_finite() && label_info.lineHeight > 0.0 {
        label_info.lineHeight
    } else {
        LabelInfo::DEFAULT_LINE_HEIGHT
    };
    line_height / LABEL_FONT_SIZE
}

// maxWidth is in world units but layout happens in points.
fn wrap_width_for(label_info: &LabelInfo) -> f32 {
    match label_info.maxWidth {
        Some(max_width) if max_width.is_finite() && max_width > 0.0 => max_width / scale_per_point_for(label_info),
        _ => f32::INFINITY,
    }
}

fn hash_of_key(key: &LabelGeometryKey) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}
