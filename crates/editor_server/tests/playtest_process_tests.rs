use editor_server::playtest_process::{PlaytestProcess, PlaytestRequest};
use shared::player_profile::PlayerProfile;
use std::{net::UdpSocket, path::Path};

// One test, so that killing this test's playtest process cannot hit a playtest that another test started.
#[tokio::test(flavor = "multi_thread")]
async fn a_playtest_runs_in_its_own_process_and_a_crash_ends_only_the_playtest() -> anyhow::Result<()> {
    let program = Path::new(env!("CARGO_BIN_EXE_editor_server"));
    let world = include_str!("../../../dev-demo.kube");
    let fixed_port = UdpSocket::bind("127.0.0.1:0")?.local_addr()?.port();

    // A person's playtest takes the editor's playtest port and hands out join tokens.
    let (process, server_port) = PlaytestProcess::start(program, world.to_string(), Some(fixed_port), |_, _, _| {}).await?;
    assert_eq!(server_port, fixed_port);
    let join_token: String = process
        .request(PlaytestRequest::AddPlayer {
            username: "Tester".to_string(),
            user_id: None,
            profile: PlayerProfile::GUEST,
        })
        .await?;
    assert!(!join_token.is_empty());
    assert!(!process.request::<bool>(PlaytestRequest::HasPlayers).await?);

    // An AI playtest's headless players join and leave inside the playtest process. Each one runs scripts on a thread of
    // its own, which on Intel and AMD Linux machines only works because main sets V8 up before any other thread starts.
    for expected_client_id in [1, 2] {
        let client_id: u8 = process.request(PlaytestRequest::StartClient).await?;
        assert_eq!(client_id, expected_client_id);
    }
    assert!(process.request::<bool>(PlaytestRequest::HasPlayers).await?);
    for client_id in [1, 2] {
        process.request::<()>(PlaytestRequest::StopClient { client_id }).await?;
    }
    assert!(process.request::<()>(PlaytestRequest::StopClient { client_id: 1 }).await.is_err());

    // A stopped playtest frees its port for the next one.
    process.stop().await;
    assert!(process.has_stopped());
    let (process, _) = PlaytestProcess::start(program, world.to_string(), Some(fixed_port), |_, _, _| {}).await?;

    // A crash ends the playtest, and its requests say why. This test, standing in for the editor, keeps running.
    let killed = std::process::Command::new("pkill")
        .args(["-KILL", "-P", &std::process::id().to_string(), "-f", "run-playtest"])
        .status()?;
    assert!(killed.success());
    let error = process.request::<bool>(PlaytestRequest::HasPlayers).await.unwrap_err();
    assert!(error.to_string().contains("SIGKILL"), "{error}");
    assert!(process.has_stopped());
    Ok(())
}
