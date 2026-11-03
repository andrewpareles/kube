//! Each playtest owns its own temporary reservations. Stopping it invalidates every unused token.
use crate::database::hash_desktop_auth_token;
use anyhow::{Result, bail};
use shared::player_profile::PlayerProfile;
use std::{collections::HashMap, time::Duration};
use tokio::sync::{Mutex, watch};
use tokio::time::Instant;
use uuid::Uuid;

// TODO: AI-PICKED-VALUE: Allow two minutes for a desktop process to start and claim its reservation.
pub const PLAYTEST_JOIN_TIMEOUT: Duration = Duration::from_secs(120);

// trusted (validated earlier)
pub struct PendingPlayer {
    pub username: String,
    pub user_id: Option<i64>, // the account that added this player, if any
    pub profile: PlayerProfile,
    pub expires_at: Instant,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub enum PlaytestServerStatus {
    #[default]
    Starting,
    Running,
    Stopped,
}

#[derive(Default)]
pub struct PlaytestAuth {
    pending_player_of_auth_hash: Mutex<HashMap<String, PendingPlayer>>,
    status_tx: watch::Sender<PlaytestServerStatus>,
}

impl PlaytestAuth {
    pub fn set_status(&self, status: PlaytestServerStatus) {
        self.status_tx.send_replace(status);
    }

    pub async fn wait_until_running(&self) -> Result<()> {
        let mut status_rx = self.status_tx.subscribe();
        let status = *status_rx.wait_for(|status| *status != PlaytestServerStatus::Starting).await?;
        anyhow::ensure!(status == PlaytestServerStatus::Running, "Playtest server failed to bind its port.");
        Ok(())
    }
    pub async fn wait_until_stopped(&self) -> Result<()> {
        self.status_tx.subscribe().wait_for(|status| *status == PlaytestServerStatus::Stopped).await?;
        Ok(())
    }

    pub async fn reserve(&self, username: String, user_id: Option<i64>, profile: PlayerProfile) -> String {
        let join_token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let mut pending_player_of_auth_hash_lock = self.pending_player_of_auth_hash.lock().await;
        pending_player_of_auth_hash_lock.retain(|_, player| player.expires_at > Instant::now());
        pending_player_of_auth_hash_lock.insert(
            hash_desktop_auth_token(&join_token),
            PendingPlayer {
                username,
                user_id,
                profile,
                expires_at: Instant::now() + PLAYTEST_JOIN_TIMEOUT,
            },
        );
        drop(pending_player_of_auth_hash_lock);
        join_token
    }

    pub async fn claim(&self, join_token: &str) -> Result<PendingPlayer> {
        let mut pending_player_of_auth_hash_lock = self.pending_player_of_auth_hash.lock().await;
        let player = pending_player_of_auth_hash_lock.remove(&hash_desktop_auth_token(join_token));
        drop(pending_player_of_auth_hash_lock);
        let Some(player) = player else {
            bail!("Playtest reservation does not exist or was already claimed.");
        };
        if player.expires_at <= Instant::now() {
            bail!("Playtest reservation expired.");
        }
        Ok(player)
    }
}
