use server::playtest_auth::{PlaytestAuth, PlaytestServerStatus};
use shared::player_profile::PlayerProfile;
use std::sync::Arc;

#[tokio::test]
async fn a_handshake_claims_the_player_reservation_once() {
    let auth = PlaytestAuth::default();
    let token = auth.reserve("Player".into(), Some(7), PlayerProfile::GUEST).await;
    let player = auth.claim(&token).await.unwrap();
    assert_eq!(player.username, "Player");
    assert_eq!(player.user_id, Some(7));
    assert!(auth.claim(&token).await.is_err());
}
#[tokio::test]
async fn simultaneous_handshakes_can_only_claim_a_reservation_once() {
    let auth_arc = Arc::new(PlaytestAuth::default());
    let token = auth_arc.reserve("Player".into(), None, PlayerProfile::GUEST).await;
    let (first, second) = tokio::join!(auth_arc.claim(&token), auth_arc.claim(&token));
    assert_ne!(first.is_ok(), second.is_ok());
}
#[tokio::test]
async fn replacement_servers_and_players_have_independent_tokens() {
    let first = PlaytestAuth::default();
    let second = PlaytestAuth::default();
    let first_token = first.reserve("First".into(), None, PlayerProfile::GUEST).await;
    let second_token = first.reserve("Second".into(), None, PlayerProfile::GUEST).await;
    assert_ne!(first_token, second_token);
    assert!(second.claim(&first_token).await.is_err());
    first.claim(&first_token).await.unwrap();
    assert_eq!(first.claim(&second_token).await.unwrap().username, "Second");
}
#[tokio::test]
async fn startup_failure_and_shutdown_wake_waiters() {
    let auth = PlaytestAuth::default();
    auth.set_status(PlaytestServerStatus::Stopped);
    assert!(auth.wait_until_running().await.is_err());
    auth.wait_until_stopped().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn expired_reservations_cannot_be_claimed() {
    let auth = PlaytestAuth::default();
    let token = auth.reserve("Player".into(), None, PlayerProfile::GUEST).await;
    tokio::time::advance(server::playtest_auth::PLAYTEST_JOIN_TIMEOUT + std::time::Duration::from_secs(1)).await;
    assert!(auth.claim(&token).await.is_err());
}

#[tokio::test]
async fn stop_waiter_observes_a_running_playtest_stopping() {
    let auth = PlaytestAuth::default();
    auth.set_status(PlaytestServerStatus::Running);
    let did_stop = std::cell::Cell::new(false);
    tokio::join!(
        async {
            auth.wait_until_stopped().await.unwrap();
            assert!(did_stop.get(), "The waiter must stay pending while the playtest is running");
        },
        async {
            tokio::task::yield_now().await;
            did_stop.set(true);
            auth.set_status(PlaytestServerStatus::Stopped);
        }
    );
}
