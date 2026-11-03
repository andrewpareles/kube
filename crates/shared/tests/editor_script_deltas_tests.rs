mod helpers;

use helpers::world_test_helpers::{add_script, make_world};
use shared::game::world::{World, WorldControlMode};

// Script text is separate from ObjectInfo, but editor saves must still make it into a delta batch.
#[test]
fn editor_script_source_changes_are_dirty_even_without_object_info_changes() -> anyhow::Result<()> {
    let stored = make_world()?.toStoredWorldString()?;
    let mut world = World::fromStoredWorldString(&stored, WorldControlMode::Editor, None)?;
    let root = world.getRootObjectId();
    let script = root + 1;
    add_script(&mut world, script, root)?;
    let added = world.flushDirtyObjects();
    assert!(added.addedObjectIds.contains(&script));
    assert!(!world.flushDirtyObjects().modifiedObjectFlags.contains_key(&script));
    world.setScriptRawCode(script, "const changed = 1;".into());
    let changed = world.flushDirtyObjects();
    assert!(changed.modifiedObjectFlags.contains_key(&script));
    assert_eq!(world.scriptRawCodeOfId[&script], "const changed = 1;");
    let delta = world.buildModifyDelta(script, &changed.modifiedObjectFlags[&script])?;
    assert!(delta.modifiedObjectInfo.is_none());
    assert!(!world.flushDirtyObjects().modifiedObjectFlags.contains_key(&script));
    Ok(())
}
