use serde_json::json;
use shared::game::migrateStoredWorld::{StoredWorldKind, updateToVersion};

// moveObjectTo used to jump an object to its new place. Only teleportObjectTo jumps now, so scripts saved before that call it instead.

fn migrate_script(code: &str, stored_world_kind: StoredWorldKind) -> anyhow::Result<String> {
    let mut stored_world = json!({ "worldSchemaVersion": 90, "scriptRawCodeOfId": { "2": code } });
    updateToVersion(&mut stored_world, 91, stored_world_kind)?;
    Ok(stored_world["scriptRawCodeOfId"]["2"].as_str().unwrap().to_string())
}

#[test]
fn moving_an_object_to_a_place_becomes_a_teleport() -> anyhow::Result<()> {
    let cases = [
        (
            "game.objectService.moveObjectTo(body.id, spawnPosition);",
            "game.objectService.teleportObjectTo(body.id, spawnPosition);",
        ),
        (
            "const { objectService } = game;\nif (y < -50) objectService.moveObjectTo(id, { x: 0, y: 5, z: 0 });",
            "const { objectService } = game;\nif (y < -50) objectService.teleportObjectTo(id, { x: 0, y: 5, z: 0 });",
        ),
        (
            "for (const p of players) game.objectService.moveObjectTo(p.characterObjectId, lobby);\ngame.objectService.moveObjectTo(ball.id, center);",
            "for (const p of players) game.objectService.teleportObjectTo(p.characterObjectId, lobby);\ngame.objectService.teleportObjectTo(ball.id, center);",
        ),
        (
            "const sendBack = game.objectService.moveObjectTo;",
            "const sendBack = game.objectService.teleportObjectTo;",
        ),
    ];
    for (old, new) in cases {
        assert_eq!(migrate_script(old, StoredWorldKind::GameWorld)?, new, "migrating {old}");
    }
    Ok(())
}

// The default character's spawn script respawns players with moveObjectTo, and downloading it adds an asset object to the game.
#[test]
fn a_downloaded_asset_teleports_too() -> anyhow::Result<()> {
    assert_eq!(
        migrate_script("game.objectService.moveObjectTo(body.id, spawnPosition);", StoredWorldKind::AssetObject)?,
        "game.objectService.teleportObjectTo(body.id, spawnPosition);"
    );
    Ok(())
}

// Quotes in template text, JSX text or a regex used to start a string that hid every call up to the next quote.
#[test]
fn every_move_object_to_becomes_a_teleport_however_the_script_reaches_it() -> anyhow::Result<()> {
    let cases = [
        (
            "showMessage(`Don't fall!`);\ngame.objectService.moveObjectTo(id, spawn);\nconst label = 'Respawned';",
            "showMessage(`Don't fall!`);\ngame.objectService.teleportObjectTo(id, spawn);\nconst label = 'Respawned';",
        ),
        (
            "game.guiService.show('hud', <p>Don't fall off!</p>);\ngame.objectService.moveObjectTo(id, spawn);\nconst toast = 'Respawned';",
            "game.guiService.show('hud', <p>Don't fall off!</p>);\ngame.objectService.teleportObjectTo(id, spawn);\nconst toast = 'Respawned';",
        ),
        (
            "const quotes = /'/g;\ngame.objectService.moveObjectTo(id, spawn);\nconst name = 'a';",
            "const quotes = /'/g;\ngame.objectService.teleportObjectTo(id, spawn);\nconst name = 'a';",
        ),
        (
            "game.objectService\n    .moveObjectTo(id, spawn);",
            "game.objectService\n    .teleportObjectTo(id, spawn);",
        ),
        (
            "const { moveObjectTo } = game.objectService;\nmoveObjectTo(id, spawn);",
            "const { teleportObjectTo } = game.objectService;\nteleportObjectTo(id, spawn);",
        ),
        (
            "const objects = game.objectService;\nobjects.moveObjectTo(id, spawn);",
            "const objects = game.objectService;\nobjects.teleportObjectTo(id, spawn);",
        ),
        (
            "// game.objectService.moveObjectTo used to jump",
            "// game.objectService.teleportObjectTo used to jump",
        ),
    ];
    for (old, new) in cases {
        assert_eq!(migrate_script(old, StoredWorldKind::GameWorld)?, new, "migrating {old}");
    }
    Ok(())
}

#[test]
fn other_moves_stay_as_they_were() -> anyhow::Result<()> {
    let untouched = [
        "game.objectService.moveObjectBy(id, offset);",
        "myMoveObjectTo(id, position);",
        "moveObjectToward(id, position);",
    ];
    for code in untouched {
        assert_eq!(migrate_script(code, StoredWorldKind::GameWorld)?, code);
    }
    Ok(())
}
