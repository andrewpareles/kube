use crate::collect_animation_content::{collect_pivot_offsets, collect_property_overrides, collect_render_sk_animations};
use crate::computePropertyValue::ComputedPropertyOverrides;
use crate::label_text_mesh::label_meshes_of_label_infos;
use crate::renderer_types::{
    RenderBall, RenderBrick, RenderCylinder, RenderLabel, RenderLabelContent, RenderMesh, RenderObject, RenderSkKeyframe, RenderSkeleton, RenderSkeletonInfo,
    RenderTransform, SkJointToFollow,
};
use game_types::{
    object_types::{ClockState, LabelInfo, LiveObject, Location3D, ObjectId, ObjectInfo, ObjectLocation, PhysicsInfo3D, PropertyKeyframeInfo},
    physics_primitive_types::PrimitiveSettings,
};
use glam::Vec3;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub struct RenderSnapshot {
    pub render_object_of_id: HashMap<ObjectId, RenderObject>,
    pub property_overrides_of_id: HashMap<ObjectId, ComputedPropertyOverrides>,
    pub rotation_base_degrees_of_id: HashMap<ObjectId, Vec3>, // the axes nobody animates, so the renderer never decomposes animated rotation either
    pub pivot_offset_of_id: HashMap<ObjectId, Vec3>,          // where each object with a Pivot turns, in its own coordinates
}

// What render objects are built from. The live world and stored worlds each answer from their own data.
pub trait Renderable {
    fn object_of_id(&self) -> &HashMap<ObjectId, LiveObject>;
    fn parent_of_id(&self) -> &HashMap<ObjectId, ObjectId>;
    fn primitive_settings_of_id(&self) -> &HashMap<ObjectId, PrimitiveSettings>;
    fn skeleton_info_arc_of_id(&self) -> &HashMap<ObjectId, Arc<RenderSkeletonInfo>>;
    fn sk_keyframes_of_animation_id(&self) -> &HashMap<ObjectId, Vec<RenderSkKeyframe>>;
    fn property_keyframes_arc_of_animation_id(&self) -> &HashMap<ObjectId, Arc<Vec<PropertyKeyframeInfo>>>;
    fn rotation_base_degrees_of_id(&self) -> HashMap<ObjectId, Vec3>;
    fn render_object_kind_lists(&self) -> RenderObjectKindLists;
    fn physics_info(&self, object_id: ObjectId) -> Option<PhysicsInfo3D>;
    fn rigid_group_to_follow(&self, object_id: ObjectId) -> Option<(ObjectId, Location3D)>;
    fn teleport_count(&self, object_id: ObjectId) -> u64;

    // Builds every object's render object from scratch.
    fn getRenderSnapshot(&self) -> RenderSnapshot {
        let kind_lists = self.render_object_kind_lists();
        let mut render_object_of_id = HashMap::with_capacity(self.object_of_id().len());
        update_render_objects(self, &mut render_object_of_id, self.object_of_id().keys().copied(), &kind_lists);
        create_render_snapshot_of_render_objects(self, render_object_of_id, kind_lists)
    }
}

// The objects of each kind a snapshot has to find without looking through every object. A stored world looks through every
// object for them, and the live world reads its lists of each kind. Each list is sorted, so the same world always gives the same snapshot.
pub struct RenderObjectKindLists {
    pub sk_animator_ids: Vec<ObjectId>,
    pub property_animator_ids: Vec<ObjectId>,
    pub tween_ids: Vec<ObjectId>,
    pub sk_attachment_ids: Vec<ObjectId>,
    pub pivot_ids: Vec<ObjectId>,
    pub label_ids: Vec<ObjectId>,
    pub skeleton_ids: Vec<ObjectId>,
}

impl RenderObjectKindLists {
    pub fn sorted(mut self) -> Self {
        for object_ids in [
            &mut self.sk_animator_ids,
            &mut self.property_animator_ids,
            &mut self.tween_ids,
            &mut self.sk_attachment_ids,
            &mut self.pivot_ids,
            &mut self.label_ids,
            &mut self.skeleton_ids,
        ] {
            object_ids.sort_unstable();
        }
        self
    }
}

pub fn scan_render_object_kind_lists(object_of_id: &HashMap<ObjectId, LiveObject>) -> RenderObjectKindLists {
    let mut kind_lists = RenderObjectKindLists {
        sk_animator_ids: Vec::new(),
        property_animator_ids: Vec::new(),
        tween_ids: Vec::new(),
        sk_attachment_ids: Vec::new(),
        pivot_ids: Vec::new(),
        label_ids: Vec::new(),
        skeleton_ids: Vec::new(),
    };
    for (&object_id, object) in object_of_id {
        match &object.info {
            ObjectInfo::SkAnimator(_) => kind_lists.sk_animator_ids.push(object_id),
            ObjectInfo::PropertyAnimator(_) => kind_lists.property_animator_ids.push(object_id),
            ObjectInfo::Tween(_) => kind_lists.tween_ids.push(object_id),
            ObjectInfo::SkAttachment(_) => kind_lists.sk_attachment_ids.push(object_id),
            ObjectInfo::Pivot(_) => kind_lists.pivot_ids.push(object_id),
            ObjectInfo::Label(_) => kind_lists.label_ids.push(object_id),
            ObjectInfo::Skeleton(_) => kind_lists.skeleton_ids.push(object_id),
            _ => {}
        }
    }
    kind_lists.sorted()
}

// Builds the render object of each given object into render_object_of_id, or takes it out of there when the object has none now.
// Text labels are laid out together after the rest, since they share one atlas of letters.
fn update_render_objects<Source: Renderable + ?Sized>(
    source: &Source,
    render_object_of_id: &mut HashMap<ObjectId, RenderObject>,
    object_ids: impl IntoIterator<Item = ObjectId>,
    kind_lists: &RenderObjectKindLists,
) {
    let object_of_id = source.object_of_id();
    let get_physics_info = |object_id| source.physics_info(object_id);

    // Resolve animator and attachment references once, before constructing their target objects.
    let mut sk_animator_ids_of_skeleton_id: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for &animator_id in &kind_lists.sk_animator_ids {
        let Some(ObjectInfo::SkAnimator(animator)) = object_of_id.get(&animator_id).map(|object| &object.info) else {
            continue;
        };
        if let Some(skeleton_id) = animator.skeletonId.map(ObjectId::from) {
            sk_animator_ids_of_skeleton_id.entry(skeleton_id).or_default().push(animator_id);
        }
    }
    let mut attachment_id_of_object_id = HashMap::new();
    for &attachment_id in &kind_lists.sk_attachment_ids {
        let Some(ObjectInfo::SkAttachment(attachment)) = object_of_id.get(&attachment_id).map(|object| &object.info) else {
            continue;
        };
        if let Some(id) = attachment.objectId.map(ObjectId::from) {
            // Physics uses the same lowest-id attachment when multiple attachments point to one object.
            attachment_id_of_object_id
                .entry(id)
                .and_modify(|lowest_attachment_id: &mut ObjectId| *lowest_attachment_id = (*lowest_attachment_id).min(attachment_id))
                .or_insert(attachment_id);
        }
    }

    // Construct transforms here so both sources resolve skeleton attachments identically.
    let get_transform = |object_id| {
        let physics_info = get_physics_info(object_id)?;
        let sk_joint_to_follow_arc = attachment_id_of_object_id.get(&object_id).and_then(|attachment_id| {
            let ObjectInfo::SkAttachment(attachment) = &object_of_id[attachment_id].info else {
                return None;
            };
            let mut current_id = object_id;
            let mut highest_group_id = None;
            while let Some(parent_id) = source.parent_of_id().get(&current_id) {
                current_id = *parent_id;
                let parent = object_of_id.get(parent_id)?;
                if let ObjectInfo::RigidGroup(_) = &parent.info {
                    highest_group_id = Some(*parent_id);
                }
            }
            let group_id = highest_group_id?;
            let ObjectInfo::RigidGroup(group) = &object_of_id.get(&group_id)?.info else {
                return None;
            };
            let skeleton_id = group.followSkeletonId.map(ObjectId::from)?;
            get_physics_info(group_id)?;
            Some(Arc::new(SkJointToFollow {
                skeleton_id,
                joint_name: attachment.jointName.clone(),
                offset_from_joint: attachment.offsetFromJoint.clone(),
                follower_group_id: group_id,
            }))
        });
        Some(RenderTransform {
            physicsInfo3D: physics_info,
            teleport_count: source.teleport_count(object_id),
            rigid_group_to_follow: source.rigid_group_to_follow(object_id),
            sk_joint_to_follow_arc,
        })
    };

    let create_render_object = |object_id: ObjectId, object: &LiveObject| -> Option<RenderObject> {
        match &object.info {
            ObjectInfo::Material(material_info) => Some(RenderObject::Material(material_info.clone())),
            ObjectInfo::WorldLighting(world_lighting_info) => Some(RenderObject::WorldLighting(*world_lighting_info)),
            ObjectInfo::SkJoint(_) => None,
            ObjectInfo::Skeleton(_) => source.skeleton_info_arc_of_id().get(&object_id).map(|skeleton_info_arc| {
                RenderObject::Skeleton(RenderSkeleton {
                    renderSkeletonInfoArc: Arc::clone(skeleton_info_arc),
                    render_sk_animations: Arc::new(collect_render_sk_animations(
                        object_of_id,
                        object_id,
                        sk_animator_ids_of_skeleton_id.get(&object_id).into_iter().flatten().copied(),
                        source.sk_keyframes_of_animation_id(),
                    )),
                })
            }),
            ObjectInfo::Ball(ball_info) => {
                let Some(PrimitiveSettings::Ball(primitive)) = source.primitive_settings_of_id().get(&object_id) else {
                    return None;
                };
                let Some(transform) = get_transform(object_id) else {
                    return None;
                };
                Some(RenderObject::Ball(RenderBall {
                    transform,
                    visualSettings: ball_info.visualSettings.clone(),
                    material: ball_info.material,
                    primitive: primitive.clone(),
                }))
            }
            ObjectInfo::Brick(brick_info) => {
                let Some(PrimitiveSettings::Brick(primitive)) = source.primitive_settings_of_id().get(&object_id) else {
                    return None;
                };
                let Some(transform) = get_transform(object_id) else {
                    return None;
                };
                Some(RenderObject::Brick(RenderBrick {
                    transform,
                    visualSettings: brick_info.visualSettings.clone(),
                    material: brick_info.material,
                    primitive: primitive.clone(),
                }))
            }
            ObjectInfo::Cylinder(cylinder_info) => {
                let Some(PrimitiveSettings::Cylinder(primitive)) = source.primitive_settings_of_id().get(&object_id) else {
                    return None;
                };
                let Some(transform) = get_transform(object_id) else {
                    return None;
                };
                Some(RenderObject::Cylinder(RenderCylinder {
                    transform,
                    visualSettings: cylinder_info.visualSettings.clone(),
                    material: cylinder_info.material,
                    primitive: primitive.clone(),
                }))
            }
            ObjectInfo::RigidGroup(_) => {
                let Some(transform) = get_transform(object_id) else {
                    return None;
                };
                Some(RenderObject::RigidGroup(transform))
            }
            ObjectInfo::LabelImage(label_image_info) => {
                let Some(physical_object_id) = label_image_info.physicalObjectId.map(|id| id.into()) else {
                    return None;
                };
                if get_physics_info(physical_object_id).is_none() {
                    return None;
                }
                Some(RenderObject::RenderLabel(RenderLabel {
                    physicalObjectId: physical_object_id,
                    offsetFromObject: label_image_info.offsetFromObject,
                    rotationFromObject: label_image_info.rotationFromObject,
                    billboardMode: label_image_info.billboardMode,
                    isConstantSize: label_image_info.isConstantSize,
                    visualSettings: label_image_info.visualSettings.clone(),
                    content: RenderLabelContent::Image {
                        textureDataId: label_image_info.textureDataId,
                        width: label_image_info.width,
                        height: label_image_info.height,
                    },
                }))
            }
            ObjectInfo::Mesh(mesh_info) => {
                let Some(physical_object_id) = mesh_info.physicalObjectId.map(|id| id.into()) else {
                    return None;
                };
                if get_physics_info(physical_object_id).is_none() {
                    return None;
                };
                let skin_skeleton_id = mesh_info.followSkeletonId.map(|id| id.into());
                Some(RenderObject::Mesh(RenderMesh {
                    physicalObjectId: physical_object_id,
                    visualSettings: mesh_info.visualSettings.clone(),
                    material: mesh_info.material,
                    meshDataId: mesh_info.meshDataId,
                    scale: mesh_info.scale,
                    skin_skeleton_id,
                }))
            }

            // Text labels are laid out together below.
            ObjectInfo::Label(_)
            | ObjectInfo::Player(_)
            | ObjectInfo::SoundPlayer(_)
            | ObjectInfo::Script(_)
            | ObjectInfo::LocalGroup(_)
            | ObjectInfo::Clock(_)
            | ObjectInfo::Pivot(_)
            | ObjectInfo::SkAnimation(_)
            | ObjectInfo::SkAttachment(_)
            | ObjectInfo::SkKeyframe(_)
            | ObjectInfo::SkKeyframePose(_)
            | ObjectInfo::SkAnimator(_)
            | ObjectInfo::PropertyAnimation(_)
            | ObjectInfo::PropertyKeyframe(_)
            | ObjectInfo::PropertyAnimator(_)
            | ObjectInfo::Tween(_) => None,
        }
    };

    let mut label_info_and_object_ids: Vec<(ObjectId, LabelInfo)> = Vec::new();
    for object_id in object_ids {
        let Some(object) = object_of_id.get(&object_id) else {
            render_object_of_id.remove(&object_id);
            continue;
        };
        if let ObjectInfo::Label(label_info) = &object.info
            && label_info
                .physicalObjectId
                .is_some_and(|physical_object_id| get_physics_info(physical_object_id.into()).is_some())
        {
            label_info_and_object_ids.push((object_id, label_info.clone()));
            continue;
        }
        match create_render_object(object_id, object) {
            Some(render_object) => render_object_of_id.insert(object_id, render_object),
            None => render_object_of_id.remove(&object_id),
        };
    }

    if !label_info_and_object_ids.is_empty() {
        let label_infos: Vec<LabelInfo> = label_info_and_object_ids.iter().map(|(_, label_info)| label_info.clone()).collect();
        let label_meshes = label_meshes_of_label_infos(&label_infos);
        for ((object_id, label_info), label_mesh) in label_info_and_object_ids.iter().zip(label_meshes) {
            let Some(physical_object_id) = label_info.physicalObjectId.map(|id| id.into()) else {
                continue;
            };
            render_object_of_id.insert(
                *object_id,
                RenderObject::RenderLabel(RenderLabel {
                    physicalObjectId: physical_object_id,
                    offsetFromObject: label_info.offsetFromObject,
                    rotationFromObject: label_info.rotationFromObject,
                    billboardMode: label_info.billboardMode,
                    isConstantSize: label_info.isConstantSize,
                    visualSettings: label_info.visualSettings.clone(),
                    content: RenderLabelContent::Text {
                        scale: label_mesh.scale_per_point,
                        backgroundColor: label_info.backgroundColor,
                        backgroundOpacity: label_info.backgroundOpacity,
                        geometry: label_mesh.geometry,
                        atlas: label_mesh.atlas,
                    },
                }),
            );
        }
    }
}

// A snapshot of these render objects, with the animation data every snapshot carries.
fn create_render_snapshot_of_render_objects<Source: Renderable + ?Sized>(
    source: &Source,
    render_object_of_id: HashMap<ObjectId, RenderObject>,
    kind_lists: RenderObjectKindLists,
) -> RenderSnapshot {
    RenderSnapshot {
        render_object_of_id,
        property_overrides_of_id: collect_property_overrides(
            source.object_of_id(),
            kind_lists.property_animator_ids,
            source.property_keyframes_arc_of_animation_id(),
            kind_lists.tween_ids,
        ),
        rotation_base_degrees_of_id: source.rotation_base_degrees_of_id(),
        pivot_offset_of_id: collect_pivot_offsets(source.object_of_id(), kind_lists.pivot_ids),
    }
}

// A drawing client's render objects, kept between snapshots so each one only rebuilds what may have changed: the objects its
// source says changed, the objects a changed attachment was or is on, and every text label and skeleton, since text labels share
// one atlas of letters and skeletons read every animator.
#[derive(Default)]
pub struct RenderObjectCache {
    render_object_of_id: Option<HashMap<ObjectId, RenderObject>>, // None until the first snapshot, which builds everything
    attached_object_id_of_sk_attachment_id: HashMap<ObjectId, ObjectId>,
}

impl RenderObjectCache {
    pub fn build_render_snapshot(&mut self, source: &impl Renderable, changed_object_ids: HashSet<ObjectId>) -> RenderSnapshot {
        let kind_lists = source.render_object_kind_lists();
        let attached_object_id_of_sk_attachment_id: HashMap<ObjectId, ObjectId> = kind_lists
            .sk_attachment_ids
            .iter()
            .filter_map(|&sk_attachment_id| {
                let ObjectInfo::SkAttachment(sk_attachment) = &source.object_of_id().get(&sk_attachment_id)?.info else {
                    return None;
                };
                Some((sk_attachment_id, sk_attachment.objectId?.into()))
            })
            .collect();
        let (mut render_object_of_id, object_ids_to_build): (HashMap<ObjectId, RenderObject>, HashSet<ObjectId>) = match self.render_object_of_id.take() {
            None => (
                HashMap::with_capacity(source.object_of_id().len()),
                source.object_of_id().keys().copied().collect(),
            ),
            Some(render_object_of_id) => {
                let attached_object_ids: Vec<ObjectId> = changed_object_ids
                    .iter()
                    .flat_map(|object_id| {
                        [
                            self.attached_object_id_of_sk_attachment_id.get(object_id),
                            attached_object_id_of_sk_attachment_id.get(object_id),
                        ]
                    })
                    .flatten()
                    .copied()
                    .collect();
                let object_ids_to_build = changed_object_ids
                    .into_iter()
                    .chain(attached_object_ids)
                    .chain(kind_lists.label_ids.iter().copied())
                    .chain(kind_lists.skeleton_ids.iter().copied())
                    .collect();
                (render_object_of_id, object_ids_to_build)
            }
        };
        update_render_objects(source, &mut render_object_of_id, object_ids_to_build, &kind_lists);
        self.attached_object_id_of_sk_attachment_id = attached_object_id_of_sk_attachment_id;
        self.render_object_of_id = Some(render_object_of_id.clone());
        create_render_snapshot_of_render_objects(source, render_object_of_id, kind_lists)
    }
}

// The earliest playing clock or transition start in each time domain, as (server, client). Previews treat
// it as the moment their timeline begins.
pub fn preview_timeline_start_ms<'a>(objects: impl Iterator<Item = &'a LiveObject>) -> anyhow::Result<(f64, f64)> {
    let mut origin_of_location = HashMap::new();
    for object in objects {
        let start = match &object.info {
            ObjectInfo::Clock(clock) => match clock.clockState {
                ClockState::Playing { localStartMs, timeLocation } => Some((timeLocation, localStartMs)),
                ClockState::Paused { .. } => None,
            },
            ObjectInfo::SkAnimator(animator) => animator.weightTransition.map(|transition| (transition.timeLocation, transition.startedAtMs)),
            ObjectInfo::PropertyAnimator(animator) => animator.weightTransition.map(|transition| (transition.timeLocation, transition.startedAtMs)),
            _ => None,
        };
        if let Some((location, start)) = start {
            anyhow::ensure!(start.is_finite(), "Invalid animation start time");
            let key = match location {
                ObjectLocation::Server => 0,
                ObjectLocation::Client => 1,
            };
            origin_of_location
                .entry(key)
                .and_modify(|origin: &mut f64| *origin = origin.min(start))
                .or_insert(start);
        }
    }
    Ok((
        origin_of_location.get(&0).copied().unwrap_or(0.0),
        origin_of_location.get(&1).copied().unwrap_or(0.0),
    ))
}
