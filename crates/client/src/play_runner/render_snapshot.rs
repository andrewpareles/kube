use crate::play_web_view::PlayersListEntry;
use game_types::object_types::{CameraLocation, ObjectId, ObjectInfo, ObjectKind};
use glam::Vec3;
use indexmap::IndexMap;
use render_core::{computeSkeletonPose::SkeletonPose, render_scene::computeRenderObjectTransform, renderable::RenderSnapshot, renderer_types::RenderObject};
use shared::game::{
    gui_types::{GuiRenderedRoot, ScriptGuiKey},
    jsRuntime::{CameraLimits, MouseCursor},
    world::World,
};
use std::collections::HashMap;

// This snapshot is the boundary packet from the sim thread to the render thread.
// Keeping the render-facing data from one tick together avoids mixing camera and object state from different ticks.
pub struct FullWorldRenderSnapshot {
    pub snapshot_live_client_sim_ms: f64,
    pub snapshot_client_world_delay_ms: f32, // the client world delay in effect when this snapshot was produced - the renderer reads it from here so there is a single source of truth
    pub render_snapshot: RenderSnapshot,
    pub camera_target_object_id: Option<ObjectId>,
    pub fallback_camera_location: CameraLocation,
    pub gui_root_of_script_gui_key: IndexMap<ScriptGuiKey, GuiRenderedRoot>,
    pub players_list: Vec<PlayersListEntry>, // in the order the players joined
    pub camera_limits: CameraLimits,
    pub mouse_cursor: MouseCursor,
}

impl FullWorldRenderSnapshot {
    pub fn new(
        render_snapshot: RenderSnapshot,
        world: &World,
        player_object_id: ObjectId,
        live_client_sim_ms: f64,
        snapshot_client_world_delay_ms: f32,
        gui_root_of_script_gui_key: IndexMap<ScriptGuiKey, GuiRenderedRoot>,
        camera_limits: CameraLimits,
        mouse_cursor: MouseCursor,
        ping_ms_of_player_object_id: &HashMap<ObjectId, u16>,
    ) -> Self {
        let camera_target_object_id = world.getPlayerHeadObjectId(player_object_id);
        let mut player_object_ids: Vec<ObjectId> = world._kindOfObjectId.get_keys(&ObjectKind::Player).into_iter().flatten().copied().collect();
        player_object_ids.sort();
        let players_list = player_object_ids
            .into_iter()
            .filter_map(|object_id| match &world.getObject(object_id).ok()?.info {
                ObjectInfo::Player(player) => Some(PlayersListEntry {
                    username: player.username.clone(),
                    is_local_player: object_id == player_object_id,
                    ping_ms: ping_ms_of_player_object_id.get(&object_id).copied(),
                }),
                _ => None,
            })
            .collect();
        Self {
            snapshot_live_client_sim_ms: live_client_sim_ms,
            snapshot_client_world_delay_ms,
            render_snapshot,
            camera_target_object_id,
            fallback_camera_location: world.fallback_camera_location,
            gui_root_of_script_gui_key,
            players_list,
            camera_limits,
            mouse_cursor,
        }
    }

    // Where the player's head is among these posed objects, which is what the camera follows.
    pub fn find_camera_target_position(
        &self,
        render_object_of_id: &HashMap<ObjectId, RenderObject>,
        skeleton_pose_of_id: &HashMap<ObjectId, SkeletonPose>,
    ) -> Option<Vec3> {
        let camera_target_object_id = self.camera_target_object_id?;
        let transform = computeRenderObjectTransform(render_object_of_id, skeleton_pose_of_id, camera_target_object_id)?;
        Some(transform.w_axis.truncate())
    }
}
