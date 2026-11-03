use image::{GenericImageView, ImageFormat, RgbaImage};
use linux_sandbox::{MAX_AI_IMAGE_SIDE, shrink_png_for_ai};

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut png = Vec::new();
    RgbaImage::new(width, height)
        .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
        .unwrap();
    png
}

fn png_size(png: &[u8]) -> (u32, u32) {
    image::load_from_memory_with_format(png, ImageFormat::Png).unwrap().dimensions()
}

#[test]
fn images_bigger_than_the_ai_limit_shrink_to_fit_it_and_keep_their_shape() {
    for ((width, height), shown) in [((2048, 384), (2000, 375)), ((1000, 4096), (488, 2000)), ((4096, 4096), (2000, 2000))] {
        let (shrunk, shown_width, shown_height) = shrink_png_for_ai(png(width, height), width, height).unwrap();
        assert_eq!((shown_width, shown_height), shown);
        assert_eq!(png_size(&shrunk), shown);
    }
}

#[test]
fn images_within_the_ai_limit_are_shown_unchanged() {
    for (width, height) in [(640, 281), (MAX_AI_IMAGE_SIDE, 375), (MAX_AI_IMAGE_SIDE, MAX_AI_IMAGE_SIDE)] {
        let original = png(width, height);
        let (shown, shown_width, shown_height) = shrink_png_for_ai(original.clone(), width, height).unwrap();
        assert_eq!(shown, original);
        assert_eq!((shown_width, shown_height), (width, height));
    }
}
