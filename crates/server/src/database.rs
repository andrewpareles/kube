use game_types::GameId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shared::env::env_vars::ARBITRIUM_REQUEST_ID;
use shared::player_profile::PlayerProfile;
use shared::postgrest;
use shared::protocol::messages::GameChatMessage;
use shared::s3_storage::{PrivateFile, download_private_file};
use std::collections::HashMap;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

// -------------------------- Authentication Helpers --------------------------

pub struct ValidatedPlayAuth {
    pub profile_id: String,
    pub user_id: i64,
    pub username: String,
    pub profile: PlayerProfile,
    pub play_auth_hash: String,
}

pub fn hash_desktop_auth_token(desktop_auth_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(desktop_auth_token.as_bytes());
    let hash_bytes = hasher.finalize();
    hash_bytes.iter().map(|hash_byte| format!("{hash_byte:02x}")).collect()
}

pub async fn validate_play_auth(play_auth: &str) -> anyhow::Result<Option<ValidatedPlayAuth>> {
    #[derive(Debug, Deserialize)]
    struct ProfileRow {
        user_id: i64,
        username: String,
        preferred_color: Option<String>,
        equipped_catalog_item_id: Option<i64>,
    }

    #[derive(Debug, Deserialize)]
    struct PlayerAuthProfileRow {
        profile_id: String,
        profile: ProfileRow,
    }

    let play_auth_hash = hash_desktop_auth_token(play_auth);
    let player_auth_rows = postgrest::from("user_desktop_play_auth")
        .select("profile_id,profile:user_profiles!inner(user_id,username,preferred_color,equipped_catalog_item_id)")
        .eq("play_auth_hash", &play_auth_hash)
        .limit(1)
        .get::<PlayerAuthProfileRow>()
        .await?;
    let Some(player_auth_row) = player_auth_rows.into_iter().next() else {
        return Ok(None);
    };

    // The player's running_player_sessions row is their ticket in: the website writes it when they
    // press Play, and no row means no reservation, so no entry. Claiming stamps connected_at on that
    // same row, which is what tells the website they are really here and not just on their way -
    // until this point the website counts them as launching, not playing.
    //
    // Claiming and checking are one call so admission stays a single round trip, and the stamp is
    // written by the database rather than sent from here so connected_at and reserved_at are always
    // on the same clock.
    #[derive(serde::Serialize)]
    struct ClaimRunningPlayerSessionInputs<'a> {
        input_profile_id: &'a str,
        input_request_id: &'a str,
    }

    let did_claim_running_player_session: bool = postgrest::call(
        "claim_running_player_session",
        ClaimRunningPlayerSessionInputs {
            input_profile_id: player_auth_row.profile_id.as_str(),
            input_request_id: ARBITRIUM_REQUEST_ID.as_str(),
        },
    )
    .await?;
    if !did_claim_running_player_session {
        return Ok(None);
    }

    let ProfileRow {
        user_id,
        username,
        preferred_color,
        equipped_catalog_item_id,
    } = player_auth_row.profile;
    Ok(Some(ValidatedPlayAuth {
        profile_id: player_auth_row.profile_id,
        user_id,
        username,
        profile: PlayerProfile::from_profile_row(preferred_color, equipped_catalog_item_id)?,
        play_auth_hash,
    }))
}

pub async fn get_play_auth_hash_of_profile_id_batch<'a>(profile_ids: impl IntoIterator<Item = &'a str>) -> anyhow::Result<HashMap<String, String>> {
    #[derive(Debug, Deserialize)]
    struct PlayerAuthRow {
        profile_id: String,
        play_auth_hash: Option<String>,
    }

    let profile_ids = profile_ids.into_iter().collect::<Vec<_>>();
    if profile_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let player_auth_rows = postgrest::from("user_desktop_play_auth")
        .select("profile_id,play_auth_hash")
        .is_in("profile_id", profile_ids)
        .get::<PlayerAuthRow>()
        .await?;
    let play_auth_hash_of_profile_id = player_auth_rows
        .into_iter()
        .filter_map(|player_auth_row| {
            let play_auth_hash = player_auth_row.play_auth_hash?;
            Some((player_auth_row.profile_id, play_auth_hash))
        })
        .collect::<HashMap<_, _>>();
    Ok(play_auth_hash_of_profile_id)
}

// -------------------------- Published Worlds --------------------------

#[derive(Debug, Deserialize)]
struct PublishedWorldRow {
    world_uuid: String,
}

pub async fn download_published_world_or_none(game_id: GameId, game_version: i64) -> anyhow::Result<Option<String>> {
    let rows = postgrest::from("game_worlds")
        .select("world_uuid")
        .eq("game_id", game_id)
        .eq("version_num", game_version)
        .limit(1)
        .get::<PublishedWorldRow>()
        .await?;
    match rows.into_iter().next() {
        Some(row) => {
            let world_uuid = uuid::Uuid::parse_str(&row.world_uuid)?;
            Ok(Some(String::from_utf8(download_private_file(&PrivateFile::GameWorld { world_uuid }).await?)?))
        }
        None => Ok(None),
    }
}

pub async fn download_published_world(game_id: GameId, game_version: i64) -> anyhow::Result<String> {
    let stored_world_string = download_published_world_or_none(game_id, game_version).await?;
    let Some(stored_world_string) = stored_world_string else {
        return Err(anyhow::anyhow!("No gameworld row found for game_id {game_id} ({game_version})"));
    };
    Ok(stored_world_string)
}

// -------------------------- Player Session Sync --------------------------

// This deletes the player's reserved running session row, deleteing the entry only if it has the same play_auth_hash as specified.
pub async fn end_player_session(profile_id: &str, play_auth_hash: &str) -> anyhow::Result<()> {
    #[derive(serde::Serialize)]
    struct DeleteRunningPlayerSessionInputs<'a> {
        input_profile_id: &'a str,
        input_request_id: &'a str,
        input_play_auth_hash: &'a str,
    }

    postgrest::call::<()>(
        "delete_running_player_session_unless_play_auth_changed",
        DeleteRunningPlayerSessionInputs {
            input_profile_id: profile_id,
            input_request_id: ARBITRIUM_REQUEST_ID.as_str(),
            input_play_auth_hash: play_auth_hash,
        },
    )
    .await?;
    Ok(())
}

// Chat is written after the fact rather than as it is delivered, so each row carries the time the
// server accepted the message instead of the time the row happened to land. Several messages go in
// one request, because a busy server produces them faster than it is worth making requests.
pub async fn insert_game_chats(game_chat_messages: &[GameChatMessage]) -> anyhow::Result<()> {
    #[derive(Serialize)]
    struct GameChatRow<'a> {
        serverid: &'a str,
        time_live_server: String,
        userid: String,
        message: &'a str,
    }

    let serverid = ARBITRIUM_REQUEST_ID.as_str();
    let rows = game_chat_messages
        .iter()
        .map(|game_chat_message| {
            let nanos = i128::from(game_chat_message.time_live_server_ms) * 1_000_000;
            Ok(GameChatRow {
                serverid,
                time_live_server: OffsetDateTime::from_unix_timestamp_nanos(nanos)?.format(&Rfc3339)?,
                userid: game_chat_message.userid.to_string(),
                message: game_chat_message.message.as_str(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    postgrest::from("game_chats").insert(&rows).await?;
    Ok(())
}

// This is a best-effort graceful shutdown delete for the current Edgegap deployment.
pub async fn remove_running_server() -> anyhow::Result<()> {
    postgrest::from("running_servers")
        .eq("request_id", ARBITRIUM_REQUEST_ID.as_str())
        .delete()
        .await?;
    Ok(())
}

// Returns true once this server has no running_servers row, so the website can no longer send players to it.
pub async fn remove_running_server_if_empty() -> anyhow::Result<bool> {
    #[derive(serde::Serialize)]
    struct DeleteRunningServerIfEmptyInputs<'a> {
        input_request_id: &'a str,
    }

    Ok(postgrest::call(
        "delete_running_server_if_empty",
        DeleteRunningServerIfEmptyInputs {
            input_request_id: ARBITRIUM_REQUEST_ID.as_str(),
        },
    )
    .await?)
}
