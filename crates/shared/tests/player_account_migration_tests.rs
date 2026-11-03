use game_types::object_types::{ObjectBeingAdded, ObjectInfo, PlayerInfo, shared::Color3};
use serde_json::{Value, json};
use shared::game::jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS};
use shared::game::migrateStoredWorld::{StoredWorldKind, updateToVersion};
use shared::game::world::{World, WorldControlMode};
use tokio::sync::mpsc;

// A player's userid used to tell connections apart, and is the player's kube.new account now. Scripts saved before that use a Player
// object's id instead, and characters built like the default one get a HatSpot for the player's hat, with a script that puts it on.

const DEFAULT_CHARACTER_WORLD: &str = include_str!("fixtures/default_character.kube");

fn migrate_script(code: &str) -> anyhow::Result<String> {
    let mut stored_world = json!({ "worldSchemaVersion": 86, "scriptRawCodeOfId": { "2": code } });
    updateToVersion(&mut stored_world, 87, StoredWorldKind::GameWorld)?;
    Ok(stored_world["scriptRawCodeOfId"]["2"].as_str().unwrap().to_string())
}

#[test]
fn scripts_tell_players_apart_by_their_object_id() -> anyhow::Result<()> {
    let cases = [
        ("sync(player.userid, state).catch(() => {});", "sync(player.id, state).catch(() => {});"),
        (
            "const me = racers.find((racer) => racer.userId === player?.userid);",
            "const me = racers.find((racer) => racer.userId === player?.id);",
        ),
        ("const key = `${p.userid}:${slot}`;", "const key = `${p.id}:${slot}`;"),
        (
            "if (typeof userId !== 'string' || typeof action !== 'string') return;",
            "if (typeof userId !== 'number' || typeof action !== 'string') return;",
        ),
        (
            "players.set(player.userid, me);\nconst callerOf = (args) => typeof args[0] === \"string\" ? players.get(args[0]) : undefined;",
            "players.set(player.id, me);\nconst callerOf = (args) => typeof args[0] === \"number\" ? players.get(args[0]) : undefined;",
        ),
        ("if (typeof player.userid == 'string') ok();", "if (typeof player.id == 'number') ok();"),
        ("show(`🏆 ${winner.userid}`); // 🎉", "show(`🏆 ${winner.id}`); // 🎉"),
    ];
    for (old, new) in cases {
        assert_eq!(migrate_script(old)?, new, "migrating {old}");
    }
    Ok(())
}

#[test]
fn comments_strings_and_other_fields_stay_as_they_were() -> anyhow::Result<()> {
    let untouched = [
        "// a client function takes player.userid first",
        "log(\"player.userid\")",
        "cache.useridCount += 1",
        "if (typeof name !== 'string') return;",
    ];
    for code in untouched {
        assert_eq!(migrate_script(code)?, code);
    }
    Ok(())
}

// The default character as a world saved before players had hats, without the fixture's objects of these ids.
fn default_character_at_version_86_without(removed_ids: &[&str]) -> anyhow::Result<Value> {
    let mut stored_world: Value = serde_json::from_str(DEFAULT_CHARACTER_WORLD)?;
    updateToVersion(&mut stored_world, 86, StoredWorldKind::GameWorld)?;
    for &id in removed_ids {
        assert!(stored_world["objectOfId"][id]["name"] != Value::Null, "the fixture's object {id} moved");
        for map in [
            "objectOfId",
            "childrenOfId",
            "physicsInfo3DOfId",
            "physicsToggles3DOfId",
            "primitiveSettingsOfId",
            "scriptRawCodeOfId",
        ] {
            stored_world[map].as_object_mut().unwrap().remove(id);
        }
    }
    for children in stored_world["childrenOfId"].as_object_mut().unwrap().values_mut() {
        children
            .as_array_mut()
            .unwrap()
            .retain(|child| !removed_ids.contains(&child.to_string().as_str()));
    }
    Ok(stored_world)
}

// No HatSpot, and no appearance script.
fn default_character_without_a_hat_spot() -> anyhow::Result<Value> {
    default_character_at_version_86_without(&["488", "489", "490"]) // the HatSpot, its SkAttachment, and the appearance script
}

fn ids_named(stored_world: &Value, name: &str) -> Vec<String> {
    stored_world["objectOfId"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, object)| object["name"] == name)
        .map(|(id, _)| id.clone())
        .collect()
}

#[test]
fn a_character_built_like_the_default_one_gets_a_hat_spot_on_top_of_its_head() -> anyhow::Result<()> {
    let mut stored_world = default_character_without_a_hat_spot()?;
    updateToVersion(&mut stored_world, 88, StoredWorldKind::GameWorld)?;

    // the Head is a 1x1x1 brick at y 3.3, half a unit below its joint's offset of 0.5, so its top middle is at y 3.8 and 1.0 from the joint
    let [hat_spot] = &ids_named(&stored_world, "HatSpot")[..] else {
        anyhow::bail!("the Character should get one HatSpot");
    };
    let [body] = &ids_named(&stored_world, "Body")[..] else {
        anyhow::bail!("the fixture should have one Body");
    };
    assert!(
        stored_world["childrenOfId"][body]
            .as_array()
            .unwrap()
            .contains(&hat_spot.parse::<u64>()?.into())
    );
    let position: [f32; 3] = serde_json::from_value(stored_world["physicsInfo3DOfId"][hat_spot]["location"]["position"].clone())?;
    assert_eq!(position, [0.0, 3.8, 0.0]);
    assert_eq!(stored_world["physicsToggles3DOfId"][hat_spot]["isWalkthrough"], json!(true));
    assert_eq!(stored_world["objectOfId"][hat_spot]["info"]["Brick"]["visualSettings"]["opacity"], json!(0.0));
    let [attachment] = &stored_world["childrenOfId"][hat_spot].as_array().unwrap()[..] else {
        anyhow::bail!("the HatSpot should have one SkAttachment");
    };
    let attachment = &stored_world["objectOfId"][attachment.to_string()]["info"]["SkAttachment"];
    assert_eq!(attachment["jointName"], "Head");
    assert_eq!(attachment["objectId"].to_string(), *hat_spot);
    let offset: [f32; 3] = serde_json::from_value(attachment["offsetFromJoint"]["position"].clone())?;
    assert_eq!(offset, [0.0, 1.0, 0.0]);

    // the game gets the appearance script, which keeps an older game's colors
    let [appearance] = &ids_named(&stored_world, "appearance")[..] else {
        anyhow::bail!("the game should get one appearance script");
    };
    let code = stored_world["scriptRawCodeOfId"][appearance].as_str().unwrap();
    assert!(code.contains("const PREFERRED_COLOR_PARTS: string[] = [];"));
    assert_eq!(stored_world["objectOfId"][appearance]["info"]["Script"]["objectLocation"], "Server");

    World::fromStoredWorldString(&stored_world.to_string(), WorldControlMode::Editor, None)?;
    Ok(())
}

#[test]
fn a_character_with_no_visible_head_or_with_a_hat_of_its_own_gets_no_hat_spot() -> anyhow::Result<()> {
    let renamed_head = |name: &str| -> anyhow::Result<Value> {
        let mut stored_world = default_character_without_a_hat_spot()?;
        let head = ids_named(&stored_world, "Head")
            .into_iter()
            .find(|id| stored_world["objectOfId"][id]["info"].get("Brick").is_some())
            .unwrap();
        stored_world["objectOfId"][&head]["name"] = name.into();
        Ok(stored_world)
    };
    let mut invisible_head = renamed_head("Head")?;
    for object in invisible_head["objectOfId"].as_object_mut().unwrap().values_mut() {
        if object["name"] == "Head" && object["info"].get("Brick").is_some() {
            object["info"]["Brick"]["visualSettings"]["opacity"] = 0.0.into();
        }
    }
    let mut own_hat = renamed_head("Head")?;
    let cap = ids_named(&own_hat, "Cap")[0].clone();
    own_hat["objectOfId"][&cap]["name"] = "HatBrim".into();

    for (case, mut stored_world) in [
        ("no Head", renamed_head("Skull")?),
        ("an invisible Head", invisible_head),
        ("its own hat", own_hat),
    ] {
        updateToVersion(&mut stored_world, 88, StoredWorldKind::GameWorld)?;
        assert!(ids_named(&stored_world, "HatSpot").is_empty(), "a Character with {case} should get no HatSpot");
        assert!(
            ids_named(&stored_world, "appearance").is_empty(),
            "a game whose Character has {case} needs no appearance script"
        );
    }
    Ok(())
}

// A HatSpot placed by hand, like on a character with no Head, needs the appearance script too, unless the game's scripts already put hats on.
#[test]
fn a_hat_spot_placed_by_hand_gets_the_appearance_script() -> anyhow::Result<()> {
    let mut stored_world = default_character_at_version_86_without(&["490"])?; // just the appearance script
    updateToVersion(&mut stored_world, 88, StoredWorldKind::GameWorld)?;
    assert_eq!(
        ids_named(&stored_world, "HatSpot"),
        ["488"],
        "the HatSpot placed by hand should be the only one"
    );
    let [appearance] = &ids_named(&stored_world, "appearance")[..] else {
        anyhow::bail!("the game should get one appearance script");
    };
    assert!(stored_world["scriptRawCodeOfId"][appearance].as_str().unwrap().contains("equippedItemId"));
    World::fromStoredWorldString(&stored_world.to_string(), WorldControlMode::Editor, None)?;

    let mut with_its_appearance_script = default_character_at_version_86_without(&[])?;
    updateToVersion(&mut with_its_appearance_script, 88, StoredWorldKind::GameWorld)?;
    assert_eq!(
        ids_named(&with_its_appearance_script, "appearance"),
        ["490"],
        "a game that has the appearance script keeps just that one"
    );

    let mut dressed_by_its_spawn_script = default_character_at_version_86_without(&["490"])?;
    let [spawn] = &ids_named(&dressed_by_its_spawn_script, "spawn")[..] else {
        anyhow::bail!("the fixture should have one spawn script");
    };
    dressed_by_its_spawn_script["scriptRawCodeOfId"][spawn] = "const hatId = await game.playerService.getEquippedItemId(player.id);".into();
    updateToVersion(&mut dressed_by_its_spawn_script, 88, StoredWorldKind::GameWorld)?;
    assert!(
        ids_named(&dressed_by_its_spawn_script, "appearance").is_empty(),
        "a game whose scripts put hats on needs no appearance script"
    );
    Ok(())
}

#[test]
fn an_asset_object_gets_no_hat_spot_or_script() -> anyhow::Result<()> {
    let mut stored_world = default_character_without_a_hat_spot()?;
    updateToVersion(&mut stored_world, 88, StoredWorldKind::AssetObject)?;
    assert!(ids_named(&stored_world, "HatSpot").is_empty());
    assert!(ids_named(&stored_world, "appearance").is_empty());
    Ok(())
}

// -------------------------- a player's profile is on the player object --------------------------

#[test]
fn calls_that_fetched_a_players_profile_read_it_from_the_player_object() -> anyhow::Result<()> {
    let migrate = |code: &str| -> anyhow::Result<String> {
        let mut stored_world = json!({ "worldSchemaVersion": 89, "scriptRawCodeOfId": { "2": code } });
        updateToVersion(&mut stored_world, 90, StoredWorldKind::GameWorld)?;
        Ok(stored_world["scriptRawCodeOfId"]["2"].as_str().unwrap().to_string())
    };
    let cases = [
        (
            "const color = await game.playerService.getPreferredColor(player.id);",
            "const color = await Promise.resolve(((player) => player?.type === \"player\" ? player.preferredColor : null)(game.objectService.getObjectById(player.id)));",
        ),
        (
            "game.playerService.getEquippedItemId(idOf(p)).catch(() => null)",
            "Promise.resolve(((player) => player?.type === \"player\" ? player.equippedItemId : null)(game.objectService.getObjectById(idOf(p)))).catch(() => null)",
        ),
    ];
    for (old, new) in cases {
        assert_eq!(migrate(old)?, new, "migrating {old}");
    }
    for untouched in [
        "// game.playerService.getPreferredColor() paints the Torso",
        "log(\"game.playerService.getEquippedItemId(1)\")",
    ] {
        assert_eq!(migrate(untouched)?, untouched);
    }
    Ok(())
}

// The appearance scripts saved before this awaited both fetches together, and some caught their errors.
#[tokio::test]
async fn a_script_that_fetched_a_players_profile_still_gets_it_after_migrating() -> anyhow::Result<()> {
    let old_code = r#"game.objectService.addObjectCreatedListener(async (player) => {
  const [preferredColor, hatId] = await Promise.all([game.playerService.getPreferredColor(player.id), game.playerService.getEquippedItemId(player.id).catch(() => null)]);
  player.username = JSON.stringify([preferredColor, hatId]);
}, "player");"#;
    let stored_world = json!({
        "worldSchemaVersion": 89,
        "fallback_camera_location": { "position": [0.0, 0.0, 0.0], "orientation": { "theta": 0.0, "phi": 0.0 } },
        "objectOfId": {
            "1": { "id": 1, "name": "root", "info": { "LocalGroup": {} } },
            "2": { "id": 2, "name": "appearance", "info": { "Script": { "objectLocation": "Server" } } }
        },
        "rootObjectId": 1,
        "childrenOfId": { "1": [2], "2": [] },
        "physicsInfo3DOfId": {},
        "physicsToggles3DOfId": {},
        "primitiveSettingsOfId": {},
        "scriptRawCodeOfId": { "2": old_code }
    });
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = mpsc::unbounded_channel();
    let mut world = World::fromStoredWorldString(&stored_world.to_string(), WorldControlMode::Server, Some(js_callback_inputs_tx))?;
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    let root_object_id = world.getRootObjectId();
    let player = world.addObject(
        "alice".to_string(),
        ObjectBeingAdded::Player(PlayerInfo {
            username: "alice".to_string(),
            userid: Some(17),
            preferredColor: Some(Color3 { r: 1.0, g: 0.5, b: 0.0 }),
            equippedItemId: Some(20),
            headObjectId: None,
            characterObjectId: None,
        }),
        root_object_id,
    );
    for now_ms in [16.0, 32.0] {
        runPendingJS(
            &mut js_runtime,
            &mut world,
            now_ms,
            Some(now_ms),
            &script_manager_rc,
            &mut js_callback_inputs_rx,
        )
        .await?;
    }

    let ObjectInfo::Player(PlayerInfo { username, .. }) = &world.getObject(player)?.info else {
        anyhow::bail!("{player} should be a player");
    };
    assert_eq!(username, r#"[{"r":1,"g":0.5,"b":0},20]"#);
    Ok(())
}
