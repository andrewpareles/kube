use game_types::object_types::player::UserId;
use shared::env::env_vars::POSTHOG_KEY;

// Sends one event to PostHog under the player's numeric user id, the same id the website identifies them by.
pub async fn capture_posthog_event(event: &str, user_id: UserId, properties: serde_json::Value) -> anyhow::Result<()> {
    #[derive(serde::Serialize)]
    struct PostHogEventInputs<'a> {
        api_key: &'a str,
        event: &'a str,
        distinct_id: String,
        properties: serde_json::Value,
    }

    let inputs = PostHogEventInputs {
        api_key: POSTHOG_KEY.as_str(),
        event,
        distinct_id: user_id.to_string(),
        properties,
    };

    let response = reqwest::Client::new().post("https://us.i.posthog.com/i/v0/e/").json(&inputs).send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_else(|_| "No response body".to_string());
        return Err(anyhow::anyhow!("Failed to send PostHog event {} (status {}): {}", event, status, body));
    }

    Ok(())
}
