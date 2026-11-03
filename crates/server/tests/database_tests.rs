use serde::Deserialize;
use serde_json::{Value, json};
use server::database::{
    download_published_world_or_none, end_player_session, get_play_auth_hash_of_profile_id_batch, hash_desktop_auth_token, insert_game_chats,
    remove_running_server, remove_running_server_if_empty, validate_play_auth,
};
use shared::env::env_vars::ARBITRIUM_REQUEST_ID;
use shared::player_profile::get_player_profile;
use shared::postgrest;
use shared::protocol::messages::GameChatMessage;
use std::collections::HashMap;
use uuid::Uuid;

async fn insert(table: &str, row: Value) {
    postgrest::from(table).insert(row).await.unwrap();
}

async fn add_profile(username: &str) -> String {
    let profile_id = Uuid::new_v4().to_string();
    insert("user_profiles", json!({"id": profile_id, "username": username})).await;
    profile_id
}

async fn has_session(profile_id: &str) -> bool {
    !postgrest::from("running_player_sessions")
        .select("profile_id")
        .eq("profile_id", profile_id)
        .get::<Value>()
        .await
        .unwrap()
        .is_empty()
}

async fn is_running() -> bool {
    !postgrest::from("running_servers")
        .select("request_id")
        .eq("request_id", ARBITRIUM_REQUEST_ID.as_str())
        .get::<Value>()
        .await
        .unwrap()
        .is_empty()
}

// Run by tests/postgrest_clients.database.test.ts, which starts PostgREST over a throwaway copy of production's schema and sets
// the variables these read. A game server's database calls go through it, as this deployment. Reading a published world that
// exists also needs S3, so only a missing one is read here.
#[tokio::test]
#[ignore = "needs the PostgREST that tests/postgrest_clients.database.test.ts starts"]
async fn the_game_server_database_calls_work_through_postgrest() {
    let request_id = ARBITRIUM_REQUEST_ID.as_str();
    let creator_id = add_profile("creator").await;
    #[derive(Deserialize)]
    struct Game {
        game_id: i64,
    }
    let game_id = postgrest::from("game_metadata")
        .insert_returning::<Game>(json!({"name": "Game", "creator_id": creator_id, "max_players": 8}))
        .await
        .unwrap()
        .remove(0)
        .game_id;
    insert("game_worlds", json!({"game_id": game_id, "world_uuid": Uuid::new_v4().to_string()})).await;
    assert!(download_published_world_or_none(game_id, 2).await.unwrap().is_none());
    let running_server =
        json!({"request_id": request_id, "game_id": game_id, "gameworld_version_num": 1, "max_players": 8, "public_ip": "203.0.113.7", "external_port": 7777});
    insert("running_servers", running_server.clone()).await;

    let player_id = add_profile("player").await;
    postgrest::from("user_profiles")
        .eq("id", &player_id)
        .update(json!({"preferred_color": "#00ff00"}))
        .await
        .unwrap();
    let play_auth_hash = hash_desktop_auth_token("play-auth");
    insert("user_desktop_play_auth", json!({"profile_id": player_id, "play_auth_hash": play_auth_hash})).await;
    assert!(validate_play_auth("play-auth").await.unwrap().is_none());
    insert("running_player_sessions", json!({"profile_id": player_id, "request_id": request_id})).await;
    assert!(validate_play_auth("another-play-auth").await.unwrap().is_none());
    let player = validate_play_auth("play-auth").await.unwrap().unwrap();
    let green = json!({"preferredColor": {"r": 0.0, "g": 1.0, "b": 0.0}, "equippedItemId": null});
    assert_eq!(
        (
            player.profile_id.as_str(),
            player.username.as_str(),
            player.play_auth_hash.as_str(),
            serde_json::to_value(&player.profile).unwrap()
        ),
        (player_id.as_str(), "player", play_auth_hash.as_str(), green.clone())
    );
    #[derive(Deserialize)]
    struct Session {
        connected_at: Option<String>,
    }
    let sessions = postgrest::from("running_player_sessions")
        .select("connected_at")
        .eq("profile_id", &player_id)
        .get::<Session>()
        .await
        .unwrap();
    assert!(sessions[0].connected_at.is_some());

    let logged_out_id = add_profile("loggedout").await;
    insert("user_desktop_play_auth", json!({"profile_id": logged_out_id, "play_auth_hash": null})).await;
    assert_eq!(
        get_play_auth_hash_of_profile_id_batch([player_id.as_str(), logged_out_id.as_str(), creator_id.as_str()])
            .await
            .unwrap(),
        HashMap::from([(player_id.clone(), play_auth_hash.clone())])
    );
    assert!(get_play_auth_hash_of_profile_id_batch([]).await.unwrap().is_empty());

    assert_eq!(serde_json::to_value(get_player_profile(player.user_id).await.unwrap()).unwrap(), green);
    assert!(get_player_profile(player.user_id + 1000).await.is_err());

    insert_game_chats(&[
        GameChatMessage {
            time_live_server_ms: 1_791_590_400_123,
            userid: player.user_id,
            username: "player".into(),
            message: "Hello".into(),
        },
        GameChatMessage {
            time_live_server_ms: 1_791_590_401_000,
            userid: player.user_id,
            username: "player".into(),
            message: "Anyone here?".into(),
        },
    ])
    .await
    .unwrap();
    #[derive(Deserialize)]
    struct GameChat {
        time_live_server: String,
        userid: String,
        message: String,
    }
    let game_chats = postgrest::from("game_chats")
        .select("time_live_server,userid,message")
        .eq("serverid", request_id)
        .order_desc("game_chat_id")
        .get::<GameChat>()
        .await
        .unwrap();
    let userid = player.user_id.to_string();
    assert_eq!(
        game_chats
            .iter()
            .map(|chat| (chat.time_live_server.as_str(), chat.userid.as_str(), chat.message.as_str()))
            .collect::<Vec<_>>(),
        [
            ("2026-10-10T00:00:01+00:00", userid.as_str(), "Anyone here?"),
            ("2026-10-10T00:00:00.123+00:00", userid.as_str(), "Hello")
        ]
    );

    end_player_session(&player_id, &hash_desktop_auth_token("older-play-auth")).await.unwrap();
    assert!(has_session(&player_id).await);
    assert!(!remove_running_server_if_empty().await.unwrap());
    end_player_session(&player_id, &play_auth_hash).await.unwrap();
    assert!(!has_session(&player_id).await);
    assert!(remove_running_server_if_empty().await.unwrap());
    assert!(!is_running().await);
    insert("running_servers", running_server).await;
    remove_running_server().await.unwrap();
    assert!(!is_running().await);
}
