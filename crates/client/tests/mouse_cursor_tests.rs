use anyhow::Result;
use client::play_runner::mouse_cursor::{MOUSE_CURSOR_PIXELS_PER_POINT_LIST, make_mouse_cursor_picture};
use shared::game::jsRuntime::{BuiltinMouseCursor, MouseCursor, MouseCursorImage};
use shared::game::texture_converter::convert_rgba8_pixels_to_texture_data;

// The window gets every cursor at each number of pixels per point its displays draw at. The built-in ones are drawn at 8, so
// their size in points comes from their pictures.
#[test]
fn builtin_mouse_cursors_are_made_at_every_pixels_per_point_the_window_draws_at() -> Result<()> {
    for builtin_mouse_cursor in [BuiltinMouseCursor::Default, BuiltinMouseCursor::Pointer, BuiltinMouseCursor::Crosshair] {
        let picture = make_mouse_cursor_picture(MouseCursor::Builtin(builtin_mouse_cursor), None)?;
        let sizes: Vec<(u32, u32)> = picture.rgba_images.iter().map(|rgba_image| rgba_image.dimensions()).collect();
        let size_at = |pixels_per_point: f32| {
            (
                (picture.width_points * pixels_per_point).round() as u32,
                (picture.height_points * pixels_per_point).round() as u32,
            )
        };
        let expected_sizes: Vec<(u32, u32)> = MOUSE_CURSOR_PIXELS_PER_POINT_LIST
            .iter()
            .map(|pixels_per_point| size_at(*pixels_per_point))
            .collect();
        assert_eq!(sizes, expected_sizes, "{builtin_mouse_cursor:?}");
        assert!(
            (0.0..=picture.width_points).contains(&picture.hotspot_x_points) && (0.0..=picture.height_points).contains(&picture.hotspot_y_points),
            "{builtin_mouse_cursor:?}'s point that clicks is off its picture"
        );
    }
    Ok(())
}

// A texture's see-through pixels are black, and resizing without weighting colors by alpha blends that black into the
// picture's edges, which outlines a cursor in a dark fringe.
#[test]
fn a_texture_cursor_keeps_its_edge_colors_when_resized() -> Result<()> {
    let pixels = (0..64 * 64)
        .flat_map(|index| match ((16..48).contains(&(index % 64)), (16..48).contains(&(index / 64))) {
            (true, true) => [255, 0, 0, 255],
            _ => [0, 0, 0, 0],
        })
        .collect();
    let texture_data = convert_rgba8_pixels_to_texture_data(pixels, 64, 64, true)?;
    let mouse_cursor_image = MouseCursorImage {
        textureDataId: 1,
        width: 13.0,
        height: 13.0,
        hotspotX: 0.5,
        hotspotY: 0.5,
    };
    let picture = make_mouse_cursor_picture(MouseCursor::Image(mouse_cursor_image), Some(&texture_data))?;
    assert_eq!((picture.hotspot_x_points, picture.hotspot_y_points), (6.5, 6.5));
    for rgba_image in &picture.rgba_images {
        let edge_pixels: Vec<_> = rgba_image.pixels().filter(|pixel| (1..255).contains(&pixel[3])).collect();
        assert!(
            !edge_pixels.is_empty(),
            "13 points doesn't line up with the square's edges, so some pixels are part see-through"
        );
        for edge_pixel in edge_pixels {
            assert!(edge_pixel[0] >= 240, "an edge pixel darkened to {edge_pixel:?}");
        }
    }
    Ok(())
}
