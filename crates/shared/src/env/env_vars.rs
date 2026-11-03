// IMPORTANT: don't try to access vars you don't have access to, or you will get a panic

use game_types::GameId;
use std::sync::LazyLock;

fn read_non_empty_env_var(key: &str) -> String {
    let value = std::env::var(key).unwrap();
    let trimmed_value = value.trim();
    if trimmed_value.is_empty() {
        panic!("{key} cannot be empty");
    }
    trimmed_value.to_string()
}

pub static UPDATER_PUBLIC_KEY: LazyLock<String> = LazyLock::new(|| std::env::var("UPDATER_PUBLIC_KEY").unwrap());
pub static SUPABASE_URL: LazyLock<String> = LazyLock::new(|| std::env::var("SUPABASE_URL").unwrap());
pub static SUPABASE_PUBLISHABLE_KEY: LazyLock<String> = LazyLock::new(|| std::env::var("SUPABASE_PUBLISHABLE_KEY").unwrap());
pub static SUPABASE_SECRET_KEY: LazyLock<String> = LazyLock::new(|| std::env::var("SUPABASE_SECRET_KEY").unwrap());
pub static EDGEGAP_INTERNAL_PORT: LazyLock<u16> = LazyLock::new(|| std::env::var("EDGEGAP_INTERNAL_PORT").unwrap().parse::<u16>().unwrap());
pub static SITE_URL: LazyLock<String> = LazyLock::new(|| std::env::var("SITE_URL").unwrap().trim_end_matches('/').to_string());

// server
pub static GAME_ID: LazyLock<GameId> = LazyLock::new(|| read_non_empty_env_var("GAME_ID").parse::<GameId>().unwrap());
pub static GAMEWORLD_VERSION_NUM: LazyLock<i64> = LazyLock::new(|| read_non_empty_env_var("GAMEWORLD_VERSION_NUM").parse::<i64>().unwrap());
pub static ARBITRIUM_REQUEST_ID: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("ARBITRIUM_REQUEST_ID"));
pub static POSTHOG_KEY: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("POSTHOG_KEY"));
pub static QUIC_SERVER_KEY_DER_BASE64: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("QUIC_SERVER_KEY_DER_BASE64"));

// Hosted editor service connections are configured by its deployment.
pub static ARBITRIUM_DELETE_URL: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("ARBITRIUM_DELETE_URL"));
pub static ARBITRIUM_DELETE_TOKEN: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("ARBITRIUM_DELETE_TOKEN"));

pub static EDITOR_SERVER_PORT: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("EDITOR_SERVER_PORT").to_string());

pub static EDITOR_CHAT_PORT: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("EDITOR_CHAT_PORT"));

pub static EDITOR_PLAYTEST_PORT: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("EDITOR_PLAYTEST_PORT").to_string());

pub static EDITOR_SERVER_SECRET: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("EDITOR_SERVER_SECRET").to_string());

// The game server and the editor share one AWS user for S3.
pub static S3_ACCESS_KEY_ID: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("S3_ACCESS_KEY_ID"));
pub static S3_SECRET_ACCESS_KEY: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("S3_SECRET_ACCESS_KEY"));

pub static AWS_BEARER_TOKEN_BEDROCK: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("AWS_BEARER_TOKEN_BEDROCK"));

pub static RESEND_API_KEY: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("RESEND_API_KEY"));

pub static AZURE_OPENAI_API_KEY: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("AZURE_OPENAI_API_KEY"));

pub static CHROMIUM_PATH: LazyLock<String> = LazyLock::new(|| read_non_empty_env_var("CHROMIUM_PATH"));
