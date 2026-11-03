use crate::game::world::WorldControlMode;
use crate::helpers::hashmap_with_reverse::{HashMapWithReverse, HashMapWithReverse1To1};
use anyhow::anyhow;
use game_types::{
    consts::DEFAULT_GRAVITY,
    object_types::{Location3D, Movement3D, ObjectId, PhysicsInfo3D, PhysicsToggles3D, shared::is_client_object_id},
    physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings, CylinderPrimitiveSettings, PrimitiveSettings},
};
use glam::{Quat, Vec3};
// use rapier3d::parry::bounding_volume::BoundingVolume;
use rapier3d::parry::utils::PoseOpt;
use rapier3d::prelude::*;
use std::collections::{HashMap, HashSet};

const CONTROLLED_OBJECT_FRICTION: f32 = 0.0;

// whether a physics write is a jump or a smooth move
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SetOrForce {
    Force, // 'force' it to the new place
    Set,   // set it to the new place but dont make a physics discontinuity in velocity
}

// struct that lets you specify which part of physicsInfo3D to update
#[derive(Clone, Copy, Default)]
pub struct SetPhysicsInfo3D {
    pub position: Option<Vec3>,
    pub rotation: Option<Quat>,
    pub velocity: Option<Vec3>,
    pub angularVelocity: Option<Vec3>,
}
impl SetPhysicsInfo3D {
    pub fn from(physicsInfo3D: PhysicsInfo3D) -> Self {
        Self {
            position: Some(physicsInfo3D.location.position),
            rotation: Some(physicsInfo3D.location.quaternion),
            velocity: Some(physicsInfo3D.movement.velocity),
            angularVelocity: Some(physicsInfo3D.movement.angularVelocity),
        }
    }
}

// struct that lets you specify which part of physicsToggles3D to update
#[derive(Clone, Copy, Default)]
pub struct SetPhysicsToggles3D {
    pub isWalkthrough: Option<bool>,
    pub isPushable: Option<bool>,
    pub density: Option<f32>,
}

pub enum CollisionPairEvent {
    Started { object_id_1: ObjectId, object_id_2: ObjectId },
    Stopped { object_id_1: ObjectId, object_id_2: ObjectId },
}

#[derive(Clone, Copy, Debug)]
pub struct ContactHit {
    pub object_id: ObjectId,
    pub normal: Vec3,
    pub position: Vec3,
}

#[derive(Clone, Copy, Debug)]
pub struct CastHit {
    pub object_id: ObjectId,
    pub normal: Vec3,
    pub distance: f32,
    pub position: Vec3, // the world-space point where it hit, on the surface of the object that was hit
}

// Rapier Docs - https://rapier.rs/docs/user_guides/rust/getting_started/

/*
We want to allow objects to have these properties. They are only defined for ball/brick/cylinder objectid primitives, not rigidgroups.
TLDR: pushable is part of rigid_body, but density and isWalkthrough are part of each individual collider. This makes pushable special.

isPushable:
    - Rapier doesn't support subitems being kinematic/dynamic, only rigid bodies.
    - For our purposes we want to allow both, where if any subitem is not pushable, its rigid body's state is treated as not pushable. this must always be upheld.
    - For our purposes not pushable means Fixed until something first moves it, then kinematic, and pushable always means dynamic.
Density:
    - For consistency, rigid bodies should not store density, only colliders should store density
    - Always read density with collider.density()
isWalkthrough:
    - isWalkthrough means the collider is a sensor
    - two walkthrough colliders never pair up, so they never collide or fire collision events with each other
*/

// ---------------- Colliders ----------------

fn ballCollider(radius: f32) -> ColliderBuilder {
    ColliderBuilder::ball(radius.abs())
}

fn brickCollider(size: Vec3) -> ColliderBuilder {
    let half = size.abs() * 0.5;
    let collider = ColliderBuilder::cuboid(half.x, half.y, half.z);
    collider
}

fn cylinderCollider(radius: f32, length: f32) -> ColliderBuilder {
    let radius = radius.abs().max(f32::EPSILON);
    let half_length = (length.abs() * 0.5).max(f32::EPSILON);
    let y_to_z_rotation = Pose::from_parts(Vec3::ZERO, Quat::from_scaled_axis(Vec3::new(std::f32::consts::FRAC_PI_2, 0.0, 0.0)));
    ColliderBuilder::compound(vec![(y_to_z_rotation, SharedShape::cylinder(half_length, radius))])
}

// Nothing physical happens between two walkthrough objects, and a pile of them would put every pair in it through a check on every step.
fn getWalkthroughCollisionGroups(isWalkthrough: bool) -> InteractionGroups {
    match isWalkthrough {
        true => InteractionGroups::new(Group::GROUP_1, Group::ALL.difference(Group::GROUP_1), InteractionTestMode::And),
        false => InteractionGroups::all(),
    }
}

fn createPrimitiveColliderBuilder(info: PrimitiveSettings) -> ColliderBuilder {
    match info {
        PrimitiveSettings::Ball(BallPrimitiveSettings { radius }) => ballCollider(radius),
        PrimitiveSettings::Brick(BrickPrimitiveSettings { size }) => brickCollider(size),
        PrimitiveSettings::Cylinder(CylinderPrimitiveSettings { radius, length }) => cylinderCollider(radius, length),
    }
}

// fn getAllSubitems(rigidGroupObjId: ObjectId, world: &World) -> Vec<SubitemInfo> {
//     let mut subitems = Vec::new();
// }

// ---------------- Actual Physics struct ----------------
pub struct Physics {
    rigid_body_set: RigidBodySet,
    collider_set: ColliderSet,
    physics_pipeline: PhysicsPipeline,
    island_manager: IslandManager,
    broad_phase: DefaultBroadPhase,
    narrow_phase: NarrowPhase,
    impulse_joint_set: ImpulseJointSet,
    multibody_joint_set: MultibodyJointSet,
    ccd_solver: CCDSolver,
    gravity: Vector,

    // our metadata:
    // ball/brick/cylinder shape data (owned by physics)
    pub primitiveSettingsOfId: HashMap<ObjectId, PrimitiveSettings>,

    // rigidgroup
    groupRigidBodyHandleOfObjId: HashMapWithReverse1To1<ObjectId, RigidBodyHandle>,
    groupObjIdOfSubitemObjId: HashMapWithReverse<ObjectId, ObjectId>,
    groupIsControlledByUsOfObjId: HashMap<ObjectId, bool>,

    // subitem inside rigidgroup
    subitemColliderHandleOfObjId: HashMapWithReverse1To1<ObjectId, ColliderHandle>,
    subitemIsPushableMetadataOfObjId: HashMap<ObjectId, bool>,

    // standalone rigid body
    standaloneRigidBodyHandleOfObjId: HashMapWithReverse1To1<ObjectId, RigidBodyHandle>,
    standaloneColliderHandleOfObjId: HashMapWithReverse1To1<ObjectId, ColliderHandle>,
    standaloneIsPushableMetadataOfObjId: HashMap<ObjectId, bool>,
    standaloneIsControlledByUsOfObjId: HashMap<ObjectId, bool>,

    // kinematic objects dont have intrinsic velocity, so we store the latest-set velocity here
    kinematicLatestVelocityOfObjId: HashMap<ObjectId, Movement3D>,

    // bodies whose pose changed during a step since the server last took them to send
    objectIdsMovedByStep: HashSet<ObjectId>,

    // copied from world
    worldControlMode: WorldControlMode,
}

// Object reads include kinematic moves that have not been simulated yet.
fn getLatestRigidBodyPose(rigid_body: &RigidBody) -> &Pose {
    // 99% of the time position and next_position will be the same, except in internal intermediate cases where we wrote to physics before stepping, like for animations
    if rigid_body.is_kinematic() {
        rigid_body.next_position()
    } else {
        rigid_body.position()
    }
}

// Rapier checks a kinematic body against everything it touches on every step, and a Fixed one only when something moves,
// so a body stays Fixed until it's first given somewhere to go.
// TODO: turn a body back to Fixed once it's still. Rapier 0.35 can't do that yet: a body it has woken stays on its awake
// list until it's removed or disabled, so it's still checked every step, and a Fixed body there gets re-placed through its
// center of mass every step, which drifts it by rounding error. It needs a Rapier change that takes a body off the list.
fn setNextKinematicPose(rigid_body: &mut RigidBody, pose: Pose) {
    if rigid_body.is_fixed() {
        if *rigid_body.position() == pose {
            return;
        }
        rigid_body.set_body_type(RigidBodyType::KinematicPositionBased, true);
    }
    rigid_body.set_next_kinematic_position(pose);
}

// A kinematic body may have a move it hasn't made yet, and Rapier never takes a body it has woken out of its awake list,
// where a Fixed body is re-placed through its center of mass every step. So neither turns Fixed: they turn kinematic.
fn setBodyTypeKeepingKinematic(rigid_body: &mut RigidBody, is_awake: bool, body_type: RigidBodyType) {
    let is_kept_kinematic = body_type == RigidBodyType::Fixed && (rigid_body.is_kinematic() || is_awake || rigid_body.is_sleeping());
    match is_kept_kinematic {
        true => rigid_body.set_body_type(RigidBodyType::KinematicPositionBased, false),
        false => rigid_body.set_body_type(body_type, false),
    }
}

impl Physics {
    pub fn new(worldControlMode: WorldControlMode) -> Self {
        let rigid_body_set = RigidBodySet::new();
        let collider_set = ColliderSet::new();
        let gravity = Vec3::new(0.0, -DEFAULT_GRAVITY, 0.0);
        let physics_pipeline = PhysicsPipeline::new();
        let island_manager = IslandManager::new();
        let broad_phase = DefaultBroadPhase::new();
        let narrow_phase = NarrowPhase::new();
        let impulse_joint_set = ImpulseJointSet::new();
        let multibody_joint_set = MultibodyJointSet::new();
        let ccd_solver = CCDSolver::new();

        Self {
            rigid_body_set,
            collider_set,
            physics_pipeline,
            island_manager,
            broad_phase,
            narrow_phase,
            impulse_joint_set,
            multibody_joint_set,
            ccd_solver,
            gravity,
            // our metadata
            primitiveSettingsOfId: HashMap::new(),
            groupRigidBodyHandleOfObjId: HashMapWithReverse1To1::new(),
            groupObjIdOfSubitemObjId: HashMapWithReverse::new(),
            groupIsControlledByUsOfObjId: HashMap::new(),
            subitemColliderHandleOfObjId: HashMapWithReverse1To1::new(),
            subitemIsPushableMetadataOfObjId: HashMap::new(),
            standaloneRigidBodyHandleOfObjId: HashMapWithReverse1To1::new(),
            standaloneColliderHandleOfObjId: HashMapWithReverse1To1::new(),
            standaloneIsPushableMetadataOfObjId: HashMap::new(),
            standaloneIsControlledByUsOfObjId: HashMap::new(),
            kinematicLatestVelocityOfObjId: HashMap::new(),
            objectIdsMovedByStep: HashSet::new(),
            worldControlMode,
        }
    }

    pub fn update_collisions(&mut self) {
        let velocities: Vec<_> = self.rigid_body_set.iter().map(|(handle, body)| (handle, body.linvel(), body.angvel())).collect();
        let _ = self.step(0.0);
        for (handle, linvel, angvel) in velocities {
            let Some(body) = self.rigid_body_set.get_mut(handle) else { continue };
            if body.linvel() != linvel || body.angvel() != angvel {
                body.set_linvel(linvel, false);
                body.set_angvel(angvel, false);
            }
        }
    }

    pub fn step(&mut self, dt_secs: f32) -> Vec<CollisionPairEvent> {
        let integration_parameters = IntegrationParameters {
            dt: dt_secs,
            // Otherwise Rapier sweeps fast bodies against Fixed ones, which changes how fast things hit anchored blocks.
            max_ccd_substeps: 0,
            // Otherwise contacts with Fixed bodies can be reused while up to 5 cm out of date, which breaks ground checks.
            contact_recycling: false,
            ..Default::default()
        };
        let (collision_send, collision_recv) = std::sync::mpsc::channel();
        let (contact_force_send, _contact_force_recv) = std::sync::mpsc::channel();
        let event_handler = ChannelEventCollector::new(collision_send, contact_force_send);

        // Moved means the pose changed. Rapier's velocity can't tell us: a rotated kinematic body that never moves gets rounding error as its velocity.
        let pose_before_step_of_body: HashMap<RigidBodyHandle, Pose> = match self.worldControlMode.should_mark_objects() {
            true => self
                .island_manager
                .active_bodies()
                .filter_map(|handle| Some((handle, *self.rigid_body_set.get(handle)?.position())))
                .collect(),
            false => HashMap::new(),
        };

        self.physics_pipeline.step(
            self.gravity,
            &integration_parameters,
            &mut self.island_manager,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.rigid_body_set,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            &mut self.ccd_solver,
            &(), // <-- this lets us change the behavior of the physics engine
            &event_handler,
        );

        // A body that was just added or just woken up was not awake before the step, so it has no pose to compare and counts as moved.
        if self.worldControlMode.should_mark_objects() {
            let moved_object_ids: Vec<ObjectId> = self
                .island_manager
                .active_bodies()
                .filter(|handle| pose_before_step_of_body.get(handle) != self.rigid_body_set.get(*handle).map(RigidBody::position))
                .filter_map(|handle| self._getObjectIdfRigidBodyHandle(&handle))
                .collect();
            self.objectIdsMovedByStep.extend(moved_object_ids);
        }

        // Rapier recommends draining the collision channel after the step so we can handle every collision event in order.
        let mut collision_pair_events = Vec::new();
        while let Ok(collision_event) = collision_recv.try_recv() {
            let (collider_handle_1, collider_handle_2, did_start) = match collision_event {
                CollisionEvent::Started(collider_handle_1, collider_handle_2, _) => (collider_handle_1, collider_handle_2, true),
                CollisionEvent::Stopped(collider_handle_1, collider_handle_2, _) => (collider_handle_1, collider_handle_2, false),
            };
            let Some(object_id_1) = self._getObjectIdOfColliderHandle(collider_handle_1) else {
                continue;
            };
            let Some(object_id_2) = self._getObjectIdOfColliderHandle(collider_handle_2) else {
                continue;
            };
            let collision_pair_event = if did_start {
                CollisionPairEvent::Started { object_id_1, object_id_2 }
            } else {
                CollisionPairEvent::Stopped { object_id_1, object_id_2 }
            };
            collision_pair_events.push(collision_pair_event);
        }
        collision_pair_events
    }

    // ---------------- getters ----------------

    pub fn getGravity(&self) -> Vec3 {
        self.gravity
    }

    pub fn getBallPrimitiveSettings(&self, objectId: ObjectId) -> Option<BallPrimitiveSettings> {
        match self.primitiveSettingsOfId.get(&objectId) {
            Some(PrimitiveSettings::Ball(info)) => Some(*info),
            _ => None,
        }
    }

    pub fn getBrickPrimitiveSettings(&self, objectId: ObjectId) -> Option<BrickPrimitiveSettings> {
        match self.primitiveSettingsOfId.get(&objectId) {
            Some(PrimitiveSettings::Brick(info)) => Some(*info),
            _ => None,
        }
    }

    pub fn getCylinderPrimitiveSettings(&self, objectId: ObjectId) -> Option<CylinderPrimitiveSettings> {
        match self.primitiveSettingsOfId.get(&objectId) {
            Some(PrimitiveSettings::Cylinder(info)) => Some(*info),
            _ => None,
        }
    }

    pub fn getPrimitiveSettings(&self, objectId: ObjectId) -> Option<PrimitiveSettings> {
        self.primitiveSettingsOfId.get(&objectId).copied()
    }

    pub fn getObjectPhysicsInfo3D(&self, objectId: ObjectId) -> Option<PhysicsInfo3D> {
        fn _physicsInfo3DFromRigidBody(rigid_body: &RigidBody) -> Option<PhysicsInfo3D> {
            let position = getLatestRigidBodyPose(rigid_body);
            let velocity = rigid_body.linvel();
            let angularVelocity = rigid_body.angvel();

            return Some(PhysicsInfo3D::new(position.translation, position.rotation, velocity, angularVelocity));
        }
        fn _physicsInfo3DFromSubitemCollider(rigid_body: &RigidBody, collider: &Collider) -> Option<PhysicsInfo3D> {
            let local_collider_position = collider.position_wrt_parent()?;
            // Children and their group must use the same pose, including a move waiting for the next physics step.
            let position = getLatestRigidBodyPose(rigid_body);
            let world_collider_position = *position * *local_collider_position;
            let translation = world_collider_position.translation;
            let rotation = world_collider_position.rotation;
            let velocity = rigid_body.linvel();
            let angularVelocity = rigid_body.angvel();

            return Some(PhysicsInfo3D::new(translation, rotation, velocity, angularVelocity));
        }

        // group
        if let Some(rigid_body_handle) = self.groupRigidBodyHandleOfObjId.get(&objectId) {
            let rigid_body = self.rigid_body_set.get(*rigid_body_handle)?;
            return _physicsInfo3DFromRigidBody(rigid_body);
        }

        // standalone
        if let Some(rigid_body_handle) = self.standaloneRigidBodyHandleOfObjId.get(&objectId) {
            let rigid_body = self.rigid_body_set.get(*rigid_body_handle)?;
            return _physicsInfo3DFromRigidBody(rigid_body);
        }

        // subitem
        if let Some(collider_handle) = self.subitemColliderHandleOfObjId.get(&objectId) {
            let collider = self.collider_set.get(*collider_handle)?;

            let group_obj_id = *self.groupObjIdOfSubitemObjId.get(&objectId)?;
            let rigid_body_handle = *self.groupRigidBodyHandleOfObjId.get(&group_obj_id)?;
            let rigid_body = self.rigid_body_set.get(rigid_body_handle)?;

            return _physicsInfo3DFromSubitemCollider(rigid_body, collider);
        }

        None
    }

    // The ground's velocity sampled at object_id's center of mass.
    fn _getGroundVelocityAtCoM(&self, object_id: ObjectId, ground_object_id: ObjectId) -> Option<Vec3> {
        let center_of_mass = self.rigid_body_set.get(self._getAncestorRigidBodyHandle(object_id)?)?.center_of_mass();
        let ground_rigid_body = self.rigid_body_set.get(self._getAncestorRigidBodyHandle(ground_object_id)?)?;
        Some(ground_rigid_body.velocity_at_point(center_of_mass))
    }

    pub fn getVelocityRelativeToGround(&self, object_id: ObjectId, max_distance_to_ground: f32, max_ground_slope_degrees: f32) -> Option<Vec3> {
        let velocity = self.getObjectPhysicsInfo3D(object_id)?.movement.velocity;
        let Some(ground_support_hit) = self.getGroundSupport(object_id, max_distance_to_ground, max_ground_slope_degrees) else {
            return Some(velocity);
        };
        let ground_velocity = self._getGroundVelocityAtCoM(object_id, ground_support_hit.object_id).unwrap_or_default();
        Some(velocity - ground_velocity)
    }

    // Read the body that owns this collider so rendering can compose its local pose with that body.
    pub fn getSubitemRigidGroupId(&self, object_id: ObjectId) -> Option<ObjectId> {
        self.groupObjIdOfSubitemObjId.get(&object_id).copied()
    }

    pub fn getRigidGroupSubitemIds(&self, group_id: ObjectId) -> Vec<ObjectId> {
        self.groupObjIdOfSubitemObjId.get_keys(&group_id).map(|ids| ids.iter().copied().collect()).unwrap_or_default()
    }

    pub fn getSubitemRelativeLocation3D(&self, object_id: ObjectId) -> Option<Location3D> {
        let collider_handle = self.subitemColliderHandleOfObjId.get(&object_id)?;
        let collider = self.collider_set.get(*collider_handle)?;
        let local_collider_position = collider.position_wrt_parent()?;
        Some(Location3D {
            position: local_collider_position.translation,
            quaternion: local_collider_position.rotation,
        })
    }

    pub fn computeSubitemRelativeLocation3DToGroup(&self, worldLocation3D: &Location3D, rigidGroupObjId: ObjectId) -> anyhow::Result<Location3D> {
        let rigid_body_handle = self
            .groupRigidBodyHandleOfObjId
            .get(&rigidGroupObjId)
            .ok_or_else(|| anyhow!("Rigid body handle not found"))?;
        let rigid_body = self.rigid_body_set.get(*rigid_body_handle).ok_or_else(|| anyhow!("Rigid body not found"))?;

        // Convert child writes using the same parent pose as child reads.
        let parent_pose = getLatestRigidBodyPose(rigid_body);
        let parent_translation = parent_pose.translation;
        let parent_rotation = parent_pose.rotation;
        let child_translation = worldLocation3D.position;
        let child_rotation = worldLocation3D.quaternion;

        let world_offset = child_translation - parent_translation;
        let local_translation = parent_rotation.inverse() * world_offset;
        let local_rotation = parent_rotation.inverse() * child_rotation;
        Ok(Location3D {
            position: local_translation,
            quaternion: local_rotation,
        })
    }

    pub fn getObjectPhysicsToggles3D(&self, objectId: ObjectId) -> Option<PhysicsToggles3D> {
        // group
        if self.groupRigidBodyHandleOfObjId.contains_key(&objectId) {
            return None;
        }
        // standalone
        if self.standaloneRigidBodyHandleOfObjId.contains_key(&objectId) {
            let isPushable = *self.standaloneIsPushableMetadataOfObjId.get(&objectId)?;
            let collider_handle = self.standaloneColliderHandleOfObjId.get(&objectId)?;
            let collider = self.collider_set.get(*collider_handle)?;

            return Some(PhysicsToggles3D {
                isWalkthrough: collider.is_sensor(),
                isPushable,
                density: collider.density(),
            });
        }

        // subitem
        if let Some(collider_handle) = self.subitemColliderHandleOfObjId.get(&objectId) {
            let collider = self.collider_set.get(*collider_handle)?;
            let subitemIsPushable = *self.subitemIsPushableMetadataOfObjId.get(&objectId)?;

            return Some(PhysicsToggles3D {
                isWalkthrough: collider.is_sensor(),
                isPushable: subitemIsPushable,
                density: collider.density(),
            });
        }

        None
    }

    pub fn canSetObjectVelocity(&self, object_id: ObjectId) -> bool {
        if self.subitemColliderHandleOfObjId.contains_key(&object_id) {
            return false;
        }

        let Some(rigid_body_handle) = self._getRigidBodyHandle(object_id) else {
            return false;
        };
        let Some(rigid_body) = self.rigid_body_set.get(rigid_body_handle) else {
            return false;
        };

        rigid_body.is_dynamic()
    }

    pub fn isControlledByUs(&self, object_id: ObjectId) -> Option<bool> {
        // group
        if self.groupRigidBodyHandleOfObjId.contains_key(&object_id) {
            let val = self.groupIsControlledByUsOfObjId.get(&object_id)?;
            return Some(*val);
        }
        // standalone
        if self.standaloneRigidBodyHandleOfObjId.contains_key(&object_id) {
            let val = self.standaloneIsControlledByUsOfObjId.get(&object_id)?;
            return Some(*val);
        }
        // subitem
        if self.subitemColliderHandleOfObjId.contains_key(&object_id) {
            // defer to whether group is locally controlled
            let group_obj_id = self.groupObjIdOfSubitemObjId.get(&object_id)?;
            let val = self.groupIsControlledByUsOfObjId.get(group_obj_id)?;
            return Some(*val);
        }
        None
    }

    pub fn takeDirtyPhysicsObjectsControlledByUs(&mut self) -> Vec<ObjectId> {
        // subitems do not get dirty, they are just colliders
        std::mem::take(&mut self.objectIdsMovedByStep)
            .into_iter()
            .filter(|&object_id| self.isControlledByUs(object_id) == Some(true))
            .collect()
    }

    // ---------------- add/remove (track/untrack) objects ----------------

    pub fn startTrackingStandalone(
        &mut self,
        object_id: ObjectId,
        primitiveSettings: PrimitiveSettings,
        physicsInfo3D: &PhysicsInfo3D,
        physicsToggles3D: &PhysicsToggles3D,
        isControlledByUs: bool,
    ) {
        self.primitiveSettingsOfId.insert(object_id, primitiveSettings);

        let rigid_body_handle = self._createRigidBody(physicsInfo3D, physicsToggles3D.isPushable, isControlledByUs);

        let collider_builder = createPrimitiveColliderBuilder(primitiveSettings);
        let collider = collider_builder
            // Blocks that aren't pushable are Fixed or kinematic, so contacts between any two of them are needed too.
            .active_collision_types(ActiveCollisionTypes::all())
            .active_events(ActiveEvents::COLLISION_EVENTS)
            .density(physicsToggles3D.density)
            .friction(Self::_getFriction(object_id, isControlledByUs, &self.worldControlMode))
            .sensor(physicsToggles3D.isWalkthrough)
            .collision_groups(getWalkthroughCollisionGroups(physicsToggles3D.isWalkthrough))
            .build();
        let collider_handle = self.collider_set.insert_with_parent(collider, rigid_body_handle, &mut self.rigid_body_set);

        self.standaloneRigidBodyHandleOfObjId.insert(object_id, rigid_body_handle);
        self.standaloneColliderHandleOfObjId.insert(object_id, collider_handle);
        self.standaloneIsPushableMetadataOfObjId.insert(object_id, physicsToggles3D.isPushable);
        self.standaloneIsControlledByUsOfObjId.insert(object_id, isControlledByUs);
    }

    pub fn startTrackingSubitem(
        &mut self,
        object_id: ObjectId,
        rigidGroupObjId: ObjectId,
        primitiveSettings: PrimitiveSettings,
        relativeLocation3D: &Location3D,
        physicsToggles3D: &PhysicsToggles3D,
    ) -> anyhow::Result<()> {
        self.primitiveSettingsOfId.insert(object_id, primitiveSettings);

        let local_isometry = Self::_isometryOfLocation3D(relativeLocation3D);

        // a subitem that isn't pushable makes the group it's going in to not pushable
        if !physicsToggles3D.isPushable {
            if let Some(isControlledByUs) = self.groupIsControlledByUsOfObjId.get(&rigidGroupObjId) {
                self._setGroupBodyType(rigidGroupObjId, false, *isControlledByUs);
            } else {
                log::warn!("startTrackingSubitem: Group not found");
            }
        }

        let Some(&isControlledByUs) = self.groupIsControlledByUsOfObjId.get(&rigidGroupObjId) else {
            return Err(anyhow!("startTrackingSubitem: Group control metadata not found"));
        };
        let Some(parent_rigid_body_handle) = self.groupRigidBodyHandleOfObjId.get(&rigidGroupObjId) else {
            return Err(anyhow!("startTrackingSubitem: Parent rigid body handle not found"));
        };

        let collider_builder = createPrimitiveColliderBuilder(primitiveSettings);
        let collider = collider_builder
            // Blocks that aren't pushable are Fixed or kinematic, so contacts between any two of them are needed too.
            .active_collision_types(ActiveCollisionTypes::all())
            .active_events(ActiveEvents::COLLISION_EVENTS)
            .position(local_isometry)
            .density(physicsToggles3D.density)
            .friction(Self::_getFriction(rigidGroupObjId, isControlledByUs, &self.worldControlMode))
            .sensor(physicsToggles3D.isWalkthrough)
            .collision_groups(getWalkthroughCollisionGroups(physicsToggles3D.isWalkthrough))
            .build();
        let collider_handle = self
            .collider_set
            .insert_with_parent(collider, *parent_rigid_body_handle, &mut self.rigid_body_set);

        self.groupObjIdOfSubitemObjId.insert(object_id, rigidGroupObjId);
        self.subitemColliderHandleOfObjId.insert(object_id, collider_handle);
        self.subitemIsPushableMetadataOfObjId.insert(object_id, physicsToggles3D.isPushable);

        Ok(())
    }

    pub fn startTrackingGroup(&mut self, object_id: ObjectId, physicsInfo3D: &PhysicsInfo3D, isControlledByUs: bool) {
        // starts pushable since no subitems are added yet
        let rigid_body_handle = self._createRigidBody(physicsInfo3D, true, isControlledByUs);
        self.groupRigidBodyHandleOfObjId.insert(object_id, rigid_body_handle);
        self.groupIsControlledByUsOfObjId.insert(object_id, isControlledByUs);
    }

    fn _isometryOfLocation3D(location3D: &Location3D) -> Pose {
        let translation = location3D.position;
        let rotation = location3D.quaternion;
        Pose::from_parts(translation, rotation)
    }

    fn _setGroupBodyType(&mut self, rigidGroupObjId: ObjectId, isPushable: bool, isControlledByUs: bool) {
        let Some(rigid_body_handle) = self.groupRigidBodyHandleOfObjId.get(&rigidGroupObjId) else {
            log::error!("_setGroupBodyType: Rigid group not found");
            return;
        };

        let is_awake = self.island_manager.active_bodies().any(|handle| handle == *rigid_body_handle);
        let Some(rigid_body) = self.rigid_body_set.get_mut(*rigid_body_handle) else {
            log::error!("_setGroupBodyType: Rigid body not found");
            return;
        };
        setBodyTypeKeepingKinematic(rigid_body, is_awake, Self::_getBodyType(isPushable, isControlledByUs));
        let friction = Self::_getFriction(rigidGroupObjId, isControlledByUs, &self.worldControlMode);
        for &collider_handle in rigid_body.colliders() {
            let Some(collider) = self.collider_set.get_mut(collider_handle) else {
                log::error!("_setGroupBodyType: Collider not found");
                continue;
            };
            collider.set_friction(friction);
        }
    }

    fn _setStandaloneBodyType(&mut self, object_id: ObjectId, isPushable: bool, isControlledByUs: bool) {
        let Some(rigid_body_handle) = self.standaloneRigidBodyHandleOfObjId.get(&object_id) else {
            log::error!("_setStandaloneBodyType: Rigid body not found");
            return;
        };
        let is_awake = self.island_manager.active_bodies().any(|handle| handle == *rigid_body_handle);
        let Some(rigid_body) = self.rigid_body_set.get_mut(*rigid_body_handle) else {
            log::error!("_setStandaloneBodyType: Rigid body not found");
            return;
        };
        setBodyTypeKeepingKinematic(rigid_body, is_awake, Self::_getBodyType(isPushable, isControlledByUs));
        let friction = Self::_getFriction(object_id, isControlledByUs, &self.worldControlMode);
        for &collider_handle in rigid_body.colliders() {
            let Some(collider) = self.collider_set.get_mut(collider_handle) else {
                log::error!("_setStandaloneBodyType: Collider not found");
                continue;
            };
            collider.set_friction(friction);
        }
    }

    // A client controls its character and the objects it made for itself, and only the character slides without friction.
    fn _getFriction(object_id: ObjectId, isControlledByUs: bool, worldControlMode: &WorldControlMode) -> f32 {
        let is_local_players_character = match worldControlMode {
            WorldControlMode::Player(_) => isControlledByUs && !is_client_object_id(object_id),
            WorldControlMode::Server => false,
            WorldControlMode::Editor => false,
        };

        if is_local_players_character {
            CONTROLLED_OBJECT_FRICTION
        } else {
            ColliderBuilder::default_friction()
        }
    }

    fn _getBodyType(isPushable: bool, isControlledByUs: bool) -> RigidBodyType {
        if !isPushable || !isControlledByUs {
            RigidBodyType::Fixed
        } else {
            RigidBodyType::Dynamic
        }
    }

    fn _rigidBodyBuilder(&mut self, isPushable: bool, isControlledByUs: bool) -> RigidBodyBuilder {
        let bodyType = Self::_getBodyType(isPushable, isControlledByUs);
        match bodyType {
            RigidBodyType::Fixed => RigidBodyBuilder::fixed(),
            RigidBodyType::Dynamic => RigidBodyBuilder::dynamic(),
            _ => {
                // should never happen
                log::warn!("Invalid body type: {:?}", bodyType);
                RigidBodyBuilder::dynamic()
            }
        }
    }

    fn _createRigidBody(&mut self, physicsInfo3D: &PhysicsInfo3D, isPushable: bool, isControlledByUs: bool) -> RigidBodyHandle {
        let quat = physicsInfo3D.location.quaternion;
        let translation = physicsInfo3D.location.position;
        let unit_quat = quat;
        let isometry = Pose::from_parts(translation, unit_quat);

        // isPushable decides if it's fixed or not.
        let rigid_body = self
            ._rigidBodyBuilder(isPushable, isControlledByUs)
            .pose(isometry)
            .linvel(physicsInfo3D.movement.velocity)
            .angvel(physicsInfo3D.movement.angularVelocity)
            .build();

        // Add it to the rigid body set.
        let rigid_body_handle = self.rigid_body_set.insert(rigid_body);

        return rigid_body_handle;
    }

    fn _removeRigidBody(&mut self, rigid_body_handle: RigidBodyHandle) {
        self.rigid_body_set.remove(
            rigid_body_handle,
            &mut self.island_manager,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            true,
        );
    }

    fn _removeCollider(&mut self, collider_handle: ColliderHandle) {
        self.collider_set
            .remove(collider_handle, &mut self.island_manager, &mut self.rigid_body_set, true);
    }

    // remove object AND all its subitems if it's a rigidgroup
    // does nothing if the object is already removed
    pub fn stopTrackingObject(&mut self, object_id: ObjectId) {
        // shape data
        self.primitiveSettingsOfId.remove(&object_id);
        self.kinematicLatestVelocityOfObjId.remove(&object_id);

        // group
        if self.groupRigidBodyHandleOfObjId.contains_key(&object_id) {
            self.groupIsControlledByUsOfObjId.remove(&object_id);
            if let Some(rigid_body_handle) = self.groupRigidBodyHandleOfObjId.remove(&object_id) {
                self._removeRigidBody(rigid_body_handle);
            };
        }

        // standalone
        if self.standaloneRigidBodyHandleOfObjId.contains_key(&object_id) {
            self.standaloneColliderHandleOfObjId.remove(&object_id);
            self.standaloneIsPushableMetadataOfObjId.remove(&object_id);
            self.standaloneIsControlledByUsOfObjId.remove(&object_id);
            if let Some(rigid_body_handle) = self.standaloneRigidBodyHandleOfObjId.remove(&object_id) {
                self._removeRigidBody(rigid_body_handle);
            };
        }
        // subitem
        if self.subitemColliderHandleOfObjId.contains_key(&object_id) {
            // remove metadata
            let wasPushable = self.subitemIsPushableMetadataOfObjId.remove(&object_id).unwrap_or(true);
            let group_obj_id = self.groupObjIdOfSubitemObjId.remove(&object_id);

            // recalculate the body type based on the subitems
            if !wasPushable && let Some(group_obj_id) = group_obj_id {
                // optimization if statement
                if self._allGroupSubitemsArePushable(group_obj_id) {
                    if let Some(isControlledByUs) = self.groupIsControlledByUsOfObjId.get(&group_obj_id) {
                        self._setGroupBodyType(group_obj_id, true, *isControlledByUs);
                    }
                }
            }

            // remove the collider
            if let Some(collider_handle) = self.subitemColliderHandleOfObjId.remove(&object_id) {
                self._removeCollider(collider_handle);
            }
        }
    }

    fn _allGroupSubitemsArePushable(&self, group_obj_id: ObjectId) -> bool {
        let subitems = self.groupObjIdOfSubitemObjId.get_keys(&group_obj_id);
        let is_pushable = if let Some(subitems) = subitems {
            subitems
                .iter()
                .all(|subitem_id| *self.subitemIsPushableMetadataOfObjId.get(subitem_id).unwrap_or(&true))
        } else {
            true
        };
        is_pushable
    }

    // ---------------- modify and teleport objects ----------------

    // Moves the object over the next step instead of teleporting it, so rapier derives its velocity naturally.
    // Only kinematic rigidbodies have a target pose to move toward, so dynamic objects are teleported anyway.
    pub fn setObjectPhysicsInfo3D(&mut self, object_id: ObjectId, setPhysicsInfo3D: SetPhysicsInfo3D) {
        // if there is no rigid body (it's a subitem), just teleport it
        let Some(rigid_body_handle) = self._getRigidBodyHandle(object_id) else {
            self.forceObjectPhysicsInfo3D(object_id, setPhysicsInfo3D);
            return;
        };
        let Some(rigid_body) = self.rigid_body_set.get_mut(rigid_body_handle) else {
            self.forceObjectPhysicsInfo3D(object_id, setPhysicsInfo3D);
            return;
        };

        // if it's dynamic, just teleport it
        if rigid_body.is_dynamic() {
            self.forceObjectPhysicsInfo3D(object_id, setPhysicsInfo3D);
            return;
        }

        // otherwise if it's kinematic, move it.
        // position/orientation:
        // do it this way so writes arriving one field at a time in the same tick do not each undo the last
        let mut next_position = *getLatestRigidBodyPose(rigid_body);
        if let Some(position) = setPhysicsInfo3D.position {
            next_position.translation = position;
        }
        if let Some(rotation) = setPhysicsInfo3D.rotation {
            next_position.rotation = rotation;
        }
        setNextKinematicPose(rigid_body, next_position);

        // velocity/angvel:
        // just store the velocity instead of applying it. apply it in apply continued fn below
        if setPhysicsInfo3D.velocity.is_some() || setPhysicsInfo3D.angularVelocity.is_some() {
            let mut movement = self.kinematicLatestVelocityOfObjId.get(&object_id).cloned().unwrap_or(Movement3D {
                velocity: Vec3::ZERO,
                angularVelocity: Vec3::ZERO,
            });
            movement.velocity = setPhysicsInfo3D.velocity.unwrap_or(movement.velocity);
            movement.angularVelocity = setPhysicsInfo3D.angularVelocity.unwrap_or(movement.angularVelocity);
            self.kinematicLatestVelocityOfObjId.insert(object_id, movement);
        }
    }

    // keep kinematic objects moving according to the velocity we stored
    // always call this right before calling step()
    pub fn applyContinuedKinematicMovement(&mut self, dt_secs: f32) {
        // collected because resolving each object's rigidbody borrows self
        let latest_movement_of_object_id: Vec<(ObjectId, Movement3D)> = self
            .kinematicLatestVelocityOfObjId
            .iter()
            .map(|(object_id, movement)| (*object_id, movement.clone()))
            .collect();

        // for each kinematic object to move, move it
        for (object_id, movement) in latest_movement_of_object_id {
            // a still object has nothing to carry, and nothing else ever drops an entry for reaching zero
            if movement.velocity == Vec3::ZERO && movement.angularVelocity == Vec3::ZERO {
                self.kinematicLatestVelocityOfObjId.remove(&object_id);
                continue;
            }

            // a body someone else controls only goes where their updates put it, so it never coasts on a stored velocity
            let Some(true) = self.isControlledByUs(object_id) else {
                self.kinematicLatestVelocityOfObjId.remove(&object_id);
                continue;
            };

            let Some(rigid_body_handle) = self._getRigidBodyHandle(object_id) else {
                continue;
            };
            let Some(rigid_body) = self.rigid_body_set.get_mut(rigid_body_handle) else {
                continue;
            };

            if rigid_body.is_dynamic() {
                // it stopped being kinematic
                self.kinematicLatestVelocityOfObjId.remove(&object_id);
                continue;
            }

            let mut next_position = *getLatestRigidBodyPose(rigid_body);
            next_position.translation += movement.velocity * dt_secs;
            next_position.rotation = Quat::from_scaled_axis(movement.angularVelocity * dt_secs) * next_position.rotation;
            setNextKinematicPose(rigid_body, next_position);
        }
    }

    // discouraged as it is nonphysical:
    // teleports the item right now without setting a "next" position or moving it physically
    pub fn forceObjectPhysicsInfo3D(&mut self, object_id: ObjectId, setPhysicsInfo3D: SetPhysicsInfo3D) {
        fn updateRigidBodyPhysicsInfo3D(
            rigid_body: &mut RigidBody,
            object_id: ObjectId,
            setPhysicsInfo3D: SetPhysicsInfo3D,
            kinematicLatestVelocityOfObjId: &mut HashMap<ObjectId, Movement3D>,
        ) {
            let old_velocity = rigid_body.linvel();
            let old_angular_velocity = rigid_body.angvel();

            // A kinematic body's move waiting for the next step is where reads already see it, so the jump carries that move instead of dropping it.
            if rigid_body.is_kinematic() {
                let latest_pose = *rigid_body.next_position();
                rigid_body.set_position(latest_pose, true);
            }
            if let Some(position) = setPhysicsInfo3D.position {
                rigid_body.set_translation(position, true);
            }
            if let Some(rotation) = setPhysicsInfo3D.rotation {
                rigid_body.set_rotation(rotation, true);
            }
            if let Some(velocity) = setPhysicsInfo3D.velocity {
                rigid_body.set_linvel(velocity, true);
            }
            if let Some(angularVelocity) = setPhysicsInfo3D.angularVelocity {
                rigid_body.set_angvel(angularVelocity, true);
            }

            // for kinematic objects, set *next* kinematic position too, we need this so rapier doesnt create nonsensical giant velocity changes
            if rigid_body.is_dynamic() {
                return;
            }

            let translation = rigid_body.translation();
            let rotation = *rigid_body.rotation();
            setNextKinematicPose(rigid_body, Pose::from_parts(translation, rotation));

            // set the field in kinematicLatestVelocityOfObjId so it keeps moving with the appropriate velocity
            let movement = Movement3D {
                velocity: setPhysicsInfo3D.velocity.unwrap_or(old_velocity),
                angularVelocity: setPhysicsInfo3D.angularVelocity.unwrap_or(old_angular_velocity),
            };
            kinematicLatestVelocityOfObjId.insert(object_id, movement);
        }
        // group
        if let Some(rigid_body_handle) = self.groupRigidBodyHandleOfObjId.get(&object_id) {
            let Some(rigid_body) = self.rigid_body_set.get_mut(*rigid_body_handle) else {
                log::error!("forceObjectPhysicsInfo3D: Rigid body not found");
                return;
            };
            updateRigidBodyPhysicsInfo3D(rigid_body, object_id, setPhysicsInfo3D, &mut self.kinematicLatestVelocityOfObjId);
        }
        // standalone
        else if let Some(rigid_body_handle) = self.standaloneRigidBodyHandleOfObjId.get(&object_id) {
            let Some(rigid_body) = self.rigid_body_set.get_mut(*rigid_body_handle) else {
                log::error!("forceObjectPhysicsInfo3D: Rigid body not found");
                return;
            };
            updateRigidBodyPhysicsInfo3D(rigid_body, object_id, setPhysicsInfo3D, &mut self.kinematicLatestVelocityOfObjId);
        }
        // subitem
        else if let Some(collider_handle) = self.subitemColliderHandleOfObjId.get(&object_id) {
            let Some(collider) = self.collider_set.get_mut(*collider_handle) else {
                log::error!("forceObjectPhysicsInfo3D: Collider not found");
                return;
            };

            // get parent rigid body
            let Some(group_obj_id) = self.groupObjIdOfSubitemObjId.get(&object_id) else {
                log::error!("forceObjectPhysicsInfo3D: Rigid group not found");
                return;
            };
            let Some(rigid_body_handle) = self.groupRigidBodyHandleOfObjId.get(&group_obj_id) else {
                log::error!("forceObjectPhysicsInfo3D: Rigid body not found");
                return;
            };
            let Some(rigid_body) = self.rigid_body_set.get_mut(*rigid_body_handle) else {
                log::error!("forceObjectPhysicsInfo3D: Rigid body not found");
                return;
            };

            // get parent pos/rot
            let (parent_translation, parent_rotation) = {
                // Convert child writes using the same parent pose as child reads.
                let position = getLatestRigidBodyPose(rigid_body);
                let parent_translation = position.translation;
                let parent_rotation = position.rotation;
                (parent_translation, parent_rotation)
            };

            // set pos/rot
            if let Some(position) = setPhysicsInfo3D.position {
                let child_translation = position;
                let local_offset = child_translation - parent_translation;
                let local_translation = parent_rotation.inverse() * local_offset;
                collider.set_translation_wrt_parent(local_translation);
            }
            if let Some(rotation) = setPhysicsInfo3D.rotation {
                let child_rotation = rotation;
                let local_rotation = parent_rotation.inverse() * child_rotation;
                let Some(local_collider_isometry) = collider.position_wrt_parent() else {
                    log::error!("forceObjectPhysicsInfo3D: Collider has no parent rigid body");
                    return;
                };
                let local_translation = local_collider_isometry.translation;
                let local_isometry = Pose::from_parts(local_translation, local_rotation);
                collider.set_position_wrt_parent(local_isometry);
            }
            if setPhysicsInfo3D.velocity.is_some() || setPhysicsInfo3D.angularVelocity.is_some() {
                log::warn!(
                    "forceObjectPhysicsInfo3D: Velocity and angular velocity cannot be set for RigidGroup subitems because their parent body owns those values."
                );
            }
        }
    }

    pub fn setSubitemRelativeLocation3D(&mut self, objectId: ObjectId, relative_location_3d: Location3D) -> anyhow::Result<()> {
        let collider_handle = self
            .subitemColliderHandleOfObjId
            .get(&objectId)
            .ok_or_else(|| anyhow!("Subitem collider not found"))?;
        let collider = self.collider_set.get_mut(*collider_handle).ok_or_else(|| anyhow!("Collider not found"))?;

        let local_isometry = Self::_isometryOfLocation3D(&relative_location_3d);
        collider.set_position_wrt_parent(local_isometry);

        Ok(())
    }

    // ---- helpers for next pub fn ----
    // in global coords, not relative
    fn setSubitemLocation3D(&mut self, objectId: ObjectId, location3D: &Location3D) -> anyhow::Result<()> {
        let group_object_id = self
            .groupObjIdOfSubitemObjId
            .get(&objectId)
            .copied()
            .ok_or_else(|| anyhow!("Rigid group not found"))?;
        let relative_location = self.computeSubitemRelativeLocation3DToGroup(location3D, group_object_id)?;
        self.setSubitemRelativeLocation3D(objectId, relative_location)
    }

    fn setNextKinematicRigidBodyPhysicsInfo3D(&mut self, objectId: ObjectId, physicsInfo3D: &PhysicsInfo3D) -> anyhow::Result<()> {
        let rigid_body_handle = self._getRigidBodyHandle(objectId).ok_or_else(|| anyhow!("Rigid body not found"))?;
        let rigid_body = self.rigid_body_set.get_mut(rigid_body_handle).ok_or_else(|| anyhow!("Rigid body not found"))?;

        if rigid_body.is_dynamic() {
            return Err(anyhow!("Rigid body is dynamic"));
        }
        let PhysicsInfo3D { location, movement } = physicsInfo3D;

        let translation = location.position;
        let rotation = location.quaternion;
        let isometry = Pose::from_parts(translation, rotation);
        setNextKinematicPose(rigid_body, isometry);

        let linvel = movement.velocity;
        let angvel = movement.angularVelocity;
        let wake_up = true;
        rigid_body.set_linvel(linvel, wake_up);
        rigid_body.set_angvel(angvel, wake_up);

        // deliberately does not store the movement: the next snapshot says where this object goes, so integrating
        // the streamed velocity locally on top of that would be dead reckoning.
        Ok(())
    }

    // preferred over forcing the object's physics, applies the change smoothly to kinematics (when possible)
    pub fn setObjectNextPhysicsInfo3D(&mut self, objectId: ObjectId, physics_info_3d: PhysicsInfo3D) -> anyhow::Result<()> {
        // subitem (collider)
        let is_subitem = self.subitemColliderHandleOfObjId.contains_key(&objectId);
        if is_subitem {
            self.setSubitemLocation3D(objectId, &physics_info_3d.location)?;
            return Ok(());
        }

        // should always exist
        let Some(rigid_body_handle) = self._getRigidBodyHandle(objectId) else {
            return Err(anyhow!("setObjectNextPhysicsInfo3D: Object not found"));
        };
        let Some(rigid_body) = self.rigid_body_set.get(rigid_body_handle) else {
            return Err(anyhow!("setObjectNextPhysicsInfo3D: Rigid body not found"));
        };

        // dynamic
        if rigid_body.is_dynamic() {
            self.forceObjectPhysicsInfo3D(objectId, SetPhysicsInfo3D::from(physics_info_3d));
            return Ok(());
        }

        // kinematic or fixed
        self.setNextKinematicRigidBodyPhysicsInfo3D(objectId, &physics_info_3d)?;
        Ok(())
    }

    pub fn updateObjectToHavePhysicsToggles3D(&mut self, object_id: ObjectId, setPhysicsToggles3D: SetPhysicsToggles3D) {
        // group
        if self.groupRigidBodyHandleOfObjId.contains_key(&object_id) {
            // not allowed right now
        }
        // standalone
        else if let Some(rigid_body_handle) = self.standaloneRigidBodyHandleOfObjId.get(&object_id) {
            let is_awake = self.island_manager.active_bodies().any(|handle| handle == *rigid_body_handle);
            let Some(rigid_body) = self.rigid_body_set.get_mut(*rigid_body_handle) else {
                log::error!("updateObjectToHavePhysicsToggles3D: Rigid body not found");
                return;
            };
            let Some(&collider_handle) = self.standaloneColliderHandleOfObjId.get(&object_id) else {
                log::error!("updateObjectToHavePhysicsToggles3D: Collider not found");
                return;
            };
            let Some(collider) = self.collider_set.get_mut(collider_handle) else {
                log::error!("updateObjectToHavePhysicsToggles3D: Collider not found");
                return;
            };

            // update pushable metadata
            if let Some(isPushable) = setPhysicsToggles3D.isPushable {
                self.standaloneIsPushableMetadataOfObjId.insert(object_id, isPushable);
            }

            // walkthrough
            if let Some(isWalkthrough) = setPhysicsToggles3D.isWalkthrough {
                collider.set_sensor(isWalkthrough);
                collider.set_collision_groups(getWalkthroughCollisionGroups(isWalkthrough));
            }
            // density
            if let Some(density) = setPhysicsToggles3D.density {
                collider.set_density(density);
                rigid_body.recompute_mass_properties_from_colliders(&self.collider_set);
            }
            // pushable body type
            if let Some(isPushable) = setPhysicsToggles3D.isPushable {
                let Some(&isControlledByUs) = self.standaloneIsControlledByUsOfObjId.get(&object_id) else {
                    log::error!("updateObjectToHavePhysicsToggles3D: Standalone metadata not found");
                    return;
                };
                setBodyTypeKeepingKinematic(rigid_body, is_awake, Self::_getBodyType(isPushable, isControlledByUs));
            }
        }
        // subitem
        else if let Some(collider_handle) = self.subitemColliderHandleOfObjId.get(&object_id) {
            let Some(collider) = self.collider_set.get_mut(*collider_handle) else {
                log::error!("updateObjectToHavePhysicsToggles3D: Collider not found");
                return;
            };
            let Some(&group_obj_id) = self.groupObjIdOfSubitemObjId.get(&object_id) else {
                log::error!("updateObjectToHavePhysicsToggles3D: Rigid group not found");
                return;
            };
            let Some(subitemIsPushableMut) = self.subitemIsPushableMetadataOfObjId.get_mut(&object_id) else {
                log::error!("updateObjectToHavePhysicsToggles3D: Subitem metadata not found");
                return;
            };
            let Some(&rigid_body_handle) = self.groupRigidBodyHandleOfObjId.get(&group_obj_id) else {
                log::error!("updateObjectToHavePhysicsToggles3D: Rigid body not found");
                return;
            };
            let Some(rigid_body) = self.rigid_body_set.get_mut(rigid_body_handle) else {
                log::error!("updateObjectToHavePhysicsToggles3D: Rigid body not found");
                return;
            };

            // walkthrough
            if let Some(isWalkthrough) = setPhysicsToggles3D.isWalkthrough {
                collider.set_sensor(isWalkthrough);
                collider.set_collision_groups(getWalkthroughCollisionGroups(isWalkthrough));
            }
            // density
            if let Some(density) = setPhysicsToggles3D.density {
                collider.set_density(density);
                rigid_body.recompute_mass_properties_from_colliders(&self.collider_set);
            }
            // pushable body type and metadata
            if let Some(isPushable) = setPhysicsToggles3D.isPushable {
                *subitemIsPushableMut = isPushable;
                let is_group_pushable = self._allGroupSubitemsArePushable(group_obj_id);
                let Some(&isControlledByUs) = self.groupIsControlledByUsOfObjId.get(&group_obj_id) else {
                    log::error!("updateObjectToHavePhysicsToggles3D: Group metadata not found");
                    return;
                };
                self._setGroupBodyType(group_obj_id, is_group_pushable, isControlledByUs);
            }
        }
    }

    pub fn updateObjectIsControlledByUs(&mut self, object_id: ObjectId, setIsControlledByUs: bool) {
        // group
        if self.groupRigidBodyHandleOfObjId.contains_key(&object_id) {
            self.groupIsControlledByUsOfObjId.insert(object_id, setIsControlledByUs);

            // recompute pushable
            let isPushable = self._allGroupSubitemsArePushable(object_id);
            self._setGroupBodyType(object_id, isPushable, setIsControlledByUs);
            return;
        }
        // standalone
        else if self.standaloneRigidBodyHandleOfObjId.contains_key(&object_id) {
            self.standaloneIsControlledByUsOfObjId.insert(object_id, setIsControlledByUs);

            // recompute pushable
            let Some(&isPushable) = self.standaloneIsPushableMetadataOfObjId.get(&object_id) else {
                log::error!("updateObjectIsControlledByUs: Standalone metadata not found");
                return;
            };

            self._setStandaloneBodyType(object_id, isPushable, setIsControlledByUs);
            return;
        }
        // subitem
        else if self.subitemColliderHandleOfObjId.contains_key(&object_id) {
            log::error!("updateObjectIsControlledByUs: isControlledByUs can't be set for subitems because they defer to their parents for this setting.");
            return;
        }

        log::error!("updateObjectIsControlledByUs: Object not found");
    }

    // ---------------- control and kinematically position objects ----------------
    fn _getRigidBodyHandle(&self, object_id: ObjectId) -> Option<RigidBodyHandle> {
        // group
        if let Some(rigid_body_handle) = self.groupRigidBodyHandleOfObjId.get(&object_id) {
            return Some(*rigid_body_handle);
        }
        // standalone
        else if let Some(rigid_body_handle) = self.standaloneRigidBodyHandleOfObjId.get(&object_id) {
            return Some(*rigid_body_handle);
        }
        // subitem
        else {
            return None;
        }
    }

    // same as _getRigidBodyHandle, but a subitem defers to the rigidbody of the group that owns it
    fn _getAncestorRigidBodyHandle(&self, object_id: ObjectId) -> Option<RigidBodyHandle> {
        // group
        if let Some(rigid_body_handle) = self.groupRigidBodyHandleOfObjId.get(&object_id) {
            return Some(*rigid_body_handle);
        }
        // standalone
        else if let Some(rigid_body_handle) = self.standaloneRigidBodyHandleOfObjId.get(&object_id) {
            return Some(*rigid_body_handle);
        }
        // subitem
        else if let Some(group_obj_id) = self.groupObjIdOfSubitemObjId.get(&object_id) {
            return self.groupRigidBodyHandleOfObjId.get(group_obj_id).copied();
        } else {
            return None;
        }
    }

    // Every object on a body that isn't Fixed, which is every body that can move, and the parts of each such group, which move with it.
    pub fn getObjectIdsOnNonFixedBodies(&self) -> Vec<ObjectId> {
        let mut object_ids = Vec::new();
        for (rigid_body_handle, rigid_body) in self.rigid_body_set.iter() {
            if rigid_body.is_fixed() {
                continue;
            }
            let Some(object_id) = self._getObjectIdfRigidBodyHandle(&rigid_body_handle) else {
                continue;
            };
            object_ids.push(object_id);
            object_ids.extend(self.getRigidGroupSubitemIds(object_id));
        }
        object_ids
    }

    fn _getObjectIdfRigidBodyHandle(&self, rigid_body_handle: &RigidBodyHandle) -> Option<ObjectId> {
        // group
        if let Some(object_id) = self.groupRigidBodyHandleOfObjId.get_value(rigid_body_handle) {
            return Some(*object_id);
        }
        // standalone
        if let Some(object_id) = self.standaloneRigidBodyHandleOfObjId.get_value(rigid_body_handle) {
            return Some(*object_id);
        }
        // subitem
        None
    }

    fn _getObjectIdOfColliderHandle(&self, collider_handle: ColliderHandle) -> Option<ObjectId> {
        // standalone
        if let Some(object_id) = self.standaloneColliderHandleOfObjId.get_value(&collider_handle) {
            return Some(*object_id);
        }
        // subitem
        if let Some(object_id) = self.subitemColliderHandleOfObjId.get_value(&collider_handle) {
            return Some(*object_id);
        }
        // group
        None
    }

    // pub fn canSetNextKinematicLocation(&self, object_id: ObjectId) -> bool {
    //     let Some(rigid_body_handle) = self._getRigidBodyHandle(object_id) else {
    //         return false;
    //     };
    //     let Some(rigid_body) = self.rigid_body_set.get(rigid_body_handle) else {
    //         return false;
    //     };
    //     rigid_body.is_kinematic()
    // }

    // pub fn teleportObjectTo(&mut self, object_id: ObjectId, position: Vec3) {
    //     let Some(handle) = self.objectIdAndHandleLookup.getHandle(object_id) else {
    //         return;
    //     };
    //     let Some(rigid_body) = self.rigid_body_set.get_mut(*handle) else {
    //         return;
    //     };
    //     rigid_body.set_translation(vector![position.x, position.y, position.z], true);
    // }

    // ---------------- cast rays and shapes ----------------

    // whether shouldExcludeObject says the collider should be excluded
    fn _shouldExcludeHitObjectIsTrue<ShouldExcludeObjectFn>(
        &self,
        shouldExcludeHitObject: &ShouldExcludeObjectFn,
        collider_handle: ColliderHandle,
        collider: &Collider,
    ) -> bool
    where
        ShouldExcludeObjectFn: Fn(ObjectId) -> bool,
    {
        // if its parent should be ignored
        if let Some(parent_rigid_body_handle) = collider.parent() {
            if let Some(parent_object_id) = self._getObjectIdfRigidBodyHandle(&parent_rigid_body_handle) {
                if shouldExcludeHitObject(parent_object_id) {
                    return true;
                }
            };
        }
        // if it should be ignored
        if let Some(hit_object_id) = self._getObjectIdOfColliderHandle(collider_handle) {
            if shouldExcludeHitObject(hit_object_id) {
                return true;
            }
        };
        false
    }

    // general castRay function - always returns a ball, brick, or cylinder
    pub fn castRay<ShouldExcludeObjectFn>(
        &self,
        ray_origin: Vec3,
        ray_direction: Vec3,
        max_num_ray_steps: f32,
        should_include_walkthrough: bool,
        shouldExcludeHitObject: &ShouldExcludeObjectFn,
    ) -> Option<CastHit>
    where
        ShouldExcludeObjectFn: Fn(ObjectId) -> bool,
    {
        // create a fn that filters colliders
        let collider_filter =
            |collider_handle: ColliderHandle, collider: &Collider| !self._shouldExcludeHitObjectIsTrue(shouldExcludeHitObject, collider_handle, collider);
        // create the filter
        let filter = if should_include_walkthrough {
            QueryFilter::default().predicate(&collider_filter)
        } else {
            QueryFilter::default().exclude_sensors().predicate(&collider_filter)
        };
        let query_pipeline = self
            .broad_phase
            .as_query_pipeline(self.narrow_phase.query_dispatcher(), &self.rigid_body_set, &self.collider_set, filter);

        // finally, cast the ray
        let ray = Ray::new(
            Vec3::new(ray_origin.x, ray_origin.y, ray_origin.z),
            Vec3::new(ray_direction.x, ray_direction.y, ray_direction.z),
        );
        let (hit_collider, hit) = query_pipeline.cast_ray_and_get_normal(&ray, max_num_ray_steps, true)?;
        let hit_object_id = self._getObjectIdOfColliderHandle(hit_collider)?;

        Some(CastHit {
            object_id: hit_object_id,
            normal: hit.normal,
            distance: hit.time_of_impact * ray_direction.length(),
            position: ray_origin + ray_direction * hit.time_of_impact,
        })
    }

    // Sweep a ball through solid objects, using the same object exclusions as ray casts.
    pub fn castBall<ShouldExcludeObjectFn>(
        &self,
        origin: Vec3,
        translation: Vec3,
        radius: f32,
        shouldExcludeHitObject: &ShouldExcludeObjectFn,
    ) -> Option<(ObjectId, f32)>
    where
        ShouldExcludeObjectFn: Fn(ObjectId) -> bool,
    {
        let collider_filter =
            |collider_handle: ColliderHandle, collider: &Collider| !self._shouldExcludeHitObjectIsTrue(shouldExcludeHitObject, collider_handle, collider);
        let filter = QueryFilter::default().exclude_sensors().predicate(&collider_filter);
        let query_pipeline = self
            .broad_phase
            .as_query_pipeline(self.narrow_phase.query_dispatcher(), &self.rigid_body_set, &self.collider_set, filter);
        let options = rapier3d::parry::query::ShapeCastOptions {
            max_time_of_impact: 1.0,
            stop_at_penetration: true,
            ..Default::default()
        };
        let (hit_collider, hit) = query_pipeline.cast_shape(&Pose::from_parts(origin, Quat::IDENTITY), translation, &Ball::new(radius), options)?;
        let hit_object_id = self._getObjectIdOfColliderHandle(hit_collider)?;
        Some((hit_object_id, hit.time_of_impact * translation.length()))
    }

    // see if physically colliding with ground right now
    fn getGroundContactHit(&self, object_id: ObjectId, min_normal_y: f32) -> Option<ContactHit> {
        let (rigid_body_handle, collider_handles) = if let Some(group_rigid_body_handle) = self.groupRigidBodyHandleOfObjId.get(&object_id) {
            let rigid_body = self.rigid_body_set.get(*group_rigid_body_handle)?;
            (*group_rigid_body_handle, rigid_body.colliders().to_vec())
        } else if let Some(standalone_rigid_body_handle) = self.standaloneRigidBodyHandleOfObjId.get(&object_id) {
            let collider_handle = *self.standaloneColliderHandleOfObjId.get(&object_id)?;
            (*standalone_rigid_body_handle, vec![collider_handle])
        } else if let Some(subitem_collider_handle) = self.subitemColliderHandleOfObjId.get(&object_id) {
            let group_object_id = *self.groupObjIdOfSubitemObjId.get(&object_id)?;
            let group_rigid_body_handle = *self.groupRigidBodyHandleOfObjId.get(&group_object_id)?;
            (group_rigid_body_handle, vec![*subitem_collider_handle])
        } else {
            return None;
        };

        let mut best_contact_object_id = None;
        let mut best_contact_position = Vec3::ZERO;
        let mut best_normal_y = f32::NEG_INFINITY;
        let mut summed_contact_normal = Vec3::ZERO;

        // Build one support normal from every walkable active contact so ramp transitions do not keep snapping back to the flatter surface.
        for collider_handle in collider_handles {
            for contact_pair in self.narrow_phase.contact_pairs_with(collider_handle) {
                if !contact_pair.has_any_active_contact() {
                    continue;
                }

                let other_collider_handle = if contact_pair.collider1 == collider_handle {
                    contact_pair.collider2
                } else {
                    contact_pair.collider1
                };
                let Some(other_collider) = self.collider_set.get(other_collider_handle) else {
                    continue;
                };
                if other_collider.parent() == Some(rigid_body_handle) {
                    continue;
                }
                let Some(other_object_id) = self._getObjectIdOfColliderHandle(other_collider_handle) else {
                    continue;
                };

                for manifold in &contact_pair.manifolds {
                    if manifold.data.num_active_contacts() == 0 {
                        continue;
                    }

                    // Rapier's manifold normal points from collider1 toward collider2.
                    // For support checks we want the normal that points away from the other collider and into this object.
                    let contact_normal = if contact_pair.collider1 == collider_handle {
                        -manifold.data.normal
                    } else {
                        manifold.data.normal
                    };
                    if contact_normal.y < min_normal_y {
                        continue;
                    }

                    let contact_weight = manifold.data.num_active_contacts() as f32;
                    summed_contact_normal += contact_normal * contact_weight;

                    if contact_normal.y <= best_normal_y {
                        continue;
                    }

                    best_normal_y = contact_normal.y;
                    best_contact_object_id = Some(other_object_id);

                    // the middle of this manifold's contact points, on the other collider's surface
                    let is_other_collider2 = contact_pair.collider1 == collider_handle;
                    let other_subshape_pose = if is_other_collider2 {
                        manifold.subshape_pos2()
                    } else {
                        manifold.subshape_pos1()
                    }
                    .prepend_to(other_collider.position());
                    let summed_contact_position: Vec3 = manifold
                        .contacts()
                        .iter()
                        .map(|contact| other_subshape_pose * if is_other_collider2 { contact.local_p2 } else { contact.local_p1 })
                        .sum();
                    best_contact_position = summed_contact_position / manifold.contacts().len() as f32;
                }
            }
        }

        let best_contact_object_id = best_contact_object_id?;
        if summed_contact_normal.length_squared() <= 1e-6 {
            return None;
        }

        Some(ContactHit {
            object_id: best_contact_object_id,
            normal: summed_contact_normal.normalize(),
            position: best_contact_position,
        })
    }

    fn getCastShapeAndRigidBodyHandle(&self, object_id: ObjectId, should_exclude_cast_sensors: bool) -> Option<(RigidBodyHandle, SharedShape, Pose)> {
        // group
        if let Some(group_rigid_body_handle) = self.groupRigidBodyHandleOfObjId.get(&object_id) {
            let group_shape = self.buildGroupShape(object_id, should_exclude_cast_sensors)?;
            return Some((*group_rigid_body_handle, group_shape, Pose::identity()));
        }

        // standalone
        if let Some(standalone_rigid_body_handle) = self.standaloneRigidBodyHandleOfObjId.get(&object_id) {
            let (standalone_shape, standalone_shape_position) = self.buildStandaloneShape(object_id, should_exclude_cast_sensors)?;
            return Some((*standalone_rigid_body_handle, standalone_shape, standalone_shape_position));
        }

        // subitem
        if self.subitemColliderHandleOfObjId.contains_key(&object_id) {
            let group_object_id = *self.groupObjIdOfSubitemObjId.get(&object_id)?;
            let group_rigid_body_handle = *self.groupRigidBodyHandleOfObjId.get(&group_object_id)?;
            let (subitem_shape, subitem_shape_position) = self.buildSubitemShape(object_id, should_exclude_cast_sensors)?;
            return Some((group_rigid_body_handle, subitem_shape, subitem_shape_position));
        }

        log::warn!("castShape: Reached unexpected state.");
        None
    }

    // pub fn castShape(&self, object_id: ObjectId, ray_direction: Vec3, max_num_ray_steps: f32, exclude_group_sensors: bool) -> Option<ObjectId> {
    //     if ray_direction.length() <= f32::EPSILON {
    //         return None;
    //     }

    //     let shape_cast_options = rapier3d::parry::query::ShapeCastOptions::with_max_time_of_impact(max_num_ray_steps);
    //     let (exclude_rigid_body_handle, shape) = self.getCastShapeAndRigidBodyHandle(object_id, exclude_group_sensors)?;
    //     let filter = QueryFilter::default().exclude_sensors().exclude_rigid_body(exclude_rigid_body_handle);
    //     let query_pipeline = self
    //         .broad_phase
    //         .as_query_pipeline(self.narrow_phase.query_dispatcher(), &self.rigid_body_set, &self.collider_set, filter);
    //     let rigid_body = self.rigid_body_set.get(exclude_rigid_body_handle)?;
    //     let shape_cast_velocity = vector![ray_direction.x, ray_direction.y, ray_direction.z];

    //     let (hit_collider_handle, _) = query_pipeline.cast_shape(rigid_body.position(), &shape_cast_velocity, shape.as_ref(), shape_cast_options)?;
    //     self._getObjectIdOfColliderHandle(hit_collider_handle)
    // }

    // Slides the object's shape from its position plus shape_start_offset along cast_direction, up to max_distance, with the
    // same object filters as castRay. The object itself is never hit.
    pub fn castObjectShapeInDirection<ShouldExcludeObjectFn>(
        &self,
        object_id: ObjectId,
        shape_start_offset: Vec3,
        cast_direction: Vec3,
        max_distance: f32,
        should_include_walkthrough: bool,
        shouldExcludeHitObject: &ShouldExcludeObjectFn,
    ) -> Option<CastHit>
    where
        ShouldExcludeObjectFn: Fn(ObjectId) -> bool,
    {
        let unit_cast_direction = cast_direction.try_normalize()?;
        let shape_cast_options = rapier3d::parry::query::ShapeCastOptions {
            max_time_of_impact: max_distance,
            target_distance: 0.0,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        let (exclude_rigid_body_handle, shape, shape_position_wrt_rigid_body) = self.getCastShapeAndRigidBodyHandle(object_id, true)?;
        let collider_filter =
            |collider_handle: ColliderHandle, collider: &Collider| !self._shouldExcludeHitObjectIsTrue(shouldExcludeHitObject, collider_handle, collider);
        let filter = if should_include_walkthrough {
            QueryFilter::default()
        } else {
            QueryFilter::default().exclude_sensors()
        }
        .exclude_rigid_body(exclude_rigid_body_handle)
        .predicate(&collider_filter);
        let query_pipeline = self
            .broad_phase
            .as_query_pipeline(self.narrow_phase.query_dispatcher(), &self.rigid_body_set, &self.collider_set, filter);
        let rigid_body = self.rigid_body_set.get(exclude_rigid_body_handle)?;
        // we don't use getLatestRigidBodyPose here - Collision queries use the simulated pose, matching the colliders in the query pipeline.
        let shape_position = rigid_body.position() * shape_position_wrt_rigid_body;
        let start_translation = shape_position.translation + shape_start_offset;
        let start_isometry = Pose::from_parts(start_translation, shape_position.rotation);

        // the direction is normalized, so the time of impact is the distance
        let (hit_collider_handle, hit) = query_pipeline.cast_shape(&start_isometry, unit_cast_direction, shape.as_ref(), shape_cast_options)?;
        let hit_object_id = self._getObjectIdOfColliderHandle(hit_collider_handle)?;

        Some(CastHit {
            object_id: hit_object_id,
            normal: hit.normal1,
            distance: hit.time_of_impact,
            position: hit.witness1,
        })
    }

    // What the object is standing on, if anything walkable is close enough below it. Ground tilted more than
    // max_ground_slope_degrees away from flat doesn't count.
    pub fn getGroundSupport(&self, object_id: ObjectId, max_distance_to_ground: f32, max_ground_slope_degrees: f32) -> Option<CastHit> {
        let min_ground_normal_y = max_ground_slope_degrees.to_radians().cos();

        // contact hit - check if already in contact with ground
        if let Some(ContactHit {
            object_id: ground_object_id,
            normal: ground_normal,
            position: ground_position,
        }) = self.getGroundContactHit(object_id, min_ground_normal_y)
        {
            return Some(CastHit {
                object_id: ground_object_id,
                normal: ground_normal,
                distance: 0.0,
                position: ground_position,
            });
        }

        // cast hit - cast object id downward
        let cast_hit = self.castObjectShapeInDirection(object_id, Vec3::ZERO, Vec3::NEG_Y, max_distance_to_ground, false, &|_| false)?;
        if cast_hit.normal.y < min_ground_normal_y {
            return None;
        }
        Some(cast_hit)
    }

    // helper for shape cast
    fn buildGroupShape(&self, group_object_id: ObjectId, exclude_group_sensors: bool) -> Option<SharedShape> {
        let subitem_object_ids = self.groupObjIdOfSubitemObjId.get_keys(&group_object_id)?;
        let mut local_shape_of_part_index: Vec<(Pose, SharedShape)> = Vec::new();
        for subitem_object_id in subitem_object_ids {
            let Some(collider_handle) = self.subitemColliderHandleOfObjId.get(subitem_object_id) else {
                continue;
            };
            let Some(collider) = self.collider_set.get(*collider_handle) else {
                continue;
            };
            if exclude_group_sensors && collider.is_sensor() {
                continue;
            }

            let Some(local_shape_position) = collider.position_wrt_parent() else {
                continue;
            };

            // Parry panics if we put a compound shape inside another compound shape, so flatten compound colliders here.
            if let Some(compound_shape) = collider.shape().as_compound() {
                for (compound_child_position, compound_child_shape) in compound_shape.shapes() {
                    local_shape_of_part_index.push((*local_shape_position * *compound_child_position, compound_child_shape.clone()));
                }
            } else {
                local_shape_of_part_index.push((*local_shape_position, collider.shared_shape().clone()));
            }
        }
        if local_shape_of_part_index.is_empty() {
            return None;
        }
        let group_shape = SharedShape::compound(local_shape_of_part_index);
        Some(group_shape)
    }

    // helper for shape cast
    fn buildStandaloneShape(&self, standalone_object_id: ObjectId, exclude_standalone_sensors: bool) -> Option<(SharedShape, Pose)> {
        let standalone_collider_handle = self.standaloneColliderHandleOfObjId.get(&standalone_object_id)?;
        let standalone_collider = self.collider_set.get(*standalone_collider_handle)?;
        if exclude_standalone_sensors && standalone_collider.is_sensor() {
            return None;
        }

        let local_shape_position = *standalone_collider.position_wrt_parent()?;
        Some((standalone_collider.shared_shape().clone(), local_shape_position))
    }

    // helper for shape cast
    fn buildSubitemShape(&self, subitem_object_id: ObjectId, exclude_subitem_sensors: bool) -> Option<(SharedShape, Pose)> {
        let subitem_collider_handle = self.subitemColliderHandleOfObjId.get(&subitem_object_id)?;
        let subitem_collider = self.collider_set.get(*subitem_collider_handle)?;
        if exclude_subitem_sensors && subitem_collider.is_sensor() {
            return None;
        }

        let local_shape_position = *subitem_collider.position_wrt_parent()?;
        Some((subitem_collider.shared_shape().clone(), local_shape_position))
    }

    // // Placement diagnostics check geometry, including walkthrough objects, independently of gameplay collisions.
    // pub fn getOverlapWarnings(&self, max_signed_distance: f32) -> Vec<(ObjectId, ObjectId, f32)> {
    //     let mut overlap_warnings = Vec::new();
    //     let mut add_overlap_warning = |collider_handle_1, collider_handle_2, signed_distance| {
    //         if signed_distance >= max_signed_distance {
    //             return;
    //         }
    //         let Some(object_id_1) = self._getObjectIdOfColliderHandle(collider_handle_1) else {
    //             return;
    //         };
    //         let Some(object_id_2) = self._getObjectIdOfColliderHandle(collider_handle_2) else {
    //             return;
    //         };
    //         if !self.primitiveSettingsOfId.contains_key(&object_id_1) || !self.primitiveSettingsOfId.contains_key(&object_id_2) {
    //             return;
    //         }

    //         let (warning_object_id_1, warning_object_id_2) = if object_id_1 <= object_id_2 {
    //             (object_id_1, object_id_2)
    //         } else {
    //             (object_id_2, object_id_1)
    //         };
    //         overlap_warnings.push((warning_object_id_1, warning_object_id_2, signed_distance));
    //     };

    //     // Use Rapier's maintained contact graph instead of rebuilding shapes and scanning every primitive pair ourselves.
    //     for contact_pair in self.narrow_phase.contact_pairs() {
    //         if let Some((_, contact)) = contact_pair.find_deepest_contact() {
    //             add_overlap_warning(contact_pair.collider1, contact_pair.collider2, contact.dist);
    //         }
    //     }

    //     // Walkthrough colliders have intersection pairs instead of contact manifolds, so measure their overlap directly.
    //     for (collider_handle_1, collider_handle_2, is_intersecting) in self.narrow_phase.intersection_pairs() {
    //         if !is_intersecting {
    //             continue;
    //         }
    //         let collider_1 = &self.collider_set[collider_handle_1];
    //         let collider_2 = &self.collider_set[collider_handle_2];
    //         match rapier3d::parry::query::contact(collider_1.position(), collider_1.shape(), collider_2.position(), collider_2.shape(), 0.0) {
    //             Ok(Some(contact)) => add_overlap_warning(collider_handle_1, collider_handle_2, contact.dist),
    //             Ok(None) => {}
    //             Err(error) => log::error!("getOverlapWarnings: Cannot query colliders {collider_handle_1:?} and {collider_handle_2:?}: {error}"),
    //         }
    //     }

    //     overlap_warnings.sort_unstable_by_key(|(object_id_1, object_id_2, _)| (*object_id_1, *object_id_2));
    //     overlap_warnings
    // }

    // // A welded assembly shares geometric support across all its parts, including walkthrough parts; empty groups need none.
    // pub fn getFloatingPlacementWarnings(&self, contact_tolerance: f32) -> Vec<ObjectId> {
    //     let mut floating_object_ids = Vec::new();
    //     for (rigid_body_handle, rigid_body) in self.rigid_body_set.iter() {
    //         // Nested groups also have empty bodies because their colliders belong to the outermost group.
    //         if rigid_body.colliders().is_empty() {
    //             continue;
    //         }
    //         let Some(object_id) = self
    //             .groupRigidBodyHandleOfObjId
    //             .get_value(&rigid_body_handle)
    //             .or_else(|| self.standaloneRigidBodyHandleOfObjId.get_value(&rigid_body_handle))
    //         else {
    //             continue;
    //         };
    //         let filter = QueryFilter::default().exclude_rigid_body(rigid_body_handle);
    //         let query_pipeline = self
    //             .broad_phase
    //             .as_query_pipeline(self.narrow_phase.query_dispatcher(), &self.rigid_body_set, &self.collider_set, filter);
    //         let mut is_supported = false;

    //         // Floating warnings care about support below the object, not whether it touches anything at all.
    //         // Check nearby shapes directly so exact contact and slight penetration have reliable normals.
    //         for collider_handle in rigid_body.colliders() {
    //             let collider = &self.collider_set[*collider_handle];
    //             // Probe one tolerance downward so touching curved surfaces yield usable support normals.
    //             // Limit the offset for thin parts so the probe cannot pass through their support.
    //             let collider_bounds = collider.compute_aabb();
    //             let support_offset = contact_tolerance.min(collider_bounds.half_extents().y);
    //             let mut support_position = *collider.position();
    //             support_position.translation.y -= support_offset;
    //             for (other_collider_handle, other_collider) in query_pipeline.intersect_aabb_conservative(collider_bounds.loosened(contact_tolerance)) {
    //                 match rapier3d::parry::query::contact(
    //                     &support_position,
    //                     collider.shape(),
    //                     other_collider.position(),
    //                     other_collider.shape(),
    //                     contact_tolerance - support_offset,
    //                 ) {
    //                     Ok(Some(contact)) if contact.normal2.y > f32::EPSILON => {
    //                         is_supported = true;
    //                         break;
    //                     }
    //                     Ok(_) => {}
    //                     Err(error) => {
    //                         log::error!("getFloatingPlacementWarnings: Cannot query colliders {collider_handle:?} and {other_collider_handle:?}: {error}")
    //                     }
    //                 }
    //             }
    //             if is_supported {
    //                 break;
    //             }
    //         }
    //         if !is_supported {
    //             floating_object_ids.push(*object_id);
    //         }
    //     }
    //     floating_object_ids.sort_unstable();
    //     floating_object_ids
    // }

    // fn shapeAndPoseOfObject(&self, object_id: ObjectId, exclude_sensors: bool) -> Option<(Pose, SharedShape)> {
    //     if let Some(group_rigid_body_handle) = self.groupRigidBodyHandleOfObjId.get(&object_id) {
    //         let group_rigid_body = self.rigid_body_set.get(*group_rigid_body_handle)?;
    //         let shape = self.buildGroupShape(object_id, exclude_sensors)?;
    //         return Some((*group_rigid_body.position(), shape));
    //     }

    //     if let Some(standalone_rigid_body_handle) = self.standaloneRigidBodyHandleOfObjId.get(&object_id) {
    //         let standalone_rigid_body = self.rigid_body_set.get(*standalone_rigid_body_handle)?;
    //         let shape = self.buildStandaloneShape(object_id, exclude_sensors)?;
    //         return Some((*standalone_rigid_body.position(), shape));
    //     }

    //     if self.subitemColliderHandleOfObjId.contains_key(&object_id) {
    //         let group_object_id = *self.groupObjIdOfSubitemObjId.get(&object_id)?;
    //         let group_rigid_body_handle = *self.groupRigidBodyHandleOfObjId.get(&group_object_id)?;
    //         let group_rigid_body = self.rigid_body_set.get(group_rigid_body_handle)?;
    //         let shape = self.buildSubitemShape(object_id, exclude_sensors)?;
    //         return Some((*group_rigid_body.position(), shape));
    //     }

    //     None
    // }

    // pub fn getSignedDistanceBetweenObjects(&self, object_id_1: ObjectId, object_id_2: ObjectId, prediction_distance: f32) -> Option<f32> {
    //     let (shape_pose_1, shape_1) = self.shapeAndPoseOfObject(object_id_1, true)?;
    //     let (shape_pose_2, shape_2) = self.shapeAndPoseOfObject(object_id_2, true)?;
    //     let contact = rapier3d::parry::query::contact(&shape_pose_1, shape_1.as_ref(), &shape_pose_2, shape_2.as_ref(), prediction_distance).ok()??;
    //     Some(contact.dist)
    // }
}
