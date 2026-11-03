use crate::conversions::float_conversions::with_short_floats;
use crate::game::add_dehydrated_gltf_to_world::add_dehydrated_gltf_to_world;
use crate::game::catalog::download_catalog_item;
use crate::game::gui_types::GuiRenderedRoot;
use crate::game::jsRuntime::{
    MouseCursor, MouseCursorImage, get_camera_orientation_from_opstate, get_camera_zoom_from_opstate, get_client_world_delay_ms_from_opstate,
    get_desired_camera_zoom_from_opstate, get_live_sim_ms_from_opstate, get_mouse_ray_from_opstate, get_player_ping_ms_from_opstate,
    get_script_manager_mut_from_opstate, get_server_fn_caller_object_id_from_opstate, get_world_change_count_from_opstate, get_world_from_opstate,
    get_world_mut_from_opstate,
};
use crate::game::jsRustConversions::{
    ValueWithPath, jsLiveObjectOfRustLiveObject, jsObjectId, jsVec3, rustObjectBeingAddedOfJSObjectInfo, rustUpdateObject, rustVec3, rustVec3RadiansFromDegrees,
};
use crate::game::physics::{CastHit, SetOrForce, SetPhysicsInfo3D};
use crate::game::world::WorldControlMode;
use crate::helpers::multi_thread_hashmap::MultiThreadHashMap;
use crate::player_profile::{PlayerProfile, get_player_profile};
use crate::protocol::messages::{ClientFunctionCallMessage, ClientFunctionResultMessage, ClientMessageTCP, ServerFnCallMessage, ServerFunctionResultMessage};
use crate::protocol::quic_send_receive_helpers::{MAX_TCP_MESSAGE_BYTES, awaitTcpMessageMaxBytes, getPingMs, sendTcpMessage, sendTcpRequest};
use deno_core::{Extension, OpState, op2, v8};
use deno_error::JsErrorBox;
use deno_web::StartTime;
use game_types::{
    consts::{DEFAULT_MAX_DISTANCE_TO_GROUND, DEFAULT_MAX_GROUND_SLOPE_DEGREES, MAX_MOUSE_CURSOR_SIZE, MAX_PLAYER_METADATA_BYTES, MIN_CAMERA_ZOOM},
    object_types::{
        InterpolationStyle, ObjectBeingAdded, ObjectId, ObjectInfo, ObjectKind, ObjectLocation, Orientation3D, PlayerInfo, WeightTransition,
        shared::{euler_degrees_to_quat, quat_to_euler_degrees},
    },
};
use glam::{Mat3, Quat, Vec3};
use quinn::Connection;
use render_core::{
    camera::{MAX_CAMERA_FOV_DEGREES, MIN_CAMERA_FOV_DEGREES},
    dehydrated_gltf::DehydratedGltf,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use tokio::sync::oneshot;

fn objectIdFromI64(id: i64) -> ObjectId {
    id as ObjectId
}

fn jsError(error: impl std::fmt::Display) -> JsErrorBox {
    JsErrorBox::generic(error.to_string())
}

// -------------------------- Conversions ---------------

fn get_time_location(world_control_mode: &WorldControlMode) -> ObjectLocation {
    match world_control_mode {
        WorldControlMode::Server => ObjectLocation::Server,
        WorldControlMode::Editor | WorldControlMode::Player(_) => ObjectLocation::Client,
    }
}

// -------------------------- Op Runtime Access Rules ---------------
// look at the comments in __globalOps__.ts for the source of truth of what we should assert here.
// CLIENT SERVER BOTH says where the op should live.

// These helpers keep the runtime-side gates in one place so wrong-side calls fail consistently.
fn assertServer(state: &OpState, op_name: &str) -> Result<(), JsErrorBox> {
    let world = get_world_from_opstate(state).map_err(jsError)?;
    match &world.worldControlMode {
        WorldControlMode::Server => Ok(()),
        WorldControlMode::Editor | WorldControlMode::Player(_) => Err(jsError(format!("{op_name}: only available on the server"))),
    }
}
fn assertClient(state: &OpState, op_name: &str) -> Result<(), JsErrorBox> {
    let world = get_world_from_opstate(state).map_err(jsError)?;
    match &world.worldControlMode {
        WorldControlMode::Player(_) => Ok(()),
        WorldControlMode::Editor | WorldControlMode::Server => Err(jsError(format!("{op_name}: only available on the client"))),
    }
}
fn assertBoth(state: &OpState, _op_name: &str) -> Result<(), JsErrorBox> {
    let _world = get_world_from_opstate(state).map_err(jsError)?;
    Ok(())
}

// Ops:
#[op2(fast)]
#[number]
fn __getRustRootObjectId__(state: &mut OpState) -> Result<i64, JsErrorBox> {
    assertBoth(state, "__getRustRootObjectId__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    Ok(jsObjectId(world.getRootObjectId()))
}

#[op2(fast)]
#[number]
fn __getWorldChangeCount__(state: &mut OpState) -> Result<u64, JsErrorBox> {
    assertBoth(state, "__getWorldChangeCount__")?;
    get_world_change_count_from_opstate(state).map_err(jsError)
}

#[op2]
fn __getRustObject__<'scope>(
    state: &mut OpState,
    scope: &mut v8::PinScope<'scope, '_>,
    // params:
    #[number] object_id: i64,
) -> Result<Option<v8::Local<'scope, v8::Value>>, JsErrorBox> {
    assertBoth(state, "__getRustObject__")?;
    let obj_id = objectIdFromI64(object_id);
    let world = get_world_from_opstate(state).map_err(jsError)?;

    // if object does not exist, return null instead of erroring later
    let Ok(_) = world.getObject(obj_id) else {
        return Ok(None);
    };

    let js_obj = jsLiveObjectOfRustLiveObject(world, obj_id, scope).map_err(jsError)?;
    Ok(Some(js_obj.into()))
}

#[op2(fast)]
fn __setRustObjectField__(
    state: &mut OpState,
    // params:
    #[number] jsObjectId: i64,
    #[string] jspath: String,
    #[string] jsvalue: String, // internally, a value of null means deletion in some cases, see _internal
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__setRustObjectField__")?;
    let jspath: Vec<String> = serde_json::from_str(&jspath).map_err(jsError)?;
    let jsvalue = serde_json::from_str(&jsvalue).map_err(jsError)?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;

    let objectId = objectIdFromI64(jsObjectId);
    if let WorldControlMode::Player(_) = world.worldControlMode
        && let Ok(object) = world.getObject(objectId)
        && let ObjectInfo::Player(_) = object.info
        && matches!(jspath.first().map(String::as_str), Some("characterObjectId" | "headObjectId"))
    {
        return Err(jsError("Only the server can change a player's character or head"));
    }

    let valueWithPath = ValueWithPath::new(&jsvalue, &jspath);
    let objectModifyFlags = rustUpdateObject(world, objectId, valueWithPath, SetOrForce::Set).map_err(jsError)?;
    world.serverMarkObjectAsModified(objectId, objectModifyFlags);
    Ok(())
}

#[op2(fast)]
fn __deleteRustObj__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__deleteRustObj__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let objectId = objectIdFromI64(object_id);
    if let WorldControlMode::Player(_) = world.worldControlMode {
        world.canDeleteOnClientOrErr(objectId).map_err(jsError)?;
    }
    world.removeObject(objectId).map_err(jsError)?;
    Ok(())
}

#[op2(fast)]
#[number]
fn __createRustObj__(
    state: &mut OpState,
    // params:
    #[string] name: String,
    #[string] jsvalue: String, // JS ObjectInfo
    #[number] parentId: i64,
) -> Result<i64, JsErrorBox> {
    assertBoth(state, "__createRustObj__")?;
    let jsvalue = serde_json::from_str(&jsvalue).map_err(jsError)?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;

    let parentId = objectIdFromI64(parentId);

    let objectBeingAdded = rustObjectBeingAddedOfJSObjectInfo(&jsvalue).map_err(jsError)?;

    // make sure player and other secure things cant be added
    match &objectBeingAdded {
        ObjectBeingAdded::Player(_) => {
            return Err(jsError("Player objects cannot be added directly"));
        }
        _ => {}
    }

    // a joint's rotation is in its restPose instead of its location
    let rotation_field = match &objectBeingAdded {
        ObjectBeingAdded::SkJoint(_) => "restPose",
        _ => "location",
    };
    let objectId = world.addObject(name, objectBeingAdded, parentId);
    // the rotation it was created with reads back as written, like one written afterwards
    let rotation_degrees = jsvalue
        .get(rotation_field)
        .and_then(|location| location.get("rotation"))
        .and_then(|rotation| rustVec3(rotation).ok());
    if let Some(rotation_degrees) = rotation_degrees
        && world.getObjectRotation(objectId).is_some()
    {
        world.writtenRotationDegreesOfObjectId.insert(objectId, rotation_degrees);
    }
    Ok(jsObjectId(objectId))
}

#[op2(fast)]
#[number]
fn __cloneRustObj__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    #[string] clone_name: String,
) -> Result<i64, JsErrorBox> {
    assertBoth(state, "__cloneRustObj__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    let cloned_object_id = world.addClonedObjectRecursive(object_id, clone_name).map_err(jsError)?;
    Ok(jsObjectId(cloned_object_id))
}

#[op2(fast)]
fn __cutRustObjToTemplate__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    #[string] template_name: String,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__cutRustObjToTemplate__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    if let WorldControlMode::Player(_) = world.worldControlMode {
        world.canDeleteOnClientOrErr(object_id).map_err(jsError)?;
    }
    world.cutObjectToTemplate(object_id, template_name).map_err(jsError)?;
    Ok(())
}

#[op2(fast)]
#[number]
fn __pasteRustObjFromTemplate__(
    state: &mut OpState,
    // params:
    #[string] template_name: String,
    #[number] parent_id: i64,
) -> Result<i64, JsErrorBox> {
    assertBoth(state, "__pasteRustObjFromTemplate__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let parent_id = objectIdFromI64(parent_id);
    let object_id = world.pasteTemplate(template_name.as_str(), parent_id).map_err(jsError)?;
    Ok(jsObjectId(object_id))
}

#[op2]
#[serde]
fn __createSkAttachmentsInRigidGroup__(
    state: &mut OpState,
    // params:
    #[number] skeleton_id: i64,
    #[number] rigid_group_id: i64,
) -> Result<Vec<i64>, JsErrorBox> {
    assertBoth(state, "__createSkAttachmentsInRigidGroup__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let skeleton_id = objectIdFromI64(skeleton_id);
    let rigid_group_id = objectIdFromI64(rigid_group_id);
    let sk_attachment_ids = world.createSkAttachmentsInRigidGroup(skeleton_id, rigid_group_id).map_err(jsError)?;
    Ok(sk_attachment_ids.into_iter().map(jsObjectId).collect())
}

#[op2]
#[serde]
fn __getRustChildrenIds__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
) -> Result<Vec<i64>, JsErrorBox> {
    assertBoth(state, "__getRustChildrenIds__")?;
    let obj_id = objectIdFromI64(object_id);
    let world = get_world_from_opstate(state).map_err(jsError)?;
    let children_ids = world.getDirectChildrenIds(obj_id).map_err(jsError)?;
    Ok(children_ids.iter().map(|id| jsObjectId(*id)).collect())
}

#[op2(fast)]
#[number]
fn __getRustLocalPlayerObjectId__(state: &mut OpState) -> Result<i64, JsErrorBox> {
    assertClient(state, "__getRustLocalPlayerObjectId__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    match world.worldControlMode {
        WorldControlMode::Player(player_object_id) => Ok(jsObjectId(player_object_id)),
        WorldControlMode::Editor | WorldControlMode::Server => Err(jsError("__getRustLocalPlayerObjectId__: only available on the client".to_string())),
    }
}

// setLocalPlayerMetadata(metadata): the JSON this client shares about its player with everyone else
#[op2(fast)]
fn __setLocalPlayerMetadata__(state: &mut OpState, #[string] metadata_json: String) -> Result<(), JsErrorBox> {
    assertClient(state, "__setLocalPlayerMetadata__")?;
    if metadata_json.len() > MAX_PLAYER_METADATA_BYTES {
        return Err(jsError(format!(
            "setLocalPlayerMetadata: the metadata is {} bytes, over the {MAX_PLAYER_METADATA_BYTES} byte limit",
            metadata_json.len()
        )));
    }
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    world.setLocalPlayerMetadataJson(metadata_json).map_err(jsError)
}

// getPlayerMetadata(playerObjectId): the JSON that player's client last shared, or null
#[op2]
#[serde]
fn __getPlayerMetadata__(state: &mut OpState, #[number] player_object_id: i64) -> Result<serde_json::Value, JsErrorBox> {
    assertBoth(state, "__getPlayerMetadata__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    Ok(match world.getPlayerMetadata(objectIdFromI64(player_object_id)) {
        Some(json) => serde_json::Value::String(json.to_string()),
        None => serde_json::Value::Null,
    })
}

// getPlayerCameraDirection(playerObjectId): the unit vector that player's camera looks along, or null
#[op2]
#[serde]
fn __getPlayerCameraDirection__(state: &mut OpState, #[number] player_object_id: i64) -> Result<serde_json::Value, JsErrorBox> {
    assertBoth(state, "__getPlayerCameraDirection__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    let Some(camera_orientation) = world.getPlayerCameraOrientation(objectIdFromI64(player_object_id)) else {
        return Ok(serde_json::Value::Null);
    };
    let direction = camera_orientation.to_unit_direction();
    Ok(with_short_floats(serde_json::json!({ "x": direction.x, "y": direction.y, "z": direction.z })))
}

// getPlayerPing(playerObjectId): that player's round trip to the server, or null. The server measures it on their connection, and a client has the server's latest update.
#[op2]
#[serde]
fn __getPlayerPing__(state: &mut OpState, #[number] player_object_id: i64) -> Result<Option<i64>, JsErrorBox> {
    assertBoth(state, "__getPlayerPing__")?;
    let player_object_id = objectIdFromI64(player_object_id);
    let ping_ms = match state.try_borrow::<Arc<MultiThreadHashMap<ObjectId, Connection>>>() {
        Some(connection_of_player_object_id_arc) => connection_of_player_object_id_arc
            .get(&player_object_id)
            .map(|connection| getPingMs(&connection)),
        None => get_player_ping_ms_from_opstate(state, player_object_id).map_err(jsError)?,
    };
    Ok(ping_ms.map(i64::from))
}

// getServerFnCaller(): the player whose call a 'use server' function is running for, or null
#[op2]
#[serde]
fn __getServerFnCaller__(state: &mut OpState) -> Result<Option<i64>, JsErrorBox> {
    assertServer(state, "__getServerFnCaller__")?;
    let server_fn_caller_object_id = get_server_fn_caller_object_id_from_opstate(state).map_err(jsError)?;
    Ok(server_fn_caller_object_id.map(jsObjectId))
}

// getPlayerMetadataAtTime(playerObjectId, timeMs): the metadata watchers had for that player at a recent time, or null
#[op2]
#[serde]
fn __getPlayerMetadataAtTime__(
    state: &mut OpState,
    // params:
    #[number] player_object_id: i64,
    time_ms: f64,
) -> Result<serde_json::Value, JsErrorBox> {
    assertServer(state, "__getPlayerMetadataAtTime__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    Ok(match world.getPlayerMetadataAtTime(objectIdFromI64(player_object_id), time_ms) {
        Some(json) => serde_json::Value::String(json.to_string()),
        None => serde_json::Value::Null,
    })
}

// getCharacterLocationAtTime(objectId, timeMs): where watchers drew a player's character at a recent time, or null
#[op2]
#[serde]
fn __getCharacterLocationAtTime__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    time_ms: f64,
) -> Result<serde_json::Value, JsErrorBox> {
    assertServer(state, "__getCharacterLocationAtTime__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    let Some(location) = world.getCharacterLocationAtTime(objectIdFromI64(object_id), time_ms) else {
        return Ok(serde_json::Value::Null);
    };
    let rotation = quat_to_euler_degrees(location.quaternion);
    Ok(with_short_floats(serde_json::json!({
        "position": { "x": location.position.x, "y": location.position.y, "z": location.position.z },
        "rotation": { "x": rotation.x, "y": rotation.y, "z": rotation.z },
    })))
}

// refreshPlayerProfile(playerObjectId) fetches with this op, then sets the player's preferredColor and equippedItemId in JS, because
// an async op finishes outside the time scripts run, when the world can't be touched. Guests have no profile.
#[op2]
#[serde]
async fn __fetchPlayerProfile__(
    state: Rc<RefCell<OpState>>, // we MUST ALWAYS drop a RefCell.borrow() before any awaits
    // params:
    #[number] player_object_id: i64,
) -> Result<PlayerProfile, JsErrorBox> {
    assertServer(&state.borrow(), "__fetchPlayerProfile__")?;
    // Which profile to fetch is read right away, so the player leaving during the fetch doesn't change the result.
    // Guests, and objects that aren't players, have none.
    let userid = match get_world_from_opstate(&state.borrow())
        .map_err(jsError)?
        .getObject(objectIdFromI64(player_object_id))
        .map(|object| &object.info)
    {
        Ok(ObjectInfo::Player(PlayerInfo { userid, .. })) => *userid,
        _ => None,
    };
    let Some(userid) = userid else {
        return Ok(PlayerProfile::GUEST);
    };
    get_player_profile(userid).await.map_err(jsError)
}

// addCatalogItemToWorld(catalogItemId, parentId) is these two ops. Only the fetch is async, because an async op finishes outside
// the time scripts run, when the world can't be touched. The fetched item goes through JS as a JSON string.
#[op2]
#[string]
async fn __fetchCatalogItemJson__(
    state: Rc<RefCell<OpState>>, // we MUST ALWAYS drop a RefCell.borrow() before any awaits
    // params:
    #[number] catalog_item_id: i64,
) -> Result<String, JsErrorBox> {
    assertServer(&state.borrow(), "__fetchCatalogItemJson__")?;
    let catalog_item = download_catalog_item(catalog_item_id).await.map_err(jsError)?;
    serde_json::to_string(&catalog_item).map_err(jsError)
}

// Adds the fetched catalog item the same way downloading it in the editor does, as a child of parentId.
// Returns its id, or null when parentId no longer exists, eg because the player it was for left during the fetch.
#[op2]
#[serde]
fn __addCatalogItemToWorld__(
    state: &mut OpState,
    // params:
    #[string] catalog_item_json: String,
    #[number] parent_id: i64,
) -> Result<Option<i64>, JsErrorBox> {
    assertServer(state, "__addCatalogItemToWorld__")?;
    let (name, dehydrated_gltf): (String, DehydratedGltf) = serde_json::from_str(&catalog_item_json).map_err(jsError)?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let parent_id = objectIdFromI64(parent_id);
    if !world.hasObject(parent_id) {
        return Ok(None);
    }
    let object_id = add_dehydrated_gltf_to_world(world, &name, dehydrated_gltf, parent_id).map_err(jsError)?;
    Ok(Some(jsObjectId(object_id)))
}

#[op2]
#[serde]
fn __getRustObjectIdsByName__(
    state: &mut OpState,
    // params:
    #[string] name: String,
) -> Result<Vec<i64>, JsErrorBox> {
    assertBoth(state, "__getRustObjectIdsByName__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    Ok(world.getObjectIdsByName(name.as_str()).into_iter().map(jsObjectId).collect())
}

#[op2]
#[serde]
fn __getRustObjectIdsByType__(
    state: &mut OpState,
    // params:
    #[string] object_type: String,
) -> Result<Vec<i64>, JsErrorBox> {
    assertBoth(state, "__getRustObjectIdsByType__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    let Some(object_kind) = ObjectKind::from_object_kind_name(object_type.as_str()) else {
        return Err(jsError(format!("__getRustObjectIdsByType__: unknown object type {object_type}")));
    };

    let Some(object_ids) = world._kindOfObjectId.get_keys(&object_kind) else {
        return Ok(Vec::new());
    };

    let object_ids = object_ids.iter().copied().collect::<Vec<_>>();
    Ok(object_ids.into_iter().map(jsObjectId).collect())
}

#[op2(fast)]
fn __scaleRustObjects__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    scale_multiplier: f64, // f64 is already #number
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__scaleRustObjects__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    let Some(center_point) = world.getPivotPoint(object_id) else {
        return Err(jsError(format!("scaleRustObjects: object {} was not a physical object.", object_id)));
    };

    world
        .setObjectScaleRecursive(object_id, scale_multiplier as f32, center_point, SetOrForce::Set)
        .map_err(jsError)?;
    Ok(())
}

// turn by degrees around a world-space axis, around the object's Pivot if it has one, otherwise its own position
#[op2(fast)]
fn __rotateRustObjects__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    #[string] axis: String,
    degrees: f64, // f64 is already #number
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__rotateRustObjects__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    let axis = rustVec3(&serde_json::from_str(&axis).map_err(jsError)?).map_err(jsError)?;
    let Some(unit_axis) = axis.try_normalize() else {
        return Err(jsError("rotateObjectBy: axis must have a nonzero length"));
    };
    let rotation_quaternion = Quat::from_axis_angle(unit_axis, (degrees as f32).to_radians());
    let Some(center_point) = world.getPivotPoint(object_id) else {
        return Err(jsError(format!("rotateRustObjects: object {} was not a physical object.", object_id)));
    };

    world
        .setObjectRotationRecursive(object_id, rotation_quaternion, center_point, SetOrForce::Set)
        .map_err(jsError)?;
    Ok(())
}

// rotate to a rotation, carrying the physical descendants by the same turn around the object's Pivot or its own position
#[op2(fast)]
fn __rotateRustObjectsTo__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    #[string] rotation: String,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__rotateRustObjectsTo__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    let rotation_degrees = rustVec3(&serde_json::from_str(&rotation).map_err(jsError)?).map_err(jsError)?;
    let Some(physics_info_3d) = world.getObjectPhysicsInfo3D(object_id) else {
        return Err(jsError(format!("rotateRustObjectsTo: object {} was not a physical object.", object_id)));
    };
    let rotation_quaternion = euler_degrees_to_quat(rotation_degrees) * physics_info_3d.location.quaternion.inverse();
    let Some(center_point) = world.getPivotPoint(object_id) else {
        return Err(jsError(format!("rotateRustObjectsTo: object {} was not a physical object.", object_id)));
    };

    world
        .setObjectRotationRecursive(object_id, rotation_quaternion, center_point, SetOrForce::Set)
        .map_err(jsError)?;
    world.writtenRotationDegreesOfObjectId.insert(object_id, rotation_degrees);
    Ok(())
}

// move by offset
#[op2(fast)]
fn __moveRustObjects__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    #[string] offset: String,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__moveRustObjects__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    let offset = rustVec3(&serde_json::from_str(&offset).map_err(jsError)?).map_err(jsError)?;

    world.setObjectPositionOffsetRecursive(object_id, offset, SetOrForce::Set).map_err(jsError)?;
    Ok(())
}

fn move_rust_objects_to(state: &mut OpState, op_name: &str, object_id: i64, position: String, set_or_force: SetOrForce) -> Result<(), JsErrorBox> {
    assertBoth(state, op_name)?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    let position = rustVec3(&serde_json::from_str(&position).map_err(jsError)?).map_err(jsError)?;
    let Some(physics_info_3d) = world.getObjectPhysicsInfo3D(object_id) else {
        return Err(jsError(format!("{op_name}: object {} was not a physical object.", object_id)));
    };
    let offset = position - physics_info_3d.location.position;

    world.setObjectPositionOffsetRecursive(object_id, offset, set_or_force).map_err(jsError)?;
    Ok(())
}

// move to position
#[op2(fast)]
fn __moveRustObjectsTo__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    #[string] position: String,
) -> Result<(), JsErrorBox> {
    move_rust_objects_to(state, "__moveRustObjectsTo__", object_id, position, SetOrForce::Set)
}

// teleport to position
#[op2(fast)]
fn __teleportRustObjectsTo__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    #[string] position: String,
) -> Result<(), JsErrorBox> {
    move_rust_objects_to(state, "__teleportRustObjectsTo__", object_id, position, SetOrForce::Force)
}

#[op2(fast)]
fn __setRustObjectsWalkthrough__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    is_walkthrough: bool,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__setRustObjectsWalkthrough__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);

    world.setObjectWalkthroughRecursive(object_id, is_walkthrough).map_err(jsError)?;
    Ok(())
}

#[op2(fast)]
fn __setRustObjectsPushable__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    is_pushable: bool,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__setRustObjectsPushable__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);

    world.setObjectPushableRecursive(object_id, is_pushable).map_err(jsError)?;
    Ok(())
}

#[op2(fast)]
fn __setRustObjectsDensity__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    density: f32,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__setRustObjectsDensity__")?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);

    world.setObjectDensityRecursive(object_id, density).map_err(jsError)?;
    Ok(())
}

#[op2(fast)]
fn __pauseClock__(
    state: &mut OpState,
    // params:
    #[number] clock_id: i64,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__pauseClock__")?;
    let (est_live_server_sim_ms, live_client_sim_ms) = get_live_sim_ms_from_opstate(state).map_err(jsError)?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let clock_id = objectIdFromI64(clock_id);

    let Some(clock_object) = world.objectOfId.get(&clock_id) else {
        return Err(jsError(format!("clock object {} was not found", clock_id)));
    };
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(jsError(format!("object {} is not a clock", clock_id)));
    };
    let new_clock_info = clock_info.withPausedClock(est_live_server_sim_ms, live_client_sim_ms).map_err(jsError)?;

    world.setClockInfo(clock_id, new_clock_info).map_err(jsError)?;
    Ok(())
}

#[op2(fast)]
fn __playClock__(
    state: &mut OpState,
    // params:
    #[number] clock_id: i64,
    animation_time_secs: f32,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__playClock__")?;
    let (est_live_server_sim_ms, live_client_sim_ms) = get_live_sim_ms_from_opstate(state).map_err(jsError)?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let clock_id = objectIdFromI64(clock_id);
    let time_location = get_time_location(&world.worldControlMode);

    let Some(clock_object) = world.objectOfId.get(&clock_id) else {
        return Err(jsError(format!("clock object {} was not found", clock_id)));
    };
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(jsError(format!("object {} is not a clock", clock_id)));
    };

    let new_clock_info = clock_info
        .withPlayingClock(animation_time_secs, time_location, est_live_server_sim_ms, live_client_sim_ms)
        .map_err(jsError)?;

    world.setClockInfo(clock_id, new_clock_info).map_err(jsError)?;
    Ok(())
}

#[op2(fast)]
fn __setClockSpeed__(
    state: &mut OpState,
    // params:
    #[number] clock_id: i64,
    speed: f32,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__setClockSpeed__")?;
    let (est_live_server_sim_ms, live_client_sim_ms) = get_live_sim_ms_from_opstate(state).map_err(jsError)?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let clock_id = objectIdFromI64(clock_id);

    let Some(clock_object) = world.objectOfId.get(&clock_id) else {
        return Err(jsError(format!("clock object {} was not found", clock_id)));
    };
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(jsError(format!("object {} is not a clock", clock_id)));
    };

    let new_info = clock_info.withNewSpeed(speed, est_live_server_sim_ms, live_client_sim_ms).map_err(jsError)?;
    world.setClockInfo(clock_id, new_info).map_err(jsError)?;
    Ok(())
}

#[op2]
fn __smoothlyChangeWeight__(
    state: &mut OpState,
    // params:
    #[number] animator_id: i64,
    target_weight: f32,
    duration_ms: f32,
    #[string] interpolation_style_str: Option<String>,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__smoothlyChangeWeight__")?;
    let (est_live_server_sim_ms, live_client_sim_ms) = get_live_sim_ms_from_opstate(state).map_err(jsError)?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let animator_id = objectIdFromI64(animator_id);
    let time_location = get_time_location(&world.worldControlMode);

    let interpolationStyle = interpolation_style_str
        .and_then(|interpolationStyle| InterpolationStyle::fromString(&interpolationStyle))
        .unwrap_or(InterpolationStyle::DEFAULT);

    let Some(animator_object) = world.objectOfId.get(&animator_id) else {
        return Err(jsError(format!("smoothlyChangeWeight: animator object {} was not found", animator_id)));
    };

    let (weight, weight_transition) = match &animator_object.info {
        ObjectInfo::SkAnimator(animator) => (animator.weight, animator.weightTransition),
        ObjectInfo::PropertyAnimator(animator) => (animator.weight, animator.weightTransition),
        _ => return Err(jsError(format!("smoothlyChangeWeight: object {} is not an animator", animator_id))),
    };

    let weight_transition = WeightTransition::fromCurrentWeight(
        time_location,
        est_live_server_sim_ms,
        live_client_sim_ms,
        weight,
        weight_transition,
        target_weight,
        duration_ms,
        interpolationStyle,
    )
    .map_err(jsError)?;

    match &animator_object.info {
        ObjectInfo::SkAnimator(animator_info) => {
            let mut animator_info = animator_info.clone();
            animator_info.weightTransition = Some(weight_transition);
            world.setSkAnimatorInfo(animator_id, animator_info).map_err(jsError)?;
        }
        ObjectInfo::PropertyAnimator(animator_info) => {
            let mut animator_info = animator_info.clone();
            animator_info.weightTransition = Some(weight_transition);
            world.setPropertyAnimatorInfo(animator_id, animator_info).map_err(jsError)?;
        }
        _ => return Err(jsError(format!("smoothlyChangeWeight: object {} is not an animator", animator_id))),
    };

    Ok(())
}

// Starts a tween on one of an object's numbers and returns the Tween object; see World::tweenTo. The script passes in the
// number's current value, read the same way a script reads any field.
#[op2]
#[number]
fn __tweenTo__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    #[string] property_path: String,
    current_value: f32,
    target_value: f32,
    duration_ms: f32,
    #[string] interpolation_style_str: Option<String>,
) -> Result<i64, JsErrorBox> {
    assertBoth(state, "__tweenTo__")?;
    let (est_live_server_sim_ms, live_client_sim_ms) = get_live_sim_ms_from_opstate(state).map_err(jsError)?;
    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    let property_path: Vec<String> = serde_json::from_str(&property_path).map_err(jsError)?;
    let time_location = get_time_location(&world.worldControlMode);

    let interpolationStyle = interpolation_style_str
        .and_then(|interpolationStyle| InterpolationStyle::fromString(&interpolationStyle))
        .unwrap_or(InterpolationStyle::DEFAULT);

    let tween_id = world
        .tweenTo(
            object_id,
            property_path,
            current_value,
            target_value,
            duration_ms,
            interpolationStyle,
            time_location,
            est_live_server_sim_ms,
            live_client_sim_ms,
        )
        .map_err(jsError)?;
    Ok(jsObjectId(tween_id))
}

#[op2(fast)]
fn __pushConsoleLogString__(
    state: &mut OpState,
    // params:
    #[string] console_log_string: String,
    #[string] console_log_level: String,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__pushConsoleLogString__")?;
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    script_manager_rc
        .borrow_mut()
        .push_console_log_string(console_log_string, console_log_level)
        .map_err(jsError)?;
    Ok(())
}

// The unit vector the local player's camera looks along. The client play loop refreshes the backing orientation every tick; it never exists on the server.
#[op2]
fn __getCameraDirection__<'scope>(state: &mut OpState, scope: &mut v8::PinScope<'scope, '_>) -> Result<v8::Local<'scope, v8::Value>, JsErrorBox> {
    assertClient(state, "__getCameraDirection__")?;
    let orientation = get_camera_orientation_from_opstate(state).map_err(jsError)?;
    let js_direction = jsVec3(orientation.to_unit_direction(), scope).map_err(jsError)?;
    Ok(js_direction.into())
}

#[op2(fast)]
fn __lockCameraDirection__(state: &mut OpState, #[string] js_direction: String) -> Result<(), JsErrorBox> {
    assertClient(state, "__lockCameraDirection__")?;
    let direction = rustVec3(&serde_json::from_str(&js_direction).map_err(jsError)?).map_err(jsError)?;
    let Some(unit_direction) = direction.try_normalize() else {
        return Err(jsError("lockCameraDirection: direction must have a nonzero length"));
    };
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    script_manager_rc.borrow_mut().camera_limits_mut().locked_orientation = Some(Orientation3D::from_unit_direction(unit_direction));
    Ok(())
}

#[op2(fast)]
fn __unlockCameraDirection__(state: &mut OpState) -> Result<(), JsErrorBox> {
    assertClient(state, "__unlockCameraDirection__")?;
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    script_manager_rc.borrow_mut().camera_limits_mut().locked_orientation = None;
    Ok(())
}

// The camera can't sit closer than MIN_CAMERA_ZOOM, or it would be inside the head it looks at.
#[op2(fast)]
fn __setZoomRange__(state: &mut OpState, min_zoom: f64, max_zoom: f64) -> Result<(), JsErrorBox> {
    assertClient(state, "__setZoomRange__")?;
    let (min_zoom, max_zoom) = (min_zoom as f32, max_zoom as f32);
    if !(min_zoom >= MIN_CAMERA_ZOOM && max_zoom.is_finite() && min_zoom <= max_zoom) {
        return Err(jsError(format!(
            "setZoomRange: needs {MIN_CAMERA_ZOOM} <= minZoom <= maxZoom, with finite numbers. Got {min_zoom} and {max_zoom}."
        )));
    }
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    let mut script_manager = script_manager_rc.borrow_mut();
    let camera_limits = script_manager.camera_limits_mut();
    camera_limits.min_zoom = min_zoom;
    camera_limits.max_zoom = max_zoom;
    Ok(())
}

#[op2(fast)]
fn __getZoom__(state: &mut OpState) -> Result<f64, JsErrorBox> {
    assertClient(state, "__getZoom__")?;
    let camera_zoom = get_camera_zoom_from_opstate(state).map_err(jsError)?;
    Ok(camera_zoom as f64)
}

#[op2(fast)]
fn __getDesiredZoom__(state: &mut OpState) -> Result<f64, JsErrorBox> {
    assertClient(state, "__getDesiredZoom__")?;
    let desired_camera_zoom = get_desired_camera_zoom_from_opstate(state).map_err(jsError)?;
    Ok(desired_camera_zoom as f64)
}

// null goes back to the default fov
#[op2]
fn __setFOV__(state: &mut OpState, fov_degrees: Option<f64>) -> Result<(), JsErrorBox> {
    assertClient(state, "__setFOV__")?;
    let fov_degrees = fov_degrees.map(|fov_degrees| fov_degrees as f32);
    if let Some(fov_degrees) = fov_degrees
        && !(MIN_CAMERA_FOV_DEGREES..=MAX_CAMERA_FOV_DEGREES).contains(&fov_degrees)
    {
        return Err(jsError(format!(
            "setFOV: needs {MIN_CAMERA_FOV_DEGREES} <= degrees <= {MAX_CAMERA_FOV_DEGREES}, or null. Got {fov_degrees}."
        )));
    }
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    script_manager_rc.borrow_mut().camera_limits_mut().fov_degrees = fov_degrees;
    Ok(())
}

#[op2(fast)]
fn __setCameraOffset__(state: &mut OpState, #[string] js_offset: String) -> Result<(), JsErrorBox> {
    assertClient(state, "__setCameraOffset__")?;
    let offset = rustVec3(&serde_json::from_str(&js_offset).map_err(jsError)?).map_err(jsError)?;
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    script_manager_rc.borrow_mut().camera_limits_mut().offset = offset;
    Ok(())
}

#[op2(fast)]
fn __setIsMouseLocked__(state: &mut OpState, is_mouse_locked: bool) -> Result<(), JsErrorBox> {
    assertClient(state, "__setIsMouseLocked__")?;
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    script_manager_rc.borrow_mut().camera_limits_mut().is_mouse_locked = is_mouse_locked;
    Ok(())
}

// A picture can't be bigger than MAX_MOUSE_CURSOR_SIZE points, and the point that clicks has to be on it.
#[op2(fast)]
fn __setMouseCursor__(state: &mut OpState, #[string] js_mouse_cursor: String) -> Result<(), JsErrorBox> {
    assertClient(state, "__setMouseCursor__")?;
    let mouse_cursor = serde_json::from_str::<MouseCursor>(&js_mouse_cursor).map_err(|error| jsError(format!("setMouseCursor: {error}")))?;
    if let MouseCursor::Image(MouseCursorImage {
        width,
        height,
        hotspotX,
        hotspotY,
        ..
    }) = mouse_cursor
    {
        let is_size_allowed = |size: f32| size > 0.0 && size <= MAX_MOUSE_CURSOR_SIZE;
        let is_on_picture = |fraction: f32| (0.0..=1.0).contains(&fraction);
        if !(is_size_allowed(width) && is_size_allowed(height) && is_on_picture(hotspotX) && is_on_picture(hotspotY)) {
            return Err(jsError(format!(
                "setMouseCursor: needs 0 < width, height <= {MAX_MOUSE_CURSOR_SIZE} and 0 <= hotspotX, hotspotY <= 1. Got width {width}, height {height}, hotspotX {hotspotX} and hotspotY {hotspotY}."
            )));
        }
    }
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    *script_manager_rc.borrow_mut().mouse_cursor_mut() = mouse_cursor;
    Ok(())
}

// This uses the camera it just drew with
#[op2]
#[serde]
fn __getMouseRay__(state: &mut OpState) -> Result<serde_json::Value, JsErrorBox> {
    assertClient(state, "__getMouseRay__")?;
    let Some((ray_origin, unit_ray_direction)) = get_mouse_ray_from_opstate(state).map_err(jsError)? else {
        return Ok(serde_json::Value::Null);
    };
    Ok(with_short_floats(serde_json::json!({
        "origin": { "x": ray_origin.x, "y": ray_origin.y, "z": ray_origin.z },
        "direction": { "x": unit_ray_direction.x, "y": unit_ray_direction.y, "z": unit_ray_direction.z },
    })))
}

// castRay, getGroundSupport and castObjectShape all return this. The script side turns the objectId into a live object.
fn castHitToJson(cast_hit: Option<CastHit>) -> serde_json::Value {
    let Some(CastHit {
        object_id,
        normal,
        distance,
        position,
    }) = cast_hit
    else {
        return serde_json::Value::Null;
    };
    with_short_floats(serde_json::json!({
        "objectId": object_id,
        "distance": distance,
        "position": { "x": position.x, "y": position.y, "z": position.z },
        "normal": { "x": normal.x, "y": normal.y, "z": normal.z },
    }))
}

// objectService.castRay. Excluded object ids also exclude their rigid-group subitems (the physics filter checks the collider's group parent).
#[op2]
#[serde]
fn __castRay__(
    state: &mut OpState,
    // params:
    #[string] js_ray_origin: String,
    #[string] js_ray_direction: String,
    max_distance: f64,
    include_walkthrough: bool,
    #[string] js_exclude_object_ids: String,
) -> Result<serde_json::Value, JsErrorBox> {
    assertBoth(state, "__castRay__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;

    let ray_origin = rustVec3(&serde_json::from_str(&js_ray_origin).map_err(jsError)?).map_err(jsError)?;
    let ray_direction = rustVec3(&serde_json::from_str(&js_ray_direction).map_err(jsError)?).map_err(jsError)?;
    let Some(unit_ray_direction) = ray_direction.try_normalize() else {
        return Err(jsError("castRay: rayDirection must have a nonzero length"));
    };
    let exclude_object_ids = excludeObjectIdsOfJs(&js_exclude_object_ids)?;
    let shouldExcludeHitObject = |object_id: ObjectId| exclude_object_ids.contains(&object_id);

    // the direction is normalized, so the time of impact is the distance
    let hit = world.physics.castRay(
        ray_origin,
        unit_ray_direction,
        max_distance as f32,
        include_walkthrough,
        &shouldExcludeHitObject,
    );
    Ok(castHitToJson(hit))
}

// castRay's and castObjectShape's excludeObjectIds
fn excludeObjectIdsOfJs(js_exclude_object_ids: &str) -> Result<std::collections::HashSet<ObjectId>, JsErrorBox> {
    let js_exclude_object_ids: Vec<i64> = serde_json::from_str(js_exclude_object_ids).map_err(jsError)?;
    Ok(js_exclude_object_ids.into_iter().map(objectIdFromI64).collect())
}

// getGroundSupport's and getVelocityRelativeToGround's options, as (max distance, max ground slope degrees).
fn groundSupportOptionsOfJs(js_options: &str) -> Result<(f32, f32), JsErrorBox> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GroundSupportOptions {
        max_distance: Option<f32>,
        max_ground_slope_degrees: Option<f32>,
    }
    let options: GroundSupportOptions = serde_json::from_str(js_options).map_err(jsError)?;
    Ok((
        options.max_distance.unwrap_or(DEFAULT_MAX_DISTANCE_TO_GROUND),
        options.max_ground_slope_degrees.unwrap_or(DEFAULT_MAX_GROUND_SLOPE_DEGREES),
    ))
}

// getGroundSupport(objectId, options): a live walkable contact, or else the first walkable ground the object's shape would hit
// within maxDistance below it.
#[op2]
#[serde]
fn __getGroundSupport__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    #[string] js_options: String,
) -> Result<serde_json::Value, JsErrorBox> {
    assertBoth(state, "__getGroundSupport__")?;
    let (max_distance, max_ground_slope_degrees) = groundSupportOptionsOfJs(&js_options)?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    if world.getObjectPhysicsInfo3D(object_id).is_none() {
        return Err(jsError(format!("getGroundSupport: object {} was not a physical object", object_id)));
    }
    let ground_support_hit = world.getGroundSupport(object_id, max_distance, max_ground_slope_degrees);
    Ok(castHitToJson(ground_support_hit))
}

// castObjectShape(objectId, direction, options): slides the object's shape and returns the first solid hit, never the object itself.
#[op2]
#[serde]
fn __castObjectShape__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
    #[string] js_start_offset: String,
    #[string] js_direction: String,
    max_distance: f64,
    include_walkthrough: bool,
    #[string] js_exclude_object_ids: String,
) -> Result<serde_json::Value, JsErrorBox> {
    assertBoth(state, "__castObjectShape__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    let start_offset = rustVec3(&serde_json::from_str(&js_start_offset).map_err(jsError)?).map_err(jsError)?;
    let direction = rustVec3(&serde_json::from_str(&js_direction).map_err(jsError)?).map_err(jsError)?;
    if direction.try_normalize().is_none() {
        return Err(jsError("castObjectShape: direction must have a nonzero length"));
    }
    let exclude_object_ids = excludeObjectIdsOfJs(&js_exclude_object_ids)?;
    let shouldExcludeHitObject = |object_id: ObjectId| exclude_object_ids.contains(&object_id);
    if world.getObjectPhysicsInfo3D(object_id).is_none() {
        return Err(jsError(format!("castObjectShape: object {} was not a physical object", object_id)));
    }
    let cast_hit = world.physics.castObjectShapeInDirection(
        object_id,
        start_offset,
        direction,
        max_distance as f32,
        include_walkthrough,
        &shouldExcludeHitObject,
    );
    Ok(castHitToJson(cast_hit))
}

#[op2]
fn __getGravity__<'scope>(state: &mut OpState, scope: &mut v8::PinScope<'scope, '_>) -> Result<v8::Local<'scope, v8::Value>, JsErrorBox> {
    assertBoth(state, "__getGravity__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    Ok(jsVec3(world.physics.getGravity(), scope).map_err(jsError)?.into())
}

// -------------------------- clientCharacterService ---------------

// Applies what a clientCharacterService step callback returned to the local player's character.
#[op2(fast)]
fn __applyCharacterChanges__(
    state: &mut OpState,
    // params:
    #[string] character_changes: String,
) -> Result<(), JsErrorBox> {
    assertClient(state, "__applyCharacterChanges__")?;
    let character_changes: serde_json::Value = serde_json::from_str(&character_changes).map_err(jsError)?;
    let Some(character_changes) = character_changes.as_object() else {
        return Err(jsError("applyCharacterChanges: a step callback must return an object of changes"));
    };
    let velocity_change = character_changes.get("velocityChange").map(rustVec3).transpose().map_err(jsError)?;
    let angular_velocity_change = character_changes
        .get("angularVelocityChange")
        .map(rustVec3RadiansFromDegrees)
        .transpose()
        .map_err(jsError)?;
    let velocity = character_changes.get("velocity").map(rustVec3).transpose().map_err(jsError)?;
    let position = character_changes.get("position").map(rustVec3).transpose().map_err(jsError)?;

    let world = get_world_mut_from_opstate(state).map_err(jsError)?;
    let WorldControlMode::Player(player_object_id) = world.worldControlMode else {
        return Err(jsError("applyCharacterChanges: only available on the client"));
    };
    let Some(character_object_id) = world.getPlayerCharacterObject(player_object_id) else {
        return Err(jsError("applyCharacterChanges: the local player has no character"));
    };
    let movement = world
        .getObjectPhysicsInfo3D(character_object_id)
        .ok_or_else(|| jsError("applyCharacterChanges: the character is not a physical object"))?
        .movement;

    // a new position is a smooth move like any other, so a warp calls teleportObjectTo instead
    if let Some(position) = position {
        world.setPhysicsInfo3D(
            character_object_id,
            SetPhysicsInfo3D {
                position: Some(position),
                ..Default::default()
            },
            false,
        );
    }

    let new_velocity = match (velocity, velocity_change) {
        (None, None) => None,
        (velocity, velocity_change) => Some(velocity.unwrap_or(movement.velocity) + velocity_change.unwrap_or(Vec3::ZERO)),
    };
    world.setPhysicsInfo3D(
        character_object_id,
        SetPhysicsInfo3D {
            velocity: new_velocity,
            angularVelocity: angular_velocity_change.map(|angular_velocity_change| movement.angularVelocity + angular_velocity_change),
            ..Default::default()
        },
        false,
    );
    Ok(())
}

// getVelocityRelativeToGround(objectId, options): compares the object and support at the object's center of mass so animation can ignore motion carried by the ground.
#[op2]
fn __getVelocityRelativeToGround__<'scope>(
    state: &mut OpState,
    scope: &mut v8::PinScope<'scope, '_>,
    // params:
    #[number] object_id: i64,
    #[string] js_options: String,
) -> Result<v8::Local<'scope, v8::Value>, JsErrorBox> {
    assertBoth(state, "__getVelocityRelativeToGround__")?;
    let (max_distance, max_ground_slope_degrees) = groundSupportOptionsOfJs(&js_options)?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    let velocity = world
        .getVelocityRelativeToGround(object_id, max_distance, max_ground_slope_degrees)
        .ok_or_else(|| jsError(format!("getVelocityRelativeToGround: object {} was not a physical object", object_id)))?;
    Ok(jsVec3(velocity, scope).map_err(jsError)?.into())
}

// getObjectAxes(objectId): the directions the object's own x, y and z axes point in world coordinates, each 1 long.
#[op2]
#[serde]
fn __getObjectAxes__(
    state: &mut OpState,
    // params:
    #[number] object_id: i64,
) -> Result<serde_json::Value, JsErrorBox> {
    assertBoth(state, "__getObjectAxes__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    let object_id = objectIdFromI64(object_id);
    let Some(physics_info_3d) = world.getObjectPhysicsInfo3D(object_id) else {
        return Err(jsError(format!("getObjectAxes: object {} was not a physical object", object_id)));
    };
    let Mat3 { x_axis, y_axis, z_axis } = Mat3::from_quat(physics_info_3d.location.quaternion);
    Ok(with_short_floats(serde_json::json!({
        "x": { "x": x_axis.x, "y": x_axis.y, "z": x_axis.z },
        "y": { "x": y_axis.x, "y": y_axis.y, "z": y_axis.z },
        "z": { "x": z_axis.x, "y": z_axis.y, "z": z_axis.z },
    })))
}

// getTime(). Returns the server sim ms set by beforeRunJS: the tick's simulation time on the server, the synced estimate on clients.
#[op2(fast)]
fn __getTimeMs__(state: &mut OpState) -> Result<f64, JsErrorBox> {
    assertBoth(state, "__getTimeMs__")?;
    let (est_live_server_sim_ms, _live_client_sim_ms) = get_live_sim_ms_from_opstate(state).map_err(jsError)?;
    Ok(est_live_server_sim_ms)
}

// getDelayedWorldTime(). getTime() minus this client's world delay: the time everything from the server is at in its world.
#[op2(fast)]
fn __getDelayedWorldTimeMs__(state: &mut OpState) -> Result<f64, JsErrorBox> {
    assertClient(state, "__getDelayedWorldTimeMs__")?;
    let (est_live_server_sim_ms, _live_client_sim_ms) = get_live_sim_ms_from_opstate(state).map_err(jsError)?;
    let client_world_delay_ms = get_client_world_delay_ms_from_opstate(state).map_err(jsError)?;
    let est_dely_server_sim_ms = est_live_server_sim_ms - client_world_delay_ms as f64;
    Ok(est_dely_server_sim_ms)
}

#[op2]
async fn __tickInternal__(
    state: Rc<RefCell<OpState>>, // we MUST ALWAYS drop a RefCell.borrow() before any awaits
    // params:
    #[number] script_id: i64,
) -> Result<(), JsErrorBox> {
    assertBoth(&state.borrow(), "__tickInternal__")?;
    let script_id = objectIdFromI64(script_id);
    let (next_tick_tx, next_tick_rx) = oneshot::channel();

    // { }'s to drop rc before await
    {
        let script_manager_rc = get_script_manager_mut_from_opstate(&mut state.borrow_mut()).map_err(jsError)?.clone();
        let mut script_manager = script_manager_rc.borrow_mut();
        // only a live script gets registered. a dead one leaves next_tick_tx to drop here, which fails the
        // await below.
        if script_manager.is_script_live(&script_id) {
            script_manager.set_as_awaiting_next_tick(&script_id, next_tick_tx);
        }
    }

    // no tick is ever coming
    if next_tick_rx.await.is_err() {
        std::future::pending::<()>().await
    }
    Ok(())
}

#[op2]
async fn __stopScriptInternal__(
    state: Rc<RefCell<OpState>>, // we MUST ALWAYS drop a RefCell.borrow() before any awaits
                                 // (no params)
) -> Result<(), JsErrorBox> {
    assertBoth(&state.borrow(), "__stopScriptInternal__")?;

    // wait forever
    std::future::pending::<Result<(), JsErrorBox>>().await
}

// Every script's module url ends in ?id=<its object id>, and V8 knows which module each function was written in, so this
// says whether the script a function came from is still in the world. Functions from anywhere else count as live.
#[op2(fast)]
fn __isFnFromLiveScript__<'scope>(
    state: &mut OpState,
    scope: &mut v8::PinScope<'scope, '_>,
    // params:
    js_fn: v8::Local<'scope, v8::Function>,
) -> Result<bool, JsErrorBox> {
    assertBoth(state, "__isFnFromLiveScript__")?;
    let world = get_world_from_opstate(state).map_err(jsError)?;
    let Some(module_url) = js_fn.get_script_origin(scope).resource_name() else {
        return Ok(true);
    };
    let module_url = module_url.to_rust_string_lossy(scope);
    let Some(script_object_id) = module_url
        .rsplit_once("?id=")
        .and_then(|(_, script_object_id)| script_object_id.parse::<ObjectId>().ok())
    else {
        return Ok(true);
    };
    Ok(world.hasObject(script_object_id))
}

#[op2(fast)]
fn __registerJsServerFn__<'scope>(
    state: &mut OpState,
    scope: &mut v8::PinScope<'scope, '_>,
    // params:
    #[string] fn_identifier: String,
    server_js_fn: v8::Local<'scope, v8::Function>,
) -> Result<(), JsErrorBox> {
    assertServer(state, "__registerJsServerFn__")?;
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    script_manager_rc
        .borrow_mut()
        .register_server_js_fn(fn_identifier, v8::Global::new(scope, server_js_fn));
    Ok(())
}

#[op2(fast)]
fn __registerJsClientFn__<'scope>(
    state: &mut OpState,
    scope: &mut v8::PinScope<'scope, '_>,
    // params:
    #[string] fn_identifier: String,
    client_js_fn: v8::Local<'scope, v8::Function>,
) -> Result<(), JsErrorBox> {
    assertClient(state, "__registerJsClientFn__")?;
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    script_manager_rc
        .borrow_mut()
        .register_client_js_fn(fn_identifier, v8::Global::new(scope, client_js_fn));
    Ok(())
}

#[op2(fast)]
fn __registerJsCallbackFn__<'scope>(
    state: &mut OpState,
    scope: &mut v8::PinScope<'scope, '_>,
    // params:
    js_callback_fn: v8::Local<'scope, v8::Function>,
) -> Result<(), JsErrorBox> {
    assertBoth(state, "__registerJsCallbackFn__")?;
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    script_manager_rc.borrow_mut().register_js_callback_fn(v8::Global::new(scope, js_callback_fn));
    Ok(())
}

#[op2(fast)]
fn __setRustGuiRoot__(
    state: &mut OpState,
    // params:
    #[number] script_object_id: i64,
    #[string] gui_key: String,
    #[string] gui_root: String,
) -> Result<(), JsErrorBox> {
    assertClient(state, "__setRustGuiRoot__")?;
    let gui_root: GuiRenderedRoot = serde_json::from_str(&gui_root).map_err(jsError)?;
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    let script_object_id = objectIdFromI64(script_object_id);
    script_manager_rc.borrow_mut().set_gui_root(script_object_id, gui_key, gui_root);
    Ok(())
}

#[op2(fast)]
fn __clearRustGuiRoot__(
    state: &mut OpState,
    // params:
    #[number] script_object_id: i64,
    #[string] gui_key: String,
) -> Result<(), JsErrorBox> {
    assertClient(state, "__clearRustGuiRoot__")?;
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    let script_object_id = objectIdFromI64(script_object_id);
    script_manager_rc.borrow_mut().clear_gui_root(&script_object_id, gui_key.as_str());
    Ok(())
}

#[op2(fast)]
fn __registerRustGuiHandler__<'scope>(
    state: &mut OpState,
    scope: &mut v8::PinScope<'scope, '_>,
    // params:
    #[number] script_object_id: i64,
    #[string] gui_key: String,
    #[string] handler_id: String,
    handler: v8::Local<'scope, v8::Function>,
) -> Result<(), JsErrorBox> {
    assertClient(state, "__registerRustGuiHandler__")?;
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    let script_object_id = objectIdFromI64(script_object_id);
    script_manager_rc
        .borrow_mut()
        .register_gui_handler(script_object_id, gui_key, handler_id, v8::Global::new(scope, handler));
    Ok(())
}

#[op2(fast)]
fn __clearRustGuiHandlers__(
    state: &mut OpState,
    // params:
    #[number] script_object_id: i64,
    #[string] gui_key: String,
) -> Result<(), JsErrorBox> {
    assertClient(state, "__clearRustGuiHandlers__")?;
    let script_manager_rc = get_script_manager_mut_from_opstate(state).map_err(jsError)?;
    let script_object_id = objectIdFromI64(script_object_id);
    script_manager_rc.borrow_mut().clear_gui_handlers(&script_object_id, gui_key.as_str());
    Ok(())
}

#[op2]
#[string]
// Returning Err(JsErrorBox) here rejects the JS Promise instead of throwing synchronously, so .catch() still works.
async fn __callJsServerFnFromClient__(
    state: Rc<RefCell<OpState>>, // we MUST ALWAYS drop a RefCell.borrow() before any awaits
    // params:
    #[string] fn_identifier: String,
    #[string] js_args: String,
) -> Result<String, JsErrorBox> {
    assertClient(&state.borrow(), "__callJsServerFnFromClient__")?;

    // get connnection
    let connection = state
        .borrow()
        .try_borrow::<Connection>()
        .ok_or_else(|| jsError("Server function caller is not configured"))?
        .clone();

    // open new bidi stream that we will send to then wait for response for
    // wait for response
    let server_function_result: ServerFunctionResultMessage = sendTcpRequest(
        &connection,
        &ClientMessageTCP::ServerFnCall(ServerFnCallMessage { fn_id: fn_identifier, js_args }),
    )
    .await
    .map_err(|error| jsError(format!("Server function request failed: {}", error)))?;

    match server_function_result.result {
        Ok(js_result) => Ok(js_result),
        Err(error) => Err(jsError(error)),
    }
}

#[op2]
#[string]
// Returning Err(JsErrorBox) here rejects the JS Promise instead of throwing synchronously, so .catch() still works.
async fn __callJsClientFnFromServer__(
    state: Rc<RefCell<OpState>>, // we MUST ALWAYS drop a RefCell.borrow() before any awaits
    // params:
    #[number] player_object_id: i64,
    #[string] fn_identifier: String,
    #[string] js_args: String,
) -> Result<String, JsErrorBox> {
    assertServer(&state.borrow(), "__callJsClientFnFromServer__")?;

    let connection_of_player_object_id_arc = state
        .borrow()
        .try_borrow::<Arc<MultiThreadHashMap<ObjectId, Connection>>>()
        .ok_or_else(|| jsError("Client function caller is not configured"))?
        .clone();

    let connection_arc = {
        let Some(connection_arc) = connection_of_player_object_id_arc.get(&objectIdFromI64(player_object_id)) else {
            return Err(jsError(format!("Client connection not found for player {}", player_object_id)));
        };
        connection_arc
    };

    let (mut send_stream, mut recv_stream) = connection_arc
        .open_bi()
        .await
        .map_err(|error| jsError(format!("Could not open client function stream: {}", error)))?;
    let send_result = sendTcpMessage(&mut send_stream, &ClientFunctionCallMessage { fn_id: fn_identifier, js_args }).await;
    if let Err(error) = send_result {
        return Err(jsError(format!("Could not send client function call: {}", error)));
    }

    let client_function_result = awaitTcpMessageMaxBytes::<ClientFunctionResultMessage>(&mut recv_stream, MAX_TCP_MESSAGE_BYTES)
        .await
        .map_err(|error| jsError(format!("Could not receive client function result: {}", error)))?;

    match client_function_result.result {
        Ok(js_result) => Ok(js_result),
        Err(error) => Err(jsError(error)),
    }
}

// Scripts read the clock in steps of 2 ms, like a browser's reduced time precision, so they can't use a precise clock to
// read this process's memory through CPU timing. deno_web's clock is exact, so this replaces its op_now.
#[op2(fast)]
fn __getTimeSinceStartInSteps__(state: &mut OpState, #[buffer] time: &mut [u8]) {
    const CLOCK_STEP_NANOSECONDS: u32 = 2_000_000;
    // Scripts can reach this op with any buffer, and a panic here would stop the whole process.
    if time.len() < 8 {
        return;
    }
    let elapsed = state.borrow::<StartTime>().elapsed();
    let nanoseconds = elapsed.subsec_nanos();
    time[0..4].copy_from_slice(&(elapsed.as_secs() as u32).to_ne_bytes());
    time[4..8].copy_from_slice(&(nanoseconds - nanoseconds % CLOCK_STEP_NANOSECONDS).to_ne_bytes());
}

// ----------- Entrypoint -----------

deno_core::extension!(
    deno_web_required_imports_extension,
    esm_entry_point = "ext:aaa/bootstrap.js",
    esm = ["ext:aaa/bootstrap.js" = {
        source = r#"
const { loadExtScript } = Deno.core;
const consoleModule = loadExtScript("ext:deno_web/01_console.js");
const urlModule = loadExtScript("ext:deno_web/00_url.js");
const urlPatternModule = loadExtScript("ext:deno_web/01_urlpattern.js");
const domExceptionModule = loadExtScript("ext:deno_web/01_dom_exception.js");
const eventModule = loadExtScript("ext:deno_web/02_event.js");
const timersModule = loadExtScript("ext:deno_web/02_timers.js");
const abortModule = loadExtScript("ext:deno_web/03_abort_signal.js");
const globalInterfacesModule = loadExtScript("ext:deno_web/04_global_interfaces.js");
const base64Module = loadExtScript("ext:deno_web/05_base64.js");
const streamsModule = loadExtScript("ext:deno_web/06_streams.js");
const textEncodingModule = loadExtScript("ext:deno_web/08_text_encoding.js");
const fileModule = loadExtScript("ext:deno_web/09_file.js");
const fileReaderModule = loadExtScript("ext:deno_web/10_filereader.js");
const messagePortModule = loadExtScript("ext:deno_web/13_message_port.js");
const compressionModule = loadExtScript("ext:deno_web/14_compression.js");
const performanceModule = loadExtScript("ext:deno_web/15_performance.js");
const imageDataModule = loadExtScript("ext:deno_web/16_image_data.js");

const writable = (value) => ({
  value,
  configurable: true,
  enumerable: false,
  writable: true,
});

// This installs the standard web globals that our scripts can rely on.
Object.defineProperties(globalThis, {
  console: writable(new consoleModule.Console((msg, level) => {
    let consoleLogLevel = "log";
    switch (level) {
      case 0:
        consoleLogLevel = "debug";
        break;
      case 1:
        consoleLogLevel = "info";
        break;
      case 2:
        consoleLogLevel = "warn";
        break;
      case 3:
        consoleLogLevel = "error";
        break;
    }
    Deno.core.ops.__pushConsoleLogString__(msg, consoleLogLevel);
  })),
  URL: writable(urlModule.URL),
  URLPattern: writable(urlPatternModule.URLPattern),
  URLSearchParams: writable(urlModule.URLSearchParams),
  DOMException: writable(domExceptionModule.DOMException),
  Event: writable(eventModule.Event),
  EventTarget: writable(eventModule.EventTarget),
  CloseEvent: writable(eventModule.CloseEvent),
  CustomEvent: writable(eventModule.CustomEvent),
  ErrorEvent: writable(eventModule.ErrorEvent),
  MessageEvent: writable(eventModule.MessageEvent),
  ProgressEvent: writable(eventModule.ProgressEvent),
  PromiseRejectionEvent: writable(eventModule.PromiseRejectionEvent),
  reportError: writable(eventModule.reportError),
  setTimeout: writable(timersModule.setTimeout),
  clearTimeout: writable(timersModule.clearTimeout),
  setInterval: writable(timersModule.setInterval),
  clearInterval: writable(timersModule.clearInterval),
  AbortController: writable(abortModule.AbortController),
  AbortSignal: writable(abortModule.AbortSignal),
  Window: writable(globalInterfacesModule.Window),
  WorkerGlobalScope: writable(globalInterfacesModule.WorkerGlobalScope),
  DedicatedWorkerGlobalScope: writable(globalInterfacesModule.DedicatedWorkerGlobalScope),
  atob: writable(base64Module.atob),
  btoa: writable(base64Module.btoa),
  ByteLengthQueuingStrategy: writable(streamsModule.ByteLengthQueuingStrategy),
  CountQueuingStrategy: writable(streamsModule.CountQueuingStrategy),
  ReadableByteStreamController: writable(streamsModule.ReadableByteStreamController),
  ReadableStream: writable(streamsModule.ReadableStream),
  ReadableStreamBYOBReader: writable(streamsModule.ReadableStreamBYOBReader),
  ReadableStreamBYOBRequest: writable(streamsModule.ReadableStreamBYOBRequest),
  ReadableStreamDefaultController: writable(streamsModule.ReadableStreamDefaultController),
  ReadableStreamDefaultReader: writable(streamsModule.ReadableStreamDefaultReader),
  TransformStream: writable(streamsModule.TransformStream),
  TransformStreamDefaultController: writable(streamsModule.TransformStreamDefaultController),
  WritableStream: writable(streamsModule.WritableStream),
  WritableStreamDefaultController: writable(streamsModule.WritableStreamDefaultController),
  WritableStreamDefaultWriter: writable(streamsModule.WritableStreamDefaultWriter),
  TextDecoder: writable(textEncodingModule.TextDecoder),
  TextDecoderStream: writable(textEncodingModule.TextDecoderStream),
  TextEncoder: writable(textEncodingModule.TextEncoder),
  TextEncoderStream: writable(textEncodingModule.TextEncoderStream),
  Blob: writable(fileModule.Blob),
  File: writable(fileModule.File),
  FileReader: writable(fileReaderModule.FileReader),
  MessageChannel: writable(messagePortModule.MessageChannel),
  MessagePort: writable(messagePortModule.MessagePort),
  structuredClone: writable(messagePortModule.structuredClone),
  CompressionStream: writable(compressionModule.CompressionStream),
  DecompressionStream: writable(compressionModule.DecompressionStream),
  Performance: writable(performanceModule.Performance),
  PerformanceEntry: writable(performanceModule.PerformanceEntry),
  PerformanceMark: writable(performanceModule.PerformanceMark),
  PerformanceMeasure: writable(performanceModule.PerformanceMeasure),
  performance: writable(performanceModule.performance),
  ImageData: writable(imageDataModule.ImageData),
});
"#
    }],
);

deno_core::extension!(
    our_custom_js_ops_extension,
    ops = [
        __getRustRootObjectId__,
        __getRustLocalPlayerObjectId__,
        __setLocalPlayerMetadata__,
        __getPlayerMetadata__,
        __getPlayerCameraDirection__,
        __getPlayerPing__,
        __getServerFnCaller__,
        __getPlayerMetadataAtTime__,
        __getCharacterLocationAtTime__,
        __fetchPlayerProfile__,
        __fetchCatalogItemJson__,
        __addCatalogItemToWorld__,
        __getWorldChangeCount__, // INTERNAL
        __getRustObject__,
        __setRustObjectField__,
        __deleteRustObj__,
        __createRustObj__,
        __cloneRustObj__,
        __cutRustObjToTemplate__,
        __pasteRustObjFromTemplate__,
        __createSkAttachmentsInRigidGroup__,
        __getRustChildrenIds__,
        __moveRustObjects__,
        __moveRustObjectsTo__,
        __teleportRustObjectsTo__,
        __scaleRustObjects__,
        __rotateRustObjects__,
        __rotateRustObjectsTo__,
        __setRustObjectsWalkthrough__,
        __setRustObjectsPushable__,
        __setRustObjectsDensity__,
        __pauseClock__,
        __playClock__,
        __setClockSpeed__,
        __smoothlyChangeWeight__,
        __tweenTo__,
        __getRustObjectIdsByName__,
        __getRustObjectIdsByType__,
        __pushConsoleLogString__,
        __getTimeMs__,
        __getDelayedWorldTimeMs__,
        __getCameraDirection__,
        __lockCameraDirection__,
        __unlockCameraDirection__,
        __setZoomRange__,
        __getZoom__,
        __getDesiredZoom__,
        __setFOV__,
        __setCameraOffset__,
        __setIsMouseLocked__,
        __setMouseCursor__,
        __getMouseRay__,
        __castRay__,
        __getGroundSupport__,
        __castObjectShape__,
        __getGravity__,
        __applyCharacterChanges__, // INTERNAL
        __getVelocityRelativeToGround__,
        __getObjectAxes__,
        __tickInternal__,
        __stopScriptInternal__,
        __isFnFromLiveScript__,   // INTERNAL
        __registerJsServerFn__,   // INTERNAL
        __registerJsClientFn__,   // INTERNAL
        __registerJsCallbackFn__, // INTERNAL
        __setRustGuiRoot__,
        __clearRustGuiRoot__,
        __clearRustGuiHandlers__,
        __registerRustGuiHandler__,
        __callJsServerFnFromClient__, // INTERNAL
        __callJsClientFnFromServer__, // INTERNAL
    ],
    middleware = |op| match op.name {
        "op_now" => op.with_implementation_from(&__getTimeSinceStartInSteps__()),
        _ => op,
    },
);

pub fn denoWebRequiredImportsExtension() -> Extension {
    deno_web_required_imports_extension::init()
}

pub fn ourCustomJsOpsExtension() -> Extension {
    our_custom_js_ops_extension::init()
}
