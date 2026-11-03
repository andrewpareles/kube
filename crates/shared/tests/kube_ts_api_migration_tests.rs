use serde_json::json;
use shared::game::migrateStoredWorld::{StoredWorldKind, updateToVersion};

// The script API package used to be called voidscript-js and is kube-ts-api now, so scripts saved before that name the new one.

fn migrate_script(code: &str, stored_world_kind: StoredWorldKind) -> anyhow::Result<String> {
    let mut stored_world = json!({ "worldSchemaVersion": 91, "scriptRawCodeOfId": { "2": code } });
    updateToVersion(&mut stored_world, 92, stored_world_kind)?;
    Ok(stored_world["scriptRawCodeOfId"]["2"].as_str().unwrap().to_string())
}

#[test]
fn every_mention_of_the_old_package_name_becomes_the_new_one() -> anyhow::Result<()> {
    let cases = [
        ("import { vec3 } from 'voidscript-js';", "import { vec3 } from 'kube-ts-api';"),
        (
            "import { jsx } from \"voidscript-js/jsx-runtime\";",
            "import { jsx } from \"kube-ts-api/jsx-runtime\";",
        ),
        (
            "/** @jsxImportSource voidscript-js */\ngame.guiService.show('hud', <p>Hi</p>);",
            "/** @jsxImportSource kube-ts-api */\ngame.guiService.show('hud', <p>Hi</p>);",
        ),
        (
            "import { vec3 } from 'voidscript-js';\n// everything in voidscript-js is global anyway",
            "import { vec3 } from 'kube-ts-api';\n// everything in kube-ts-api is global anyway",
        ),
    ];
    for (old, new) in cases {
        assert_eq!(migrate_script(old, StoredWorldKind::GameWorld)?, new, "migrating {old}");
    }
    Ok(())
}

#[test]
fn a_downloaded_asset_names_the_new_package_too() -> anyhow::Result<()> {
    assert_eq!(
        migrate_script("import { vec3 } from 'voidscript-js';", StoredWorldKind::AssetObject)?,
        "import { vec3 } from 'kube-ts-api';"
    );
    Ok(())
}

#[test]
fn other_names_stay_as_they_were() -> anyhow::Result<()> {
    let untouched = [
        "const voidscript = 1;",
        "import { x } from 'voidscript-jsx';",
        "import { x } from 'myvoidscript-js';",
        "game.objectService.teleportObjectTo(id, spawn);",
    ];
    for code in untouched {
        assert_eq!(migrate_script(code, StoredWorldKind::GameWorld)?, code);
    }
    Ok(())
}
