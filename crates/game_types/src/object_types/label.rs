use crate::object_types::shared::{Color3, ObjectIdPointer, VisualSettings};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use unicode_properties::{EmojiStatus, UnicodeEmoji};

// The authored size of a constant-screen-size object is what you see from this far away.
pub const CONSTANT_SIZE_REFERENCE_DISTANCE: f32 = 10.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BillboardMode {
    #[default]
    None, // draw with the object's own orientation
    Full, // face the camera directly, including pitch
}

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelHorizontalAlign {
    Left,
    #[default]
    Center,
    Right,
}

// ------------ LabelInfo ------------

// Text in the world. Like a Mesh it owns no physics: it follows physicalObjectId.
//
// Orientation depends on visualSettings.billboardMode. Billboarded, it faces the camera and
// offsetFromObject is a world offset. With BillboardMode::None it is stuck to the object like a sticker,
// inheriting its rotation with offsetFromObject measured in the object's own frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LabelInfo {
    pub text: String,          // never holds emoji: labels can't show them, so the world removes them whenever text is set
    pub lineHeight: f32,       // world units (blocks) taken up by one line of text
    pub maxWidth: Option<f32>, // world units to wrap at, or None to never wrap
    pub horizontalAlign: LabelHorizontalAlign,
    pub backgroundColor: Option<Color3>, // drawn behind the text, sized to the laid out text
    pub backgroundOpacity: f32,

    pub offsetFromObject: Vec3,
    // Only meaningful when not billboarding; picks which face of a block the label sits on.
    #[serde(default)]
    pub rotationFromObject: Quat,
    pub physicalObjectId: Option<ObjectIdPointer>, // follow this object's location (same idea as MeshInfo)

    pub billboardMode: BillboardMode,
    // Authored size is what you see at CONSTANT_SIZE_REFERENCE_DISTANCE blocks away.
    pub isConstantSize: bool,
    pub visualSettings: VisualSettings,
}

impl LabelInfo {
    pub const DEFAULT_LINE_HEIGHT: f32 = 0.5;

    // A character is removed when it shows as an emoji by default, when the U+FE0F after it asks for its emoji look, or when
    // it only joins emoji together: the zero width joiner, the keycap mark, U+FE0F itself, and the tag characters. Symbols
    // that only look like emoji when asked to, like (c) and digits, stay as text.
    pub fn text_without_emoji(text: &str) -> String {
        let mut characters = text.chars().peekable();
        let mut kept_text = String::with_capacity(text.len());
        while let Some(character) = characters.next() {
            let is_emoji = match character.emoji_status() {
                EmojiStatus::EmojiPresentation
                | EmojiStatus::EmojiPresentationAndModifierBase
                | EmojiStatus::EmojiPresentationAndEmojiComponent
                | EmojiStatus::EmojiPresentationAndModifierAndEmojiComponent
                | EmojiStatus::NonEmojiButEmojiComponent => true,
                EmojiStatus::NonEmoji => false,
                _ => characters.peek() == Some(&'\u{FE0F}'),
            };
            if !is_emoji {
                kept_text.push(character);
            }
        }
        kept_text
    }
}

pub struct LabelInfoTrackedFields {
    pub physicalObjectId: Option<ObjectIdPointer>,
}
