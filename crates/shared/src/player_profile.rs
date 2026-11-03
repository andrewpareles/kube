use crate::postgrest;
use game_types::object_types::shared::Color3;
use serde::{Deserialize, Serialize};

// The parts of a kube.new profile that a game can read. They're copied into the player's PlayerInfo when they join.
#[derive(Serialize, Deserialize)]
pub struct PlayerProfile {
    pub preferredColor: Option<Color3>,
    pub equippedItemId: Option<i64>,
}
impl PlayerProfile {
    pub const GUEST: Self = Self {
        preferredColor: None,
        equippedItemId: None,
    };

    // Reads the preferred_color and equipped_catalog_item_id columns of a user_profiles row.
    pub fn from_profile_row(preferred_color: Option<String>, equipped_catalog_item_id: Option<i64>) -> anyhow::Result<Self> {
        // The color is stored as #rrggbb, which is how the editor's color picker shows a Color3.
        let preferredColor = match preferred_color {
            Some(preferred_color) => {
                let color_channel = |start: usize| -> anyhow::Result<f32> {
                    let hex = preferred_color
                        .get(start..start + 2)
                        .ok_or_else(|| anyhow::anyhow!("Bad preferred color {preferred_color}"))?;
                    Ok(u8::from_str_radix(hex, 16)? as f32 / 255.0)
                };
                Some(Color3 {
                    r: color_channel(1)?,
                    g: color_channel(3)?,
                    b: color_channel(5)?,
                })
            }
            None => None,
        };
        Ok(Self {
            preferredColor,
            equippedItemId: equipped_catalog_item_id,
        })
    }
}

pub async fn get_player_profile(user_id: i64) -> anyhow::Result<PlayerProfile> {
    #[derive(Debug, Deserialize)]
    struct ProfileRow {
        preferred_color: Option<String>,
        equipped_catalog_item_id: Option<i64>,
    }
    let Some(ProfileRow {
        preferred_color,
        equipped_catalog_item_id,
    }) = postgrest::from("user_profiles")
        .select("preferred_color,equipped_catalog_item_id")
        .eq("user_id", user_id)
        .get::<ProfileRow>()
        .await?
        .into_iter()
        .next()
    else {
        anyhow::bail!("No user_profiles row where user_id is {user_id}");
    };
    PlayerProfile::from_profile_row(preferred_color, equipped_catalog_item_id)
}
