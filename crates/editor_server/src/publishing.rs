use crate::database;
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use resend_rs::{Resend, types::CreateEmailBaseOptions};
use serde_json::json;
use shared::{
    env::env_vars::{EDITOR_CHAT_PORT, GAME_ID, RESEND_API_KEY, SITE_URL},
    game::world::{World, WorldControlMode},
    s3_storage::{PrivateFile, PublicFile, upload_private_file, upload_public_file},
};
use std::time::Duration;
use uuid::Uuid;

// TODO: AI-PICKED-VALUE: The sender address and the email's styling. updates.kube.new is the domain verified in Resend, which already sends the Supabase sign-up emails.
const GAME_PUBLISHED_EMAIL_FROM: &str = "kube.new <noreply@updates.kube.new>";
const GAME_PUBLISHED_EMAIL_REPLY_TO: &str = "andrew@kube.new";
// TODO: AI-PICKED-VALUE: An AI thumbnail still being drawn after ten minutes, retries included, is given up on, and the game keeps the thumbnail it has.
const AI_THUMBNAIL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

// Publishing is an internal editor operation; return the created version directly.
pub async fn publish_world(stored_world_string: &str) -> Result<i64> {
    World::fromStoredWorldString(stored_world_string, WorldControlMode::Server, None).map_err(|error| anyhow::anyhow!("World did not parse: {error}"))?;
    insert_gameworld(stored_world_string)
        .await
        .map_err(|error| anyhow::anyhow!("Failed to publish world: {error}"))
}

async fn insert_gameworld(stored_world_string: &str) -> anyhow::Result<i64> {
    let world_uuid = Uuid::new_v4();
    upload_private_file(&PrivateFile::GameWorld { world_uuid }, stored_world_string.as_bytes().to_vec()).await?;
    database::insert_game_world(world_uuid).await
}

// The publisher passes the captured PNG directly; success needs no response payload.
// Each thumbnail is a new file, and the game row then points at it, so a cached thumbnail is never out of date.
pub async fn upload_thumbnail(png_bytes: Vec<u8>) -> Result<()> {
    image::load_from_memory_with_format(png_bytes.as_slice(), image::ImageFormat::Png)
        .map_err(|error| anyhow::anyhow!("Thumbnail was not a valid PNG: {error}"))?;

    let thumbnail_uuid = Uuid::new_v4();
    upload_public_file(&PublicFile::GameThumbnail { thumbnail_uuid }, png_bytes)
        .await
        .context("Failed to upload thumbnail")?;
    database::set_game_screenshot(thumbnail_uuid).await.context("Failed to save thumbnail")?;
    Ok(())
}

// The AI redraws a screenshot as a polished thumbnail, which the game shows from then on.
pub async fn upload_ai_thumbnail(http: &reqwest::Client, token: &str, screenshot_png_bytes: &[u8]) -> Result<()> {
    let settings = database::game_settings().await?.context("Game not found.")?;
    let png_bytes = http
        .post(format!("http://127.0.0.1:{}/local-only/generate-ai-thumbnail", EDITOR_CHAT_PORT.as_str()))
        .bearer_auth(token)
        .timeout(AI_THUMBNAIL_TIMEOUT)
        .json(&json!({"name": settings.name, "description": settings.description, "screenshot_png_base64": STANDARD.encode(screenshot_png_bytes)}))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let ai_thumbnail_uuid = Uuid::new_v4();
    upload_public_file(
        &PublicFile::GameThumbnail {
            thumbnail_uuid: ai_thumbnail_uuid,
        },
        png_bytes.into(),
    )
    .await
    .context("Failed to upload AI thumbnail")?;
    database::set_game_ai_thumbnail(ai_thumbnail_uuid)
        .await
        .context("Failed to save AI thumbnail")?;
    Ok(())
}

// Resend writes the plain text version from the HTML.
pub async fn send_game_published_email(creator_id: &str) -> Result<()> {
    fn escape_html(text: &str) -> String {
        text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }
    let settings = database::game_settings().await?.context("Game not found.")?;
    let (email, username) = database::profile_email_and_username(creator_id).await?;
    let html = format!(
        include_str!("game_published_email.html"),
        username = escape_html(&username),
        guest_note = if username.starts_with("Guest-") {
            " (you should change your username!)"
        } else {
            ""
        },
        game_url = format!("{}/games/{}", SITE_URL.as_str(), *GAME_ID),
        game_name = escape_html(&settings.name),
    );
    let message = CreateEmailBaseOptions::new(GAME_PUBLISHED_EMAIL_FROM, [email], format!("Your game is ready! {}", settings.name))
        .with_reply(GAME_PUBLISHED_EMAIL_REPLY_TO)
        .with_html(&html);
    Resend::new(RESEND_API_KEY.as_str()).emails.send(message).await?;
    Ok(())
}
