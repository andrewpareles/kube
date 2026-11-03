mod helpers;

use std::cell::RefCell;
use std::rc::Rc;

use deno_core::JsRuntime;
use game_types::object_types::{BrickInfo, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation, PlayerInfo, ScriptInfo, VisualSettings};
use game_types::physics_primitive_types::BrickPrimitiveSettings;
use glam::Vec3;
use helpers::world_test_helpers::{add_mesh, add_rigid_group, make_world_controlled_by_player, physics_info, physics_toggles};
use shared::game::jsRuntime::{
    JsRuntimeCameraZoom, JsRuntimeInitialization, ScriptManager, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS,
};
use shared::game::world::{JsCallbackInputs, World};
use tokio::sync::mpsc;

// The zoom_fade script runs on the local player's client. The play loop hands it the camera's zoom each tick, and it fades the parts of
// the RigidGroup holding the player's head.

const ZOOM_FADE_SCRIPT: &str = include_str!("../../../assets/scripts/zoom_fade.ts");
const PLAYER_OBJECT_ID: ObjectId = 99;
const SCRIPT_OBJECT_ID: ObjectId = 2;
const BODY_ID: ObjectId = 10;
const HEAD_ID: ObjectId = 11;
const HAT_ID: ObjectId = 12;
const TORSO_ID: ObjectId = 13;
const COLLIDER_ID: ObjectId = 14;
const WALL_ID: ObjectId = 20;
const LONE_HEAD_ID: ObjectId = 30;

fn add_brick_with_opacity(world: &mut World, object_id: ObjectId, parent_id: ObjectId, opacity: f32) {
    let brick_info = BrickInfo {
        material: None,
        visualSettings: VisualSettings {
            opacity,
            ..VisualSettings::TYPICAL_OPAQUE
        },
    };
    world.addObjectWithId(
        object_id,
        format!("brick-{object_id}"),
        ObjectBeingAdded::Brick(
            brick_info,
            BrickPrimitiveSettings { size: Vec3::ONE },
            physics_info(Vec3::new(object_id as f32 * 3.0, 0.0, 0.0)),
            physics_toggles(true, 1.0),
        ),
        parent_id,
    );
}

fn opacity_of(world: &World, object_id: ObjectId) -> anyhow::Result<f32> {
    match &world.getObject(object_id)?.info {
        ObjectInfo::Brick(brick_info) => Ok(brick_info.visualSettings.opacity),
        ObjectInfo::Mesh(mesh_info) => Ok(mesh_info.visualSettings.opacity),
        _ => Err(anyhow::anyhow!("object {object_id} has no opacity")),
    }
}

fn set_opacity(world: &mut World, object_id: ObjectId, opacity: f32) -> anyhow::Result<()> {
    match &mut world.getObjectMut(object_id)?.info {
        ObjectInfo::Brick(brick_info) => brick_info.visualSettings.opacity = opacity,
        ObjectInfo::Mesh(mesh_info) => mesh_info.visualSettings.opacity = opacity,
        _ => return Err(anyhow::anyhow!("object {object_id} has no opacity")),
    }
    Ok(())
}

fn set_player_head(world: &mut World, head_object_id: ObjectId) -> anyhow::Result<()> {
    let ObjectInfo::Player(player_info) = &world.getObject(PLAYER_OBJECT_ID)?.info else {
        return Err(anyhow::anyhow!("the player is missing"));
    };
    let player_info = PlayerInfo {
        headObjectId: Some(ObjectIdPointer(head_object_id)),
        ..player_info.clone()
    };
    world.setObjectInfo(PLAYER_OBJECT_ID, ObjectInfo::Player(player_info))
}

// A Body holding a Head with a hat, a half-see-through Torso and an invisible collider, a wall that isn't part of the Body, and a
// lone head that isn't in any RigidGroup.
fn make_character_world() -> anyhow::Result<World> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    add_rigid_group(&mut world, BODY_ID, root_object_id, Vec3::ZERO, None)?;
    add_brick_with_opacity(&mut world, HEAD_ID, BODY_ID, 1.0);
    add_mesh(&mut world, HAT_ID, HEAD_ID)?;
    add_brick_with_opacity(&mut world, TORSO_ID, BODY_ID, 0.5);
    add_brick_with_opacity(&mut world, COLLIDER_ID, BODY_ID, 0.0);
    add_brick_with_opacity(&mut world, WALL_ID, root_object_id, 1.0);
    add_brick_with_opacity(&mut world, LONE_HEAD_ID, root_object_id, 1.0);
    world.addObjectWithId(
        PLAYER_OBJECT_ID,
        "player".to_string(),
        ObjectBeingAdded::Player(PlayerInfo {
            username: "player".to_string(),
            userid: None,
            preferredColor: None,
            equippedItemId: None,
            headObjectId: Some(ObjectIdPointer(HEAD_ID)),
            characterObjectId: Some(ObjectIdPointer(BODY_ID)),
        }),
        root_object_id,
    );
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "zoom_fade".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(SCRIPT_OBJECT_ID, ZOOM_FADE_SCRIPT.to_string());
    Ok(world)
}

struct ZoomFadeRuntime {
    world: World,
    js_runtime: JsRuntime,
    script_manager_rc: Rc<RefCell<ScriptManager>>,
    js_callback_inputs_rx: mpsc::UnboundedReceiver<JsCallbackInputs>,
    now_ms: f64,
}

impl ZoomFadeRuntime {
    async fn new(mut world: World) -> anyhow::Result<Self> {
        let (js_callback_inputs_tx, js_callback_inputs_rx) = mpsc::unbounded_channel();
        world.js_callback_inputs_tx = Some(js_callback_inputs_tx);
        let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
        initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
        Ok(Self {
            world,
            js_runtime,
            script_manager_rc,
            js_callback_inputs_rx,
            now_ms: 0.0,
        })
    }

    // Like the play loop: the camera's zoom is handed to the runtime, then the script's tick loop runs once.
    async fn run_tick_at_zoom(&mut self, zoom: f32) -> anyhow::Result<()> {
        self.js_runtime.op_state().borrow_mut().borrow_mut::<JsRuntimeCameraZoom>().set(zoom);
        self.now_ms += 16.0;
        runPendingJS(
            &mut self.js_runtime,
            &mut self.world,
            self.now_ms,
            Some(self.now_ms),
            &self.script_manager_rc,
            &mut self.js_callback_inputs_rx,
        )
        .await
    }

    fn opacities(&self, object_ids: &[ObjectId]) -> anyhow::Result<Vec<f32>> {
        object_ids.iter().map(|object_id| opacity_of(&self.world, *object_id)).collect()
    }
}

const BODY_PART_IDS: [ObjectId; 4] = [HEAD_ID, HAT_ID, TORSO_ID, COLLIDER_ID];

#[tokio::test]
async fn the_character_fades_out_as_the_camera_zooms_in_and_comes_back_as_it_zooms_out() -> anyhow::Result<()> {
    let mut runtime = ZoomFadeRuntime::new(make_character_world()?).await?;

    runtime.run_tick_at_zoom(10.0).await?;
    assert_eq!(runtime.opacities(&BODY_PART_IDS)?, [1.0, 1.0, 0.5, 0.0]);

    runtime.run_tick_at_zoom(2.0).await?;
    assert_eq!(
        runtime.opacities(&BODY_PART_IDS)?,
        [0.5, 0.5, 0.25, 0.0],
        "halfway between where the fade starts and ends"
    );

    runtime.run_tick_at_zoom(0.2).await?;
    assert_eq!(runtime.opacities(&BODY_PART_IDS)?, [0.0, 0.0, 0.0, 0.0]);
    assert_eq!(opacity_of(&runtime.world, WALL_ID)?, 1.0, "objects outside the Body never fade");

    runtime.run_tick_at_zoom(10.0).await?;
    assert_eq!(runtime.opacities(&BODY_PART_IDS)?, [1.0, 1.0, 0.5, 0.0], "every part gets its own opacity back");
    Ok(())
}

// A part's opacity that the game changes while the part is faded becomes its own opacity, whether it's still faded or not.
#[tokio::test]
async fn the_game_can_change_a_faded_parts_opacity() -> anyhow::Result<()> {
    let mut runtime = ZoomFadeRuntime::new(make_character_world()?).await?;

    runtime.run_tick_at_zoom(2.0).await?;
    set_opacity(&mut runtime.world, TORSO_ID, 0.8)?;
    runtime.run_tick_at_zoom(2.0).await?;
    assert_eq!(opacity_of(&runtime.world, TORSO_ID)?, 0.4);

    set_opacity(&mut runtime.world, HAT_ID, 0.3)?;
    runtime.run_tick_at_zoom(10.0).await?;
    assert_eq!(runtime.opacities(&[HEAD_ID, HAT_ID, TORSO_ID])?, [1.0, 0.3, 0.8]);
    Ok(())
}

// A head that isn't in any RigidGroup fades by itself. The Body the player's camera left gets its own opacities back.
#[tokio::test]
async fn a_new_head_fades_and_the_old_character_comes_back() -> anyhow::Result<()> {
    let mut runtime = ZoomFadeRuntime::new(make_character_world()?).await?;

    runtime.run_tick_at_zoom(0.2).await?;
    assert_eq!(runtime.opacities(&BODY_PART_IDS)?, [0.0, 0.0, 0.0, 0.0]);

    set_player_head(&mut runtime.world, LONE_HEAD_ID)?;
    runtime.run_tick_at_zoom(0.2).await?;
    assert_eq!(runtime.opacities(&BODY_PART_IDS)?, [1.0, 1.0, 0.5, 0.0]);
    assert_eq!(runtime.opacities(&[LONE_HEAD_ID, WALL_ID])?, [0.0, 1.0]);
    Ok(())
}
