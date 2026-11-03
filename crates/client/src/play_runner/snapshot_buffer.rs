use game_types::{
    consts::{CLIENT_INTERPOLATION_WINDOW_MS, CLIENT_TOTAL_SNAPSHOT_BUFFER_MS, SERVER_SIM_AND_SEND_SECS},
    object_types::ObjectId,
};
use shared::{
    game::{interpolate_physics::get_interpolated_pose, timeline::Timeline, world::World},
    helpers::sorted_vec_deque::SortedVecDeque,
    protocol::messages::{
        ForeignCharacterDelta, ForeignPlayerDelta, MessageNum, NetworkedCoords, NetworkedPhysicsInfo3D, ObjectAddDelta, ObjectDelta, ObjectModifyDelta,
        ServerSnapshotMessage,
    },
};
use std::collections::HashMap;

enum AwaitingValidParentChange {
    ChangeParentTo(ObjectId, ObjectId),  // object, parent to set
    AddObject(ObjectId, ObjectAddDelta), // object, object being added
}

// The changes a snapshot makes all at once when it's due. Its poses, cameras and metadata go to timelines instead.
#[derive(Default)]
pub struct SnapshotChanges {
    pub modifies: Vec<(ObjectId, ObjectModifyDelta)>,
    pub removes: Vec<ObjectId>,
}

struct BufferedSnapshot {
    live_server_sim_ms: f64,
    changes: SnapshotChanges,
}

// ---------------------- SnapshotInterpolationBuffer ----------------------

// The snapshots we have here are the ones we've ACKed. Dropping some snapshots here is fine, we won't receive nor ACK them so we'll get them again if they're important.
const EXTRA_CUSHION_NUM: usize = 2; // we need an extra snapshot on each side of the window for when we're between snapshots.
const TOTAL_BUFFER_SECS: f32 = CLIENT_TOTAL_SNAPSHOT_BUFFER_MS / 1000.0;
const MAX_NUM_RECENT_SNAPSHOTS: usize = (TOTAL_BUFFER_SECS / SERVER_SIM_AND_SEND_SECS) as usize + EXTRA_CUSHION_NUM;

// Dropping an entry here permanently loses its adds (the client already acked the message, so the
// server never resends them), so this must comfortably exceed any realistic burst of add-carrying
// snapshots — e.g. a whole character spawning while snapshots aren't being popped yet.
const MAX_REMEMBERED_MESSAGES_WITH_PARENT_CHANGES: usize = 4096;

pub struct SnapshotInterpolationBuffer {
    // snapshot buffer:
    buffer: SortedVecDeque<MessageNum>, // message_nums ordered by live_server_sim_ms (and thus by message_num), front=oldest
    snapshotOfMessageNum: HashMap<MessageNum, BufferedSnapshot>, // message_num -> snapshot

    // parent change buffer:
    // mirrors buffer/snapshotOfMessageNum. changes are only applied once their message has been popped from the snapshot buffer.
    waiting_messages: SortedVecDeque<MessageNum>, // message_nums that have parent changes, front=oldest
    changes_of_message: HashMap<MessageNum, Vec<AwaitingValidParentChange>>, // message_num -> list of (object_id, desired_parent_id)

    // timelines: poses, and each player's camera and metadata, each shown at its own time
    pose_timeline_of_objectid: HashMap<ObjectId, Timeline<NetworkedPhysicsInfo3D>>,
    foreign_player_timeline_of_objectid: HashMap<ObjectId, Timeline<ForeignPlayerDelta>>,
}

impl SnapshotInterpolationBuffer {
    pub fn new() -> Self {
        Self {
            buffer: SortedVecDeque::with_capacity(MAX_NUM_RECENT_SNAPSHOTS),
            snapshotOfMessageNum: HashMap::with_capacity(MAX_NUM_RECENT_SNAPSHOTS),

            waiting_messages: SortedVecDeque::new(),
            changes_of_message: HashMap::new(),

            pose_timeline_of_objectid: HashMap::new(),
            foreign_player_timeline_of_objectid: HashMap::new(),
        }
    }

    fn _pop_front_snapshot(&mut self) -> Option<BufferedSnapshot> {
        let removed_message_num = self.buffer.pop_front()?;
        self.snapshotOfMessageNum.remove(&removed_message_num)
    }

    // --------- process a new snapshot from the server ---------
    pub fn process_new_snapshot(&mut self, snapshot: ServerSnapshotMessage) {
        let ServerSnapshotMessage {
            message_num,
            live_server_sim_ms,
            deltaOfObjectId,
            deltaOfObjectId2: _,
        } = snapshot;
        if self.snapshotOfMessageNum.contains_key(&message_num) {
            // should never happen, but just in case weird network conditions
            log::warn!("Weird network conditions caused the same snapshot twice, this is fine just adding this for curiosity");
            return;
        }

        // poses, cameras and metadata go to their timelines, adds and parent changes wait for their parents, and the rest waits for the snapshot to be due
        let mut changes = SnapshotChanges::default();
        let mut changes_for_message: Vec<AwaitingValidParentChange> = Vec::new();
        for (object_id, delta) in deltaOfObjectId {
            match delta {
                ObjectDelta::Add(add_delta) => changes_for_message.push(AwaitingValidParentChange::AddObject(object_id, add_delta)),
                ObjectDelta::Modify(mut modify_delta) => {
                    if let Some(desired_parent_id) = modify_delta.modifiedParent {
                        changes_for_message.push(AwaitingValidParentChange::ChangeParentTo(object_id, desired_parent_id));
                    }
                    if let Some(pose) = modify_delta.modifiedPhysicsInfo3D.take() {
                        self.pose_timeline_of_objectid
                            .entry(object_id)
                            .or_insert_with(Timeline::new)
                            .insert(live_server_sim_ms, pose);
                    }
                    changes.modifies.push((object_id, modify_delta));
                }
                ObjectDelta::Remove(_) => changes.removes.push(object_id),
                ObjectDelta::ForeignCharacter(ForeignCharacterDelta {
                    physicsInfo3D,
                    est_live_server_sim_ms,
                    teleportCount,
                }) => {
                    let pose = NetworkedPhysicsInfo3D {
                        coords: NetworkedCoords::WorldCoords(physicsInfo3D),
                        teleportCount,
                    };
                    self.pose_timeline_of_objectid
                        .entry(object_id)
                        .or_insert_with(Timeline::new)
                        .insert(est_live_server_sim_ms, pose);
                }
                ObjectDelta::ForeignPlayer(foreign_player_delta) => {
                    self.foreign_player_timeline_of_objectid
                        .entry(object_id)
                        .or_insert_with(Timeline::new)
                        .insert(foreign_player_delta.est_live_server_sim_ms, foreign_player_delta);
                }
            }
        }

        // waiting_messages + change_of_message + len check
        if !changes_for_message.is_empty() {
            self.waiting_messages.rinsert(message_num);
            self.changes_of_message.insert(message_num, changes_for_message);
            if !(self.waiting_messages.len() <= MAX_REMEMBERED_MESSAGES_WITH_PARENT_CHANGES) {
                log::error!("waiting_messages is full, dropping parent change {message_num}");
                let Some(msg_num) = self.waiting_messages.pop_front() else {
                    return;
                };
                let _ = self.changes_of_message.remove(&msg_num);
            }
        }

        // buffer + snapshotOfMessageNum + len check
        self.buffer.rinsert(message_num);
        self.snapshotOfMessageNum.insert(message_num, BufferedSnapshot { live_server_sim_ms, changes });
        if !(self.buffer.len() <= MAX_NUM_RECENT_SNAPSHOTS) {
            let _ = self._pop_front_snapshot();
        }
    }

    // --------- pop old snapshots whose changes are due ---------
    pub fn pop_due_snapshots(&mut self, est_dely_server_sim_ms: f64) -> SnapshotChanges {
        // pop snapshots from the ordered queue until we reach the est_dely_server_sim_ms
        let mut due_changes = SnapshotChanges::default();
        loop {
            // get the front message number
            let Some(front_msg_num_ptr) = self.buffer.front() else {
                break;
            };
            // get the snapshot with that message number
            let Some(front_snapshot_ptr) = self.snapshotOfMessageNum.get(front_msg_num_ptr) else {
                let _ = self.buffer.pop_front(); // should never happen, but remove if its corresponding entry is missing
                break;
            };
            // stop if the snapshot is not old enough
            if front_snapshot_ptr.live_server_sim_ms > est_dely_server_sim_ms {
                break;
            }

            // finally, pop the snapshot. these should never be None since we just read them...
            let Some(BufferedSnapshot {
                changes: SnapshotChanges { modifies, removes },
                ..
            }) = self._pop_front_snapshot()
            else {
                break;
            };
            due_changes.modifies.extend(modifies);
            due_changes.removes.extend(removes);
        }
        due_changes
    }

    // --------- pop deferred adds and parent changes whose message has been popped from the snapshot buffer ---------
    pub fn pop_due_valid_parent_deltas(&mut self, world: &World) -> (Vec<(ObjectId, ObjectAddDelta)>, Vec<(ObjectId, ObjectId)>) {
        let mut valid_parent_changes = Vec::new();
        let mut valid_adds = Vec::new();
        let mut message_nums_to_remove = Vec::new();

        for &msg_num in self.waiting_messages.iter() {
            // since both deques are sorted, once we hit a message still in the snapshot buffer, all later ones are too
            if self.snapshotOfMessageNum.contains_key(&msg_num) {
                break;
            }

            let Some(desired_parent_changes) = self.changes_of_message.get_mut(&msg_num) else {
                message_nums_to_remove.push(msg_num);
                continue;
            };

            let mut remaining_changes = Vec::new();
            for change_awaiting in desired_parent_changes.drain(..) {
                match change_awaiting {
                    AwaitingValidParentChange::ChangeParentTo(object_id, desired_parent_id) => {
                        if world.canAddParent(object_id, desired_parent_id) {
                            valid_parent_changes.push((object_id, desired_parent_id));
                        } else {
                            remaining_changes.push(AwaitingValidParentChange::ChangeParentTo(object_id, desired_parent_id));
                        }
                    }
                    AwaitingValidParentChange::AddObject(object_id, add_delta) => {
                        if world.canAddParent(object_id, add_delta.parentId) {
                            valid_adds.push((object_id, add_delta));
                        } else {
                            remaining_changes.push(AwaitingValidParentChange::AddObject(object_id, add_delta));
                        }
                    }
                }
            }
            if remaining_changes.is_empty() {
                message_nums_to_remove.push(msg_num);
            } else {
                *desired_parent_changes = remaining_changes;
            }
        }

        for msg_num in message_nums_to_remove {
            self.waiting_messages.remove(&msg_num);
            self.changes_of_message.remove(&msg_num);
        }

        if !valid_adds.is_empty() {
            log::info!("valid_adds: {:?}", valid_adds);
        }
        (valid_adds, valid_parent_changes)
    }

    // --------- the poses, cameras and metadata shown at this moment ---------

    // Each entry is kept for CLIENT_INTERPOLATION_WINDOW_MS after it's due, so a late next one can still be blended into.
    // A timeline with no entries left stops moving its object, so it doesn't fight whatever else moves it, like an animation.
    pub fn forget_old_timeline_entries(&mut self, est_dely_server_sim_ms: f64) {
        let forget_before_est_dely_server_sim_ms = est_dely_server_sim_ms - CLIENT_INTERPOLATION_WINDOW_MS as f64;
        for pose_timeline in self.pose_timeline_of_objectid.values_mut() {
            pose_timeline.forget_entries_before(forget_before_est_dely_server_sim_ms);
        }
        for foreign_player_timeline in self.foreign_player_timeline_of_objectid.values_mut() {
            foreign_player_timeline.forget_entries_before(forget_before_est_dely_server_sim_ms);
        }
        self.pose_timeline_of_objectid.retain(|_, pose_timeline| !pose_timeline.is_empty());
        self.foreign_player_timeline_of_objectid
            .retain(|_, foreign_player_timeline| !foreign_player_timeline.is_empty());
    }

    pub fn get_interpolated_poses(&self, est_dely_server_sim_ms: f64) -> Vec<(ObjectId, NetworkedPhysicsInfo3D)> {
        self.pose_timeline_of_objectid
            .iter()
            .filter_map(|(&object_id, pose_timeline)| Some((object_id, get_interpolated_pose(pose_timeline, est_dely_server_sim_ms)?)))
            .collect()
    }

    pub fn get_due_foreign_players(&self, est_dely_server_sim_ms: f64) -> Vec<(ObjectId, ForeignPlayerDelta)> {
        self.foreign_player_timeline_of_objectid
            .iter()
            .filter_map(|(&object_id, foreign_player_timeline)| {
                let (_, foreign_player_delta) = foreign_player_timeline.find_last_entry_at_or_before(est_dely_server_sim_ms)?;
                Some((object_id, foreign_player_delta.clone()))
            })
            .collect()
    }
}
