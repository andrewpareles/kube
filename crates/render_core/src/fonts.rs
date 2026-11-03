use epaint::text::{FontData, FontDefinitions, FontFamily};
use lucide_icons::LUCIDE_FONT_BYTES;
use std::sync::Arc;

pub const LUCIDE_FONT_FAMILY_NAME: &str = "lucide-icons";
const LUCIDE_FONT_DATA_NAME: &str = LUCIDE_FONT_FAMILY_NAME;
const INTER_VARIABLE_FONT_NAME: &str = "InterVariable";
// The desktop client's webview draws the game GUI in this same font.
pub const INTER_VARIABLE_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/InterVariable.ttf");

// The font stack world labels are drawn with. It starts with Inter, the font the game GUI uses, so
// that text in the world matches text in the UI.
pub fn build_font_definitions() -> FontDefinitions {
    let mut font_definitions = FontDefinitions::default();
    font_definitions
        .font_data
        .insert(INTER_VARIABLE_FONT_NAME.to_owned(), Arc::new(FontData::from_static(INTER_VARIABLE_FONT_BYTES)));
    font_definitions
        .font_data
        .insert(LUCIDE_FONT_DATA_NAME.to_owned(), Arc::new(FontData::from_static(LUCIDE_FONT_BYTES)));
    font_definitions
        .families
        .insert(FontFamily::Name(LUCIDE_FONT_FAMILY_NAME.into()), vec![LUCIDE_FONT_DATA_NAME.to_owned()]);
    font_definitions
        .families
        .get_mut(&FontFamily::Proportional)
        .expect("default proportional font family should exist")
        .insert(0, INTER_VARIABLE_FONT_NAME.to_owned());
    font_definitions
        .families
        .get_mut(&FontFamily::Proportional)
        .expect("default proportional font family should exist")
        .insert(1, LUCIDE_FONT_DATA_NAME.to_owned());
    font_definitions
}
