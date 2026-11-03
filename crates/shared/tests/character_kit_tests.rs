#[path = "helpers/character_kit.rs"]
mod character_kit;
mod helpers;

use character_kit::default_character_asset_object;
use deno_core::JsRuntime;
use game_types::consts::{
    CLIENT_SIM_AND_SEND_SECS, DEFAULT_MAX_DISTANCE_TO_GROUND, DEFAULT_MAX_GROUND_SLOPE_DEGREES, DEHYDRATED_GLTF_CURRENT_VERSION, SERVER_SIM_AND_SEND_SECS,
};
use game_types::object_types::{
    BrickInfo, LocalGroupInfo, MaterialInfo, MeshInfo, MeshMaterialSettings, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, Orientation3D,
    PhysicsInfo3D, PlayerInfo, RigidGroupInfo, material::AlphaMode, shared::Color3, sk_animator::SkAnimatorInfo,
};
use game_types::physics_primitive_types::{BrickPrimitiveSettings, PrimitiveSettings};
use glam::{Mat4, Quat, Vec3};
use helpers::world_test_helpers::add_brick;
use render_core::computeSkeletonPose::computeSkeletonPoseOfIdAtTime;
use render_core::dehydrated_gltf::{DehydratedAnimation, DehydratedGltf, DehydratedMaterial, DehydratedMesh, DehydratedSkeleton};
use render_core::render_scene::computeRenderObjectTransform;
use render_core::renderable::Renderable;
use render_core::renderer_types::RenderObject;
use shared::game::add_dehydrated_gltf_to_world::add_dehydrated_gltf_to_world;
use shared::game::assets::{AssetData, add_asset_to_world};
use shared::game::gltf_converter::{LoadedMeshData, parseBytesIntoLoadedGltf};
use shared::game::jsRuntime::{
    JsConsoleLogEntry, JsRuntimeCameraOrientation, JsRuntimeCameraZoom, JsRuntimeInitialization, ScriptManager, initializeJsAndRunInitialScripts,
    initializeJsRuntime, runJsCallbacks, runPendingJS, runTemporaryScript,
};
use shared::game::world::{JsCallbackInputs, World, WorldControlMode};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

// Games get their players' character by downloading the DefaultCharacter from assets: its spawn script gives each player a
// copy of the Character next to it, and the CharacterController folder that new games start with moves it.

const INITIAL_DRAFT_WORLD: &str = include_str!("../../editor_server/initial-draft-gameworld.kube");

// A new game after the AI downloads the default character. It either leaves the download where it lands, or moves it into the
// organization the system prompt describes: the spawn script in scripts/, the Character in CharacterController, the material in
// materials/ and the animations in animations/.
fn new_game_with_the_default_character(is_reorganized: bool) -> anyhow::Result<String> {
    let mut world = World::fromStoredWorldString(INITIAL_DRAFT_WORLD, WorldControlMode::Editor, None)?;
    let asset_object = default_character_asset_object()?;
    let download = add_asset_to_world(&mut world, "DefaultCharacter".to_string(), AssetData::Object(asset_object))?;
    if is_reorganized {
        let root_object_id = world.getRootObjectId();
        let [character_controller] = world.getObjectIdsByName("CharacterController")[..] else {
            anyhow::bail!("new games should start with a CharacterController folder");
        };
        let materials = world.addObject("materials".to_string(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), root_object_id);
        let animations = world.addObject("animations".to_string(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), root_object_id);
        let scripts = world.addObject("scripts".to_string(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), root_object_id);
        let downloaded_object_ids: Vec<ObjectId> = world.getDirectChildrenIds(download)?.iter().copied().collect();
        for object_id in downloaded_object_ids {
            let folder = match world.getObject(object_id)?.info {
                ObjectInfo::Material(_) => materials,
                ObjectInfo::SkAnimation(_) => animations,
                ObjectInfo::Script(_) => scripts,
                _ => character_controller,
            };
            world.setParent(object_id, folder)?;
        }
        world.removeObject(download)?;
    }
    world.toStoredWorldString()
}

#[test]
fn the_default_character_is_an_asset_object_that_downloads() -> anyhow::Result<()> {
    default_character_asset_object()?.to_object_template("DefaultCharacter")?;
    new_game_with_the_default_character(false)?;
    new_game_with_the_default_character(true)?;
    Ok(())
}

// -------------------------- a server running the game --------------------------

struct Server {
    world: World,
    js_runtime: JsRuntime,
    script_manager_rc: Rc<RefCell<ScriptManager>>,
    js_callback_inputs_rx: mpsc::UnboundedReceiver<JsCallbackInputs>,
    console_log_entry_rx: std::sync::mpsc::Receiver<JsConsoleLogEntry>,
    now_ms: f64,
}

impl Server {
    async fn start(stored_world: &str) -> anyhow::Result<Self> {
        let (js_callback_inputs_tx, js_callback_inputs_rx) = mpsc::unbounded_channel();
        let mut world = World::fromStoredWorldString(stored_world, WorldControlMode::Server, Some(js_callback_inputs_tx))?;
        let (console_log_entry_tx, console_log_entry_rx) = std::sync::mpsc::channel();
        let (mut js_runtime, script_manager_rc) = initializeJsRuntime(
            JsRuntimeInitialization::NoNetworking,
            Some(Arc::new(move |console_log_entry| {
                let _ = console_log_entry_tx.send(console_log_entry);
            })),
        )
        .await?;
        initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
        Ok(Self {
            world,
            js_runtime,
            script_manager_rc,
            js_callback_inputs_rx,
            console_log_entry_rx,
            now_ms: 0.0,
        })
    }

    // Lets the scripts' timers come due and runs whatever is waiting, like one server tick.
    async fn run(&mut self, wait_ms: u64) -> anyhow::Result<()> {
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        self.now_ms += wait_ms as f64;
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

    async fn join(&mut self, username: &str) -> anyhow::Result<ObjectId> {
        let root_object_id = self.world.getRootObjectId();
        let player_object_id = self.world.addObject(
            username.to_string(),
            ObjectBeingAdded::Player(PlayerInfo {
                username: username.to_string(),
                userid: None,
                preferredColor: None,
                equippedItemId: None,
                headObjectId: None,
                characterObjectId: None,
            }),
            root_object_id,
        );
        self.run(0).await?;
        Ok(player_object_id)
    }

    // The player's character and camera head.
    fn character_and_head(&self, player_object_id: ObjectId) -> anyhow::Result<(ObjectId, ObjectId)> {
        let ObjectInfo::Player(player_info) = &self.world.getObject(player_object_id)?.info else {
            anyhow::bail!("{player_object_id} should be a player");
        };
        let (Some(ObjectIdPointer(character_object_id)), Some(ObjectIdPointer(head_object_id))) = (player_info.characterObjectId, player_info.headObjectId)
        else {
            anyhow::bail!("the player should have a character and a head: {player_info:?}");
        };
        Ok((character_object_id, head_object_id))
    }

    fn position(&self, object_id: ObjectId) -> Vec3 {
        self.world.getObjectPhysicsInfo3D(object_id).unwrap().location.position
    }

    fn assert_no_console_errors(&self) {
        let errors: Vec<String> = self
            .console_log_entry_rx
            .try_iter()
            .filter(|console_log_entry| console_log_entry.log_level == "error")
            .map(|console_log_entry| console_log_entry.message)
            .collect();
        assert!(errors.is_empty(), "the kit's scripts logged errors: {errors:?}");
    }
}

#[tokio::test]
async fn each_player_gets_their_own_character_where_the_character_stands() -> anyhow::Result<()> {
    for is_reorganized in [false, true] {
        each_player_gets_their_own_character(is_reorganized).await?;
    }
    Ok(())
}

async fn each_player_gets_their_own_character(is_reorganized: bool) -> anyhow::Result<()> {
    let mut server = Server::start(&new_game_with_the_default_character(is_reorganized)?).await?;
    assert!(
        server.world.getObjectIdsByName("Character").is_empty(),
        "the spawn script should take the Character out of the world"
    );
    assert!(server.world.templateOfName.contains_key("Character"));

    let alice = server.join("alice").await?;
    let bob = server.join("bob").await?;
    let (alice_body, alice_head) = server.character_and_head(alice)?;
    let (bob_body, _) = server.character_and_head(bob)?;
    assert_ne!(alice_body, bob_body);
    assert_eq!(server.world.getObject(alice_body)?.name, "Body");
    assert_eq!(server.world.getObject(alice_head)?.name, "Head");
    assert_eq!(server.world.parentOfId.get(&alice_head).copied(), Some(alice_body));
    assert!(
        server.position(alice_body).length() < 1e-4,
        "alice should stand where the Character stood: {:?}",
        server.position(alice_body)
    );

    for (player_object_id, username) in [(alice, "alice"), (bob, "bob")] {
        let (body, _) = server.character_and_head(player_object_id)?;
        let character = server.world.parentOfId.get(&body).copied().unwrap();
        assert_eq!(server.world.getObject(character)?.name, "Character");
        let nametags: Vec<String> = server
            .world
            .getDirectChildrenIds(character)?
            .iter()
            .filter_map(|child| match &server.world.getObject(*child).unwrap().info {
                ObjectInfo::Label(label_info) => Some(label_info.text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(nametags, [username]);
        // The copies share the download's animations, and animate their own skeleton, which their Body follows.
        for child in server.world.getDirectChildrenIds(character)?.iter().copied() {
            if let ObjectInfo::SkAnimator(SkAnimatorInfo {
                skeletonId: Some(ObjectIdPointer(skeleton_id)),
                skAnimationId: Some(ObjectIdPointer(animation_id)),
                ..
            }) = &server.world.getObject(child)?.info
            {
                assert_ne!(server.world.parentOfId.get(animation_id).copied(), Some(character));
                assert_eq!(server.world.parentOfId.get(skeleton_id).copied(), Some(character));
            }
        }
        let ObjectInfo::RigidGroup(RigidGroupInfo {
            followSkeletonId: Some(ObjectIdPointer(skeleton_id)),
        }) = &server.world.getObject(body)?.info
        else {
            anyhow::bail!("the character's Body should follow a skeleton");
        };
        assert_eq!(server.world.parentOfId.get(skeleton_id).copied(), Some(character));
    }
    server.assert_no_console_errors();
    Ok(())
}

// A Character the AI builds itself might have no part named Head, or be left unpushable.
#[tokio::test]
async fn a_character_with_no_head_left_unpushable_still_spawns_players_who_can_move() -> anyhow::Result<()> {
    let mut stored_world: serde_json::Value = serde_json::from_str(&new_game_with_the_default_character(true)?)?;
    let objects = stored_world["objectOfId"].as_object().unwrap();
    let body = objects
        .iter()
        .find(|(_, object)| object["name"] == "Body")
        .map(|(object_id, _)| object_id.clone())
        .unwrap();
    // The Skeleton has a joint named Head too, which the Body's parts hang from, so only the part is renamed.
    let head = stored_world["childrenOfId"][&body]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.to_string())
        .find(|part| stored_world["objectOfId"][part]["name"] == "Head")
        .unwrap();
    stored_world["objectOfId"][&head]["name"] = "Skull".into();
    for part in stored_world["childrenOfId"][&body].as_array().unwrap().clone() {
        if let Some(physics_toggles) = stored_world["physicsToggles3DOfId"].get_mut(part.to_string()) {
            physics_toggles["isPushable"] = false.into();
        }
    }

    let mut server = Server::start(&stored_world.to_string()).await?;
    let player = server.join("alice").await?;
    let (character, camera_target) = server.character_and_head(player)?;
    assert_eq!(camera_target, character, "with no Head, the camera should follow the Body");
    for part in server.world.getDirectChildrenIds(character)?.iter().copied() {
        if let Some(physics_toggles) = server.world.getObjectPhysicsToggles3D(part) {
            assert!(physics_toggles.isPushable, "the player's character should be free to move");
        }
    }
    server.assert_no_console_errors();
    Ok(())
}

#[tokio::test]
async fn players_who_fall_off_the_world_come_back_where_the_character_stands() -> anyhow::Result<()> {
    let mut server = Server::start(&new_game_with_the_default_character(true)?).await?;
    let falling = server.join("falling").await?;
    let sinking_slowly = server.join("sinking").await?;
    let falling_high_up = server.join("jumper").await?;
    let bodies_before = [falling, sinking_slowly, falling_high_up].map(|player_object_id| server.character_and_head(player_object_id).unwrap().0);
    // The server moves players' characters where their clients say they are, one step at a time.
    let movement_of_body = [
        (bodies_before[0], Vec3::new(30.0, -60.0, 5.0), Vec3::new(0.0, -30.0, 0.0)),
        (bodies_before[1], Vec3::new(30.0, -60.0, 5.0), Vec3::new(0.0, -5.0, 0.0)),
        (bodies_before[2], Vec3::new(30.0, -20.0, 5.0), Vec3::new(0.0, -30.0, 0.0)),
    ];
    for step_index in 0..3 {
        for (body, position, velocity) in movement_of_body {
            let position = position + velocity * SERVER_SIM_AND_SEND_SECS * step_index as f32;
            server
                .world
                .setObjectNextPhysicsInfo3D(body, PhysicsInfo3D::new(position, Quat::IDENTITY, velocity, Vec3::ZERO))?;
        }
        server.world.step_physics(1, SERVER_SIM_AND_SEND_SECS, server.now_ms, None)?;
    }
    server.run(400).await?;
    server.world.step_physics(1, SERVER_SIM_AND_SEND_SECS, server.now_ms, None)?;

    let (respawned_body, _) = server.character_and_head(falling)?;
    assert_eq!(respawned_body, bodies_before[0], "a player who falls off the world should keep their character");
    let respawned_velocity = server.world.getObjectPhysicsInfo3D(respawned_body).unwrap().movement.velocity;
    assert!(
        respawned_velocity.length() < 1e-4,
        "a respawn should jump the player back, not sweep them there: {respawned_velocity:?}"
    );
    assert!(
        server.position(respawned_body).length() < 1e-4,
        "a player falling fast below the world should be back where the Character stood: {:?}",
        server.position(respawned_body)
    );
    for (body, who) in [
        (bodies_before[1], "a player sinking slowly"),
        (bodies_before[2], "a player falling above the world's bottom"),
    ] {
        assert!(server.position(body).y < -10.0, "{who} should be left alone: {:?}", server.position(body));
    }
    server.assert_no_console_errors();
    Ok(())
}

#[tokio::test]
async fn a_player_who_leaves_takes_their_character_with_them() -> anyhow::Result<()> {
    let mut server = Server::start(&new_game_with_the_default_character(true)?).await?;
    let staying = server.join("staying").await?;
    let leaving = server.join("leaving").await?;
    let (leaving_body, _) = server.character_and_head(leaving)?;
    let leaving_character = server.world.parentOfId.get(&leaving_body).copied().unwrap();

    server.world.removeObject(leaving)?;
    server.run(0).await?;
    assert!(
        !server.world.hasObject(leaving_character),
        "the character of a player who left should be deleted"
    );
    assert!(server.world.hasObject(server.character_and_head(staying)?.0));
    server.assert_no_console_errors();
    Ok(())
}

// The appearance script paints each player's Character in their profile's color and swaps the Cap for their hat. Guests have no
// profile, so they get the appearance script's GUEST_COLOR and keep the Cap. The Character starts red here, so the paint shows.
#[tokio::test]
async fn guests_wear_the_default_color_and_keep_the_cap() -> anyhow::Result<()> {
    let mut stored_world: serde_json::Value = serde_json::from_str(&new_game_with_the_default_character(true)?)?;
    for object in stored_world["objectOfId"].as_object_mut().unwrap().values_mut() {
        if (object["name"] == "Torso" || object["name"] == "Cap") && object["info"].get("Brick").is_some() {
            object["info"]["Brick"]["visualSettings"]["color"] = serde_json::json!({ "r": 1.0, "g": 0.0, "b": 0.0 });
        }
    }
    let mut server = Server::start(&stored_world.to_string()).await?;
    let guest = server.join("guest").await?;

    let (body, _) = server.character_and_head(guest)?;
    let part_of_body = |part_name: &str| -> anyhow::Result<ObjectId> {
        server
            .world
            .getDirectChildrenIds(body)?
            .iter()
            .copied()
            .find(|child| server.world.getObject(*child).unwrap().name == part_name)
            .ok_or_else(|| anyhow::anyhow!("the Character should have a {part_name}"))
    };
    for part_name in ["Torso", "Cap"] {
        let ObjectInfo::Brick(BrickInfo { visualSettings, .. }) = &server.world.getObject(part_of_body(part_name)?)?.info else {
            anyhow::bail!("the {part_name} should be a brick");
        };
        let Color3 { r, g, b } = visualSettings.color;
        assert_eq!(
            [r, g, b],
            [0.25, 0.55, 0.9],
            "a guest's {part_name} should be the appearance script's GUEST_COLOR"
        );
    }
    let hats: Vec<ObjectId> = server
        .world
        .getDirectChildrenIds(part_of_body("HatSpot")?)?
        .iter()
        .copied()
        .filter(|child| match server.world.getObject(*child).unwrap().info {
            ObjectInfo::LocalGroup(_) | ObjectInfo::Mesh(_) => true,
            _ => false,
        })
        .collect();
    assert!(hats.is_empty(), "a guest should have no hat: {hats:?}");
    server.assert_no_console_errors();
    Ok(())
}

// A signed-in player's preferredColor is read when they join, so the appearance script paints their character in the same tick it's
// pasted, before it's sent to anyone.
#[tokio::test]
async fn players_wear_the_color_their_profile_prefers() -> anyhow::Result<()> {
    let mut stored_world: serde_json::Value = serde_json::from_str(&new_game_with_the_default_character(true)?)?;
    for object in stored_world["objectOfId"].as_object_mut().unwrap().values_mut() {
        if (object["name"] == "Torso" || object["name"] == "Cap") && object["info"].get("Brick").is_some() {
            object["info"]["Brick"]["visualSettings"]["color"] = serde_json::json!({ "r": 1.0, "g": 0.0, "b": 0.0 });
        }
    }
    let mut server = Server::start(&stored_world.to_string()).await?;
    let root_object_id = server.world.getRootObjectId();
    let player = server.world.addObject(
        "alice".to_string(),
        ObjectBeingAdded::Player(PlayerInfo {
            username: "alice".to_string(),
            userid: Some(17),
            preferredColor: Some(Color3 { r: 0.1, g: 0.8, b: 0.3 }),
            equippedItemId: None,
            headObjectId: None,
            characterObjectId: None,
        }),
        root_object_id,
    );
    server.run(0).await?;

    let (body, _) = server.character_and_head(player)?;
    for part_name in ["Torso", "Cap"] {
        let part = server
            .world
            .getDirectChildrenIds(body)?
            .iter()
            .copied()
            .find(|child| server.world.getObject(*child).unwrap().name == part_name)
            .ok_or_else(|| anyhow::anyhow!("the Character should have a {part_name}"))?;
        let ObjectInfo::Brick(BrickInfo { visualSettings, .. }) = &server.world.getObject(part)?.info else {
            anyhow::bail!("the {part_name} should be a brick");
        };
        let Color3 { r, g, b } = visualSettings.color;
        assert_eq!([r, g, b], [0.1, 0.8, 0.3], "the player's {part_name} should be their preferredColor");
    }
    server.assert_no_console_errors();
    Ok(())
}

// -------------------------- a client playing as one of the server's players --------------------------

const FLOOR_OBJECT_ID: ObjectId = 100_000;
// How many physics steps pass between the times the client lets the scripts' timers run.
const STEPS_PER_TIMER_RUN: usize = 3;

struct Client {
    world: World,
    js_runtime: JsRuntime,
    script_manager_rc: Rc<RefCell<ScriptManager>>,
    js_callback_inputs_rx: mpsc::UnboundedReceiver<JsCallbackInputs>,
    console_log_entry_rx: std::sync::mpsc::Receiver<JsConsoleLogEntry>,
    body: ObjectId,
    now_ms: f64,
    step_count: usize,
}

impl Client {
    // Joins the server and plays in a copy of its world standing on a floor.
    async fn join(server: &mut Server, username: &str) -> anyhow::Result<Self> {
        let player_object_id = server.join(username).await?;
        let (body, _) = server.character_and_head(player_object_id)?;
        let (js_callback_inputs_tx, js_callback_inputs_rx) = mpsc::unbounded_channel();
        let mut world = World::fromStoredWorldString(
            &server.world.toStoredWorldString()?,
            WorldControlMode::Player(player_object_id),
            Some(js_callback_inputs_tx),
        )?;
        let root_object_id = world.getRootObjectId();
        add_brick(&mut world, FLOOR_OBJECT_ID, root_object_id, Vec3::new(0.0, -1.0, 0.0), false)?;
        world.setPrimitiveSettings(
            FLOOR_OBJECT_ID,
            PrimitiveSettings::Brick(BrickPrimitiveSettings {
                size: Vec3::new(400.0, 2.0, 400.0),
            }),
            true,
        )?;
        let (console_log_entry_tx, console_log_entry_rx) = std::sync::mpsc::channel();
        let (mut js_runtime, script_manager_rc) = initializeJsRuntime(
            JsRuntimeInitialization::NoNetworking,
            Some(Arc::new(move |console_log_entry| {
                let _ = console_log_entry_tx.send(console_log_entry);
            })),
        )
        .await?;
        initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
        js_runtime
            .op_state()
            .borrow_mut()
            .borrow_mut::<JsRuntimeCameraOrientation>()
            .set(Orientation3D { theta: 0.0, phi: 0.0 });
        Ok(Self {
            world,
            js_runtime,
            script_manager_rc,
            js_callback_inputs_rx,
            console_log_entry_rx,
            body,
            now_ms: 0.0,
            step_count: 0,
        })
    }

    fn run_callbacks(&mut self, js_callback_inputs: Vec<JsCallbackInputs>) -> anyhow::Result<()> {
        runJsCallbacks(
            &mut self.js_runtime,
            &mut self.world,
            self.now_ms,
            Some(self.now_ms),
            &self.script_manager_rc,
            js_callback_inputs,
        )
    }

    fn press(&mut self, key: &str) -> anyhow::Result<()> {
        self.run_callbacks(vec![JsCallbackInputs::KeyDown { key: key.to_string() }])
    }

    fn release(&mut self, key: &str) -> anyhow::Result<()> {
        self.run_callbacks(vec![JsCallbackInputs::KeyUp { key: key.to_string() }])
    }

    // Physics steps the way the client play loop runs them, letting the scripts' timers run in between.
    async fn step(&mut self, step_count: usize) -> anyhow::Result<()> {
        for _ in 0..step_count {
            self.now_ms += CLIENT_SIM_AND_SEND_SECS as f64 * 1000.0;
            self.run_callbacks(vec![JsCallbackInputs::BeforeCharacterStep {
                character_object_id: self.body,
                dt_secs: CLIENT_SIM_AND_SEND_SECS,
            }])?;
            self.world.step_physics(1, CLIENT_SIM_AND_SEND_SECS, self.now_ms, Some(self.now_ms))?;
            self.run_callbacks(vec![JsCallbackInputs::AfterCharacterStep {
                character_object_id: self.body,
                dt_secs: CLIENT_SIM_AND_SEND_SECS,
            }])?;
            self.step_count += 1;
            if self.step_count % STEPS_PER_TIMER_RUN == 0 {
                tokio::time::sleep(Duration::from_secs_f32(CLIENT_SIM_AND_SEND_SECS * STEPS_PER_TIMER_RUN as f32)).await;
                runPendingJS(
                    &mut self.js_runtime,
                    &mut self.world,
                    self.now_ms,
                    Some(self.now_ms),
                    &self.script_manager_rc,
                    &mut self.js_callback_inputs_rx,
                )
                .await?;
            }
        }
        Ok(())
    }

    fn physics(&self) -> PhysicsInfo3D {
        self.world.getObjectPhysicsInfo3D(self.body).expect("the character should be a physical object")
    }

    fn is_grounded(&self) -> bool {
        self.world
            .getGroundSupport(self.body, DEFAULT_MAX_DISTANCE_TO_GROUND, DEFAULT_MAX_GROUND_SLOPE_DEGREES)
            .is_some()
    }

    // The opacity of every brick inside the Body, as this client draws it.
    fn body_brick_opacities(&self) -> BTreeMap<ObjectId, f32> {
        let mut object_ids = vec![self.body];
        let mut opacity_of_brick_id = BTreeMap::new();
        while let Some(object_id) = object_ids.pop() {
            object_ids.extend(self.world.getDirectChildrenIds(object_id).unwrap().iter().copied());
            if let ObjectInfo::Brick(brick_info) = &self.world.getObject(object_id).unwrap().info {
                opacity_of_brick_id.insert(object_id, brick_info.visualSettings.opacity);
            }
        }
        opacity_of_brick_id
    }

    // The weight each animator of this character is at or heading to.
    fn animator_weights(&self) -> BTreeMap<String, f32> {
        let character = self.world.parentOfId.get(&self.body).copied().unwrap();
        self.world
            .getDirectChildrenIds(character)
            .unwrap()
            .iter()
            .filter_map(|child| {
                let object = self.world.getObject(*child).unwrap();
                let ObjectInfo::SkAnimator(animator_info) = &object.info else {
                    return None;
                };
                let weight = animator_info
                    .weightTransition
                    .as_ref()
                    .map_or(animator_info.weight, |weight_transition| weight_transition.finalValue);
                Some((object.name.clone(), weight))
            })
            .collect()
    }

    fn assert_playing(&self, animator_name: &str) {
        let animator_weights = self.animator_weights();
        for (name, weight) in &animator_weights {
            let expected_weight = if name == animator_name { 100.0 } else { 0.0 };
            assert!(
                (weight - expected_weight).abs() < 1e-3,
                "{animator_name} should be playing alone: {animator_weights:?}"
            );
        }
    }

    fn assert_no_console_errors(&self) {
        let errors: Vec<String> = self
            .console_log_entry_rx
            .try_iter()
            .filter(|console_log_entry| console_log_entry.log_level == "error")
            .map(|console_log_entry| console_log_entry.message)
            .collect();
        assert!(errors.is_empty(), "the kit's scripts logged errors: {errors:?}");
    }
}

#[tokio::test]
async fn the_character_walks_jumps_and_animates_to_match() -> anyhow::Result<()> {
    let mut server = Server::start(&new_game_with_the_default_character(true)?).await?;
    let mut client = Client::join(&mut server, "alice").await?;
    client.step(30).await?;
    assert!(client.is_grounded(), "the character should stand on the floor: {:?}", client.physics());
    client.assert_playing("IdleAnimator");

    let start = client.physics().location.position;
    client.press("W")?;
    client.step(60).await?;
    let travelled = client.physics().location.position - start;
    assert!(
        travelled.z > 5.0 && travelled.x.abs() < 0.5,
        "W should walk the character forward: {travelled:?}"
    );
    assert!(client.is_grounded(), "the character should keep its footing while walking");
    assert!((client.physics().location.quaternion * Vec3::Y).y > 0.99, "the character should stay upright");
    client.assert_playing("WalkAnimator");

    client.press("Space")?;
    client.step(6).await?;
    assert!(!client.is_grounded(), "Space should jump: {:?}", client.physics());
    client.step(12).await?;
    client.assert_playing("JumpAnimator");

    client.release("W")?;
    client.release("Space")?;
    client.step(90).await?;
    assert!(client.is_grounded(), "the character should land");
    client.assert_playing("IdleAnimator");
    client.assert_no_console_errors();
    Ok(())
}

// The kit's zoom_fade script hides the player's own Body on their screen when their camera zooms in on its head, and shows it again
// when the camera zooms back out.
#[tokio::test]
async fn the_players_own_character_fades_when_their_camera_zooms_in() -> anyhow::Result<()> {
    let mut server = Server::start(&new_game_with_the_default_character(true)?).await?;
    let mut client = Client::join(&mut server, "alice").await?;
    let zoomed_out_opacities = client.body_brick_opacities();
    assert!(
        zoomed_out_opacities.values().any(|opacity| *opacity > 0.0),
        "the Body should have visible bricks: {zoomed_out_opacities:?}"
    );

    client.js_runtime.op_state().borrow_mut().borrow_mut::<JsRuntimeCameraZoom>().set(0.2);
    client.step(STEPS_PER_TIMER_RUN).await?;
    let zoomed_in_opacities = client.body_brick_opacities();
    assert!(
        zoomed_in_opacities.values().all(|opacity| *opacity == 0.0),
        "every brick should be hidden: {zoomed_in_opacities:?}"
    );

    client.js_runtime.op_state().borrow_mut().borrow_mut::<JsRuntimeCameraZoom>().set(10.0);
    client.step(STEPS_PER_TIMER_RUN).await?;
    assert_eq!(client.body_brick_opacities(), zoomed_out_opacities);
    client.assert_no_console_errors();
    Ok(())
}

// -------------------------- a character made from one rigged model --------------------------

// The AI chat makes a character in Blender as one mesh skinned to one armature, with an action for each animation, and imports it
// as one .glb. fixtures/rigged_character.py builds this one in the sandbox's Blender, with bones named like the default
// character's joints.
const RIGGED_CHARACTER_GLB: &[u8] = include_bytes!("fixtures/rigged_character.glb");

// The import as the editor stores it, with numbers in place of the mesh and texture data it would upload.
fn rigged_character_import() -> anyhow::Result<DehydratedGltf> {
    let loaded_gltf = parseBytesIntoLoadedGltf(RIGGED_CHARACTER_GLB)?;
    loaded_gltf.validate()?;
    let mut dehydrated_meshes = Vec::new();
    let mut dehydrated_skeletons: Vec<DehydratedSkeleton> = Vec::new();
    for (data_id, loaded_mesh) in (1..).zip(loaded_gltf.meshes) {
        let LoadedMeshData::LoadedSkinnedMeshData {
            name,
            loadedTextures,
            skeleton_index,
            jointOfIndex,
            animations,
            ..
        } = loaded_mesh
        else {
            anyhow::bail!("every part of the rigged character should be skinned");
        };
        if dehydrated_skeletons.iter().all(|skeleton| skeleton.skeleton_index != skeleton_index) {
            dehydrated_skeletons.push(DehydratedSkeleton {
                skeleton_index,
                jointOfIndex,
                animations: animations
                    .into_iter()
                    .map(|animation| DehydratedAnimation {
                        name: animation.name,
                        sk_animation_info: animation.sk_animation_info,
                        sk_keyframes: animation.sk_keyframes,
                    })
                    .collect(),
            });
        }
        dehydrated_meshes.push(DehydratedMesh {
            name,
            mesh_data_id: data_id,
            skinned_skeleton_index: Some(skeleton_index),
            material: Some(DehydratedMaterial {
                name: loadedTextures.name,
                material_info: MaterialInfo {
                    albedoTextureDataId: Some(data_id),
                    metallicTextureDataId: None,
                    roughnessTextureDataId: None,
                    normalTextureDataId: None,
                    alphaMode: AlphaMode::Opaque,
                    isDoubleSided: false,
                },
            }),
        });
    }
    Ok(DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id: None,
        import_scale: 1.0,
        dehydrated_meshes,
        dehydrated_skeletons,
    })
}

// What the system prompt has the AI do after importing a rigged model, as the temporary script it would run: the model's
// skeleton and meshes replace the Character's, its materials go in materials/, the Body's bricks stay as its invisible collider,
// and the Character's animators play the model's animations.
const PUT_RIGGED_MODEL_ON_CHARACTER_SCRIPT: &str = r#"
const os = game.objectService;
const character = os.getObjectByName("CharacterController")!.getChild("Character")!;
const body = character.getChild("Body")!;
const model = os.getObjectByName("RobotModel")!;
const skeleton = model.getChild("skeleton_1")!;
if (body.type !== "rigidGroup") throw new Error("the Body should be a RigidGroup");

os.deleteObject(character.getChild("Skeleton")!.id);
skeleton.parent = character;
body.followSkeletonId = skeleton.id;
const materials = os.getObjectByName("materials")!;
for (const mesh of model.getChildren()) {
  if (mesh.type !== "mesh") continue;
  for (const material of mesh.getChildren()) material.parent = materials;
  mesh.parent = body;
  mesh.physicalObjectId = body.id;
}
for (const part of body.getChildren()) {
  if (part.type === "brick") part.opacity = 0;
}

const animations = os.getObjectByName("animations")!;
for (const oldAnimation of animations.getChildren()) os.deleteObject(oldAnimation.id);
for (const [animatorName, animationName] of [["IdleAnimator", "Idle"], ["WalkAnimator", "Walk"], ["JumpAnimator", "Jump"]]) {
  const animation = skeleton.getChild(animationName)!;
  animation.parent = animations;
  const animator = character.getChild(animatorName)!;
  if (animator.type !== "skAnimator") throw new Error(`${animatorName} should be an SkAnimator`);
  animator.skeletonId = skeleton.id;
  animator.skAnimationId = animation.id;
}
for (const importedAnimator of skeleton.getChildren()) {
  if (importedAnimator.type === "skAnimator") os.deleteObject(importedAnimator.id);
}
os.deleteObject(model.id);
"#;

fn new_game_with_a_rigged_character() -> anyhow::Result<String> {
    let mut world = World::fromStoredWorldString(&new_game_with_the_default_character(true)?, WorldControlMode::Editor, None)?;
    let root_object_id = world.getRootObjectId();
    let model = add_dehydrated_gltf_to_world(&mut world, "RobotModel", rigged_character_import()?, root_object_id)?;

    // The import keeps the names the model has in Blender: its object, its materials, its bones and its actions.
    let name_of = |object_id: &ObjectId| world.getObject(*object_id).unwrap().name.clone();
    let mesh_ids: Vec<ObjectId> = world
        .getDirectChildrenIds(model)?
        .iter()
        .copied()
        .filter(|object_id| match world.getObject(*object_id).unwrap().info {
            ObjectInfo::Mesh(_) => true,
            _ => false,
        })
        .collect();
    assert!(
        mesh_ids.iter().all(|mesh_id| name_of(mesh_id) == "Robot"),
        "each mesh should keep the Blender object's name"
    );
    let mut material_names: Vec<String> = mesh_ids
        .iter()
        .flat_map(|mesh_id| world.getDirectChildrenIds(*mesh_id).unwrap().iter().map(name_of).collect::<Vec<_>>())
        .collect();
    material_names.sort();
    assert_eq!(material_names, ["Pants", "Shirt", "Shoes", "Skin"]);
    let [skeleton] = world.getObjectIdsByName("skeleton_1")[..] else {
        anyhow::bail!("the import should have one skeleton");
    };
    let mut joint_and_animation_names = Vec::new();
    let mut object_ids_to_visit = vec![skeleton];
    while let Some(object_id) = object_ids_to_visit.pop() {
        if let ObjectInfo::SkJoint(_) | ObjectInfo::SkAnimation(_) = world.getObject(object_id)?.info {
            joint_and_animation_names.push(name_of(&object_id));
        }
        object_ids_to_visit.extend(world.getDirectChildrenIds(object_id)?.iter().copied());
    }
    for name in ["Hips", "Torso", "Head", "LeftArm", "RightArm", "LeftLeg", "RightLeg", "Idle", "Walk", "Jump"] {
        assert!(
            joint_and_animation_names.iter().any(|joint_or_animation| joint_or_animation == name),
            "the import should have {name}"
        );
    }

    let console_log_entries = runTemporaryScript(&mut world, PUT_RIGGED_MODEL_ON_CHARACTER_SCRIPT.to_string())?;
    let errors: Vec<&String> = console_log_entries
        .iter()
        .filter(|console_log_entry| console_log_entry.log_level == "error")
        .map(|console_log_entry| &console_log_entry.message)
        .collect();
    assert!(errors.is_empty(), "putting the model on the Character logged errors: {errors:?}");
    world.toStoredWorldString()
}

impl Client {
    // Where the meshes on this player's Body are drawn, and the left leg's posed matrix, at this moment of the animations. Each
    // mesh bends with the skeleton the Body follows.
    fn drawn_meshes_and_left_leg(&self, time_ms: f64) -> (Vec<Vec3>, Mat4) {
        let ObjectInfo::RigidGroup(RigidGroupInfo {
            followSkeletonId: Some(ObjectIdPointer(skeleton_id)),
        }) = self.world.getObject(self.body).unwrap().info
        else {
            panic!("the Body should follow a skeleton");
        };
        let render_object_of_id = self.world.getRenderSnapshot().render_object_of_id;
        let skeleton_pose_of_id = computeSkeletonPoseOfIdAtTime(&render_object_of_id, time_ms, Some(time_ms));
        let mesh_positions = render_object_of_id
            .iter()
            .filter_map(|(object_id, render_object)| {
                let RenderObject::Mesh(mesh) = render_object else {
                    return None;
                };
                if mesh.physicalObjectId != self.body {
                    return None;
                }
                assert_eq!(
                    mesh.skin_skeleton_id,
                    Some(skeleton_id),
                    "the meshes should bend with the skeleton the Body follows"
                );
                let (_, _, position) = computeRenderObjectTransform(&render_object_of_id, &skeleton_pose_of_id, *object_id)
                    .unwrap()
                    .to_scale_rotation_translation();
                Some(position)
            })
            .collect();
        let RenderObject::Skeleton(skeleton) = &render_object_of_id[&skeleton_id] else {
            panic!("{skeleton_id} should be a skeleton");
        };
        let left_leg_index = skeleton.renderSkeletonInfoArc.jointIndexOfName["LeftLeg"];
        (mesh_positions, skeleton_pose_of_id[&skeleton_id].jointMatrixOfJointIndex[left_leg_index])
    }
}

#[tokio::test]
async fn a_character_made_from_one_rigged_model_walks_jumps_and_animates() -> anyhow::Result<()> {
    let mut server = Server::start(&new_game_with_a_rigged_character()?).await?;
    let mut client = Client::join(&mut server, "alice").await?;
    // The character lands just after it spawns, so this waits out the fade from jumping to standing.
    client.step(60).await?;
    assert!(client.is_grounded(), "the character should stand on the floor: {:?}", client.physics());
    client.assert_playing("IdleAnimator");
    let (mesh_positions, idle_left_leg) = client.drawn_meshes_and_left_leg(client.now_ms);
    assert_eq!(mesh_positions.len(), 4, "the model's four materials should each be a mesh on the Body");
    // The player's copy of the Character uses the model's Materials in materials/ instead of carrying its own.
    let mut material_folder_names = Vec::new();
    for part_id in client.world.getDirectChildrenIds(client.body)?.iter().copied() {
        let ObjectInfo::Mesh(MeshInfo {
            material: Some(MeshMaterialSettings {
                materialInfoId: ObjectIdPointer(material_id),
            }),
            ..
        }) = client.world.getObject(part_id)?.info
        else {
            continue;
        };
        material_folder_names.push(client.world.getObject(client.world.parentOfId[&material_id])?.name.clone());
    }
    assert_eq!(material_folder_names, ["materials"; 4]);
    let (_, later_idle_left_leg) = client.drawn_meshes_and_left_leg(client.now_ms + 250.0);
    assert!(
        idle_left_leg.abs_diff_eq(later_idle_left_leg, 1e-4),
        "standing still should keep the legs still"
    );

    let start = client.physics().location.position;
    client.press("W")?;
    client.step(60).await?;
    let travelled = client.physics().location.position - start;
    assert!(
        travelled.z > 5.0 && travelled.x.abs() < 0.5,
        "W should walk the character forward: {travelled:?}"
    );
    client.assert_playing("WalkAnimator");
    let body_position = client.physics().location.position;
    let (mesh_positions, walk_left_leg) = client.drawn_meshes_and_left_leg(client.now_ms);
    for mesh_position in mesh_positions {
        assert!(
            mesh_position.distance(body_position) < 0.5,
            "the meshes should be drawn where the Body walked to: {mesh_position} vs {body_position}"
        );
    }
    let (_, later_walk_left_leg) = client.drawn_meshes_and_left_leg(client.now_ms + 250.0);
    assert!(!walk_left_leg.abs_diff_eq(later_walk_left_leg, 1e-2), "walking should swing the legs");

    client.press("Space")?;
    client.step(6).await?;
    assert!(!client.is_grounded(), "Space should jump: {:?}", client.physics());
    client.step(12).await?;
    client.assert_playing("JumpAnimator");

    client.release("W")?;
    client.release("Space")?;
    client.step(90).await?;
    assert!(client.is_grounded(), "the character should land");
    client.assert_playing("IdleAnimator");
    client.assert_no_console_errors();
    server.assert_no_console_errors();
    Ok(())
}
