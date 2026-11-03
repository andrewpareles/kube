use crate::game::world::World;
use game_types::{
    object_types::{LiveObject, Location3D, ObjectId, ObjectKind, PhysicsInfo3D, PropertyKeyframeInfo},
    physics_primitive_types::PrimitiveSettings,
};
use glam::Vec3;
use render_core::{
    renderable::{RenderObjectKindLists, Renderable},
    renderer_types::{RenderSkKeyframe, RenderSkeletonInfo},
};
use std::{collections::HashMap, sync::Arc};

// The live world owns physics and animation caches; scene conversion does not need the native runtime.
impl Renderable for World {
    fn object_of_id(&self) -> &HashMap<ObjectId, LiveObject> {
        &self.objectOfId
    }

    fn parent_of_id(&self) -> &HashMap<ObjectId, ObjectId> {
        &self.parentOfId
    }

    fn primitive_settings_of_id(&self) -> &HashMap<ObjectId, PrimitiveSettings> {
        &self.physics.primitiveSettingsOfId
    }

    fn skeleton_info_arc_of_id(&self) -> &HashMap<ObjectId, Arc<RenderSkeletonInfo>> {
        &self.renderSkeletonInfoArcOfId
    }

    fn sk_keyframes_of_animation_id(&self) -> &HashMap<ObjectId, Vec<RenderSkKeyframe>> {
        &self.renderSkKeyframesOfSkAnimationId
    }

    fn property_keyframes_arc_of_animation_id(&self) -> &HashMap<ObjectId, Arc<Vec<PropertyKeyframeInfo>>> {
        &self.propertyKeyframesArcOfPropertyAnimationId
    }

    fn rotation_base_degrees_of_id(&self) -> HashMap<ObjectId, Vec3> {
        self.rotationAnimationBaseDegreesOfObjectId.clone()
    }

    fn render_object_kind_lists(&self) -> RenderObjectKindLists {
        let object_ids_of_kind = |object_kind: ObjectKind| self._kindOfObjectId.get_keys(&object_kind).into_iter().flatten().copied().collect();
        RenderObjectKindLists {
            sk_animator_ids: object_ids_of_kind(ObjectKind::SkAnimator),
            property_animator_ids: object_ids_of_kind(ObjectKind::PropertyAnimator),
            tween_ids: object_ids_of_kind(ObjectKind::Tween),
            sk_attachment_ids: object_ids_of_kind(ObjectKind::SkAttachment),
            pivot_ids: object_ids_of_kind(ObjectKind::Pivot),
            label_ids: object_ids_of_kind(ObjectKind::Label),
            skeleton_ids: object_ids_of_kind(ObjectKind::Skeleton),
        }
        .sorted()
    }

    // Capture body motion and attachment data once for every physical render object.
    fn physics_info(&self, object_id: ObjectId) -> Option<PhysicsInfo3D> {
        self.physics.getObjectPhysicsInfo3D(object_id)
    }

    fn rigid_group_to_follow(&self, object_id: ObjectId) -> Option<(ObjectId, Location3D)> {
        self.physics
            .getSubitemRigidGroupId(object_id)
            .zip(self.physics.getSubitemRelativeLocation3D(object_id))
    }

    fn teleport_count(&self, object_id: ObjectId) -> u64 {
        self.getTeleportCount(object_id)
    }
}
