use serde_json::{Value, json};
use shared::game::migrateStoredWorld::{StoredWorldKind, updateToVersion};

// Objects used to be anchored or not. They are pushable or not now, the opposite, and worlds and scripts saved before that are
// rewritten when they load.

fn migrate(mut stored_world: Value) -> anyhow::Result<Value> {
    stored_world["worldSchemaVersion"] = 85.into();
    updateToVersion(&mut stored_world, 86, StoredWorldKind::GameWorld)?;
    Ok(stored_world)
}

fn migrate_script(code: &str) -> anyhow::Result<String> {
    let stored_world = migrate(json!({ "scriptRawCodeOfId": { "2": code } }))?;
    Ok(stored_world["scriptRawCodeOfId"]["2"].as_str().unwrap().to_string())
}

#[test]
fn anchored_objects_become_unpushable_and_the_rest_pushable() -> anyhow::Result<()> {
    let stored_world = migrate(json!({ "physicsToggles3DOfId": {
        "2": { "isWalkthrough": false, "isAnchored": true, "density": 1.0 },
        "3": { "isWalkthrough": true, "isAnchored": false, "density": 2.0 },
    }}))?;
    assert_eq!(stored_world["physicsToggles3DOfId"]["2"], json!({ "isWalkthrough": false, "isPushable": false, "density": 1.0 }));
    assert_eq!(stored_world["physicsToggles3DOfId"]["3"], json!({ "isWalkthrough": true, "isPushable": true, "density": 2.0 }));
    Ok(())
}

#[test]
fn scripts_set_and_read_whether_objects_are_pushable_instead() -> anyhow::Result<()> {
    let cases = [
        ("game.objectService.setObjectAnchored(body.id, false);", "game.objectService.setObjectPushable(body.id, true);"),
        ("os.setObjectAnchored(getId(a, b), true)", "os.setObjectPushable(getId(a, b), false)"),
        ("game.objectService.setObjectAnchored(id, !shouldMove)", "game.objectService.setObjectPushable(id, shouldMove)"),
        ("game.objectService.setObjectAnchored(id, a && b)", "game.objectService.setObjectPushable(id, !(a && b))"),
        ("({ isWalkthrough: false, isAnchored: true, density: 1 })", "({ isWalkthrough: false, isPushable: false, density: 1 })"),
        (
            "({ isWalkthrough: walk, isAnchored: anch === false ? false : true, density: 1 })",
            "({ isWalkthrough: walk, isPushable: !(anch === false ? false : true), density: 1 })",
        ),
        ("({\n  isAnchored: anchored,\n})", "({\n  isPushable: !anchored,\n})"),
        (
            "type Info = { isWalkthrough: boolean; isAnchored: boolean; density: number }",
            "type Info = { isWalkthrough: boolean; isPushable: boolean; density: number }",
        ),
        ("brick.isAnchored = false;", "brick.isPushable = true;"),
        ("if (body.getChild(\"Torso\").isAnchored) sit()", "if ((!body.getChild(\"Torso\").isPushable)) sit()"),
        ("const seated = torso && torso.isAnchored;", "const seated = torso && (!torso.isPushable);"),
        ("log(kid ? kid.isAnchored : '?')", "log(kid ? (!kid.isPushable) : '?')"),
        ("if ('isAnchored' in part) part.isAnchored = false", "if ('isPushable' in part) part.isPushable = true"),
    ];
    for (old, new) in cases {
        assert_eq!(migrate_script(old)?, new, "migrating {old}");
    }
    Ok(())
}

#[test]
fn comments_strings_and_a_scripts_own_functions_stay_as_they_were() -> anyhow::Result<()> {
    let untouched = [
        "// keep it isAnchored so it doesn't fall\nconst x = 1;",
        "console.log('obj.isAnchored = true')",
        "function setObjectAnchored(id, value) { return value }",
        "const helper = { setObjectAnchored(id, value) { return value } }",
    ];
    for code in untouched {
        assert_eq!(migrate_script(code)?, code);
    }
    Ok(())
}
