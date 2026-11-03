use std::{collections::HashMap, sync::Arc};

use crate::{
    computeBlendedWeights::{AnimationBlendSample, computeBlendedWeights},
    renderer_types::{RenderObject, RenderSkeletonInfo},
};
use game_types::object_types::{
    ClockState, InterpolationStyle, Location3D, ObjectId, Orientation3D, PropertyKeyframeInfo, VisualSettings, WeightTransition,
    shared::{Color3, euler_degrees_to_quat, quat_to_euler_degrees},
};
use glam::Vec3;

// -------------------------- Property animation values --------------------------

// get value from the time
pub fn getCurrentPropertyValue(property_keyframes: &[PropertyKeyframeInfo], interpolationStyle: InterpolationStyle, animation_time_secs: f32) -> Option<f32> {
    let sample_time_secs = animation_time_secs.max(0.0);
    let mut previous_keyframe: Option<&PropertyKeyframeInfo> = None;
    let mut next_keyframe: Option<&PropertyKeyframeInfo> = None;
    for keyframe in property_keyframes {
        if keyframe.time <= sample_time_secs {
            let should_replace = match previous_keyframe {
                Some(previous_keyframe) => keyframe.time >= previous_keyframe.time,
                None => true,
            };
            if should_replace {
                previous_keyframe = Some(keyframe);
            }
            continue;
        }
        let should_replace = match next_keyframe {
            Some(next_keyframe) => keyframe.time < next_keyframe.time,
            None => true,
        };
        if should_replace {
            next_keyframe = Some(keyframe);
        }
    }

    match (previous_keyframe, next_keyframe) {
        (Some(previous_keyframe), Some(next_keyframe)) => {
            let alpha = interpolationStyle.computeAlpha(previous_keyframe.time, next_keyframe.time, animation_time_secs);
            Some(previous_keyframe.value + (next_keyframe.value - previous_keyframe.value) * alpha)
        }
        (Some(previous_keyframe), None) => Some(previous_keyframe.value),
        (None, Some(_)) | (None, None) => None,
    }
}

// -------------------------- Rotation property paths --------------------------

// x, y and z rotation are three property paths but one stored quaternion, so they are always resolved together.
// Returns (field, axis): an object's location.rotation, or a joint's restPose.rotation.
pub fn rotationAxisOfPropertyPath(property_path: &[String]) -> Option<(&str, &str)> {
    let [field, subfield, axis] = property_path else {
        return None;
    };
    if (field != "location" && field != "restPose") || subfield != "rotation" {
        return None;
    }
    match axis.as_str() {
        "x" | "y" | "z" => Some((field.as_str(), axis.as_str())),
        _ => None,
    }
}

pub fn isRotationAxisPropertyPath(property_path: &[String]) -> bool {
    rotationAxisOfPropertyPath(property_path).is_some()
}

// quat_to_euler_degrees only ever reports an x in [-90, 90], so an object tipped further than straight up comes back
// as a half turn of yaw and roll wrapped around a folded pitch: (x, y, z) and (180 - x, y + 180, z + 180) are the same
// rotation, and quat_to_euler_degrees always hands back the one with the smaller pitch. Only one of the two is the
// pitch and roll the object visibly has, and it is whichever keeps them smaller - the other is the fold.
//
// This matters when the triple is going to be used as a rotation base, because the base outlives the decompose:
// the axes nobody animates are taken from it for as long as the animation runs. Capturing the fold pins an object
// to it, and a lone pitch animator then renders every angle mirrored - 165 degrees of pitch draws as 15.
pub fn canonicalRotationBaseDegrees(euler_degrees: Vec3) -> Vec3 {
    fn wrap_degrees(degrees: f32) -> f32 {
        ((degrees + 180.0).rem_euclid(360.0)) - 180.0
    }
    let unfolded = Vec3::new(
        wrap_degrees(180.0 - euler_degrees.x),
        wrap_degrees(euler_degrees.y + 180.0),
        wrap_degrees(euler_degrees.z + 180.0),
    );
    let tilt_of = |euler: Vec3| euler.x.abs().max(euler.z.abs());
    if tilt_of(unfolded) < tilt_of(euler_degrees) {
        unfolded
    } else {
        euler_degrees
    }
}

// -------------------------- Property animation application --------------------------

#[derive(Clone)]
pub struct ComputedPropertyAnimation {
    pub property_keyframes_arc: Arc<Vec<PropertyKeyframeInfo>>,
    pub duration: f32,
    pub interpolationStyle: InterpolationStyle,

    pub clock_state: ClockState,
    pub should_loop: bool,
    pub speed: f32,

    pub priority: u8,
    pub weight: f32,
    pub weight_transition: Option<WeightTransition>,
}

#[derive(Clone)]
pub struct ComputedPropertyOverrides {
    pub computed_property_animations: Vec<(Vec<String>, Vec<ComputedPropertyAnimation>)>, // (path, animations)[]
}

// rotationAnimationBaseDegreesOfObjectId holds the euler angles an object had when rotation animation started
// (see World::updateRotationAnimationBases). Animating any rotation axis emits all three, with the axes nobody
// animates taken from that base, so animated rotation never has to be decomposed back out of the quaternion.
pub fn computePropertyOverridesOfObjectIdAtTime(
    computedPropertyOverridesOfObjectId: &HashMap<ObjectId, ComputedPropertyOverrides>,
    rotationAnimationBaseDegreesOfObjectId: &HashMap<ObjectId, Vec3>,
    server_ms: f64,
    client_ms: Option<f64>,
) -> HashMap<ObjectId, HashMap<Vec<String>, f32>> {
    let mut propertyOverrideOfPathOfObjectId = HashMap::new();

    for (object_id, computed_property_overrides) in computedPropertyOverridesOfObjectId {
        let mut propertyOverrideOfPath = HashMap::new();
        for (property_path, computed_property_animations) in &computed_property_overrides.computed_property_animations {
            struct CurrentPropertyValue {
                value: f32,
                priority: u8,
                computed_weight: f32,
            }
            // get CurrentPropertyValue
            let mut current_property_values = Vec::new();
            for computed_property_animation in computed_property_animations {
                let animation_time_secs = computed_property_animation.clock_state.readTimeSecs(
                    computed_property_animation.speed,
                    computed_property_animation.should_loop,
                    computed_property_animation.duration,
                    server_ms,
                    client_ms,
                );

                let Some(value) = getCurrentPropertyValue(
                    &computed_property_animation.property_keyframes_arc,
                    computed_property_animation.interpolationStyle,
                    animation_time_secs,
                ) else {
                    continue;
                };
                let computed_weight = WeightTransition::readValueWithFallback(
                    computed_property_animation.weight_transition,
                    computed_property_animation.weight,
                    server_ms,
                    client_ms,
                );
                current_property_values.push(CurrentPropertyValue {
                    value,
                    priority: computed_property_animation.priority,
                    computed_weight,
                });
            }
            if current_property_values.is_empty() {
                continue;
            }
            let blend_samples = current_property_values
                .iter()
                .map(|sample| AnimationBlendSample {
                    priority: sample.priority,
                    computed_weight: sample.computed_weight,
                    is_valid: true,
                })
                .collect::<Vec<_>>();
            let blend_weights = computeBlendedWeights(&blend_samples);
            if blend_weights.total_animation_weight <= 0.0 {
                continue;
            }
            let mut final_value = 0.0;
            for (sample_idx, sample) in current_property_values.iter().enumerate() {
                final_value += sample.value * blend_weights.sample_weights[sample_idx];
            }
            final_value /= blend_weights.total_animation_weight;
            propertyOverrideOfPath.insert(property_path.clone(), final_value);
        }
        let rotation_field = propertyOverrideOfPath
            .keys()
            .find_map(|property_path| rotationAxisOfPropertyPath(property_path).map(|(field, _)| field.to_owned()));
        if let (Some(rotation_field), Some(rotation_base_degrees)) = (rotation_field, rotationAnimationBaseDegreesOfObjectId.get(object_id)) {
            for (rotation_axis, base_degrees) in [("x", rotation_base_degrees.x), ("y", rotation_base_degrees.y), ("z", rotation_base_degrees.z)] {
                let rotation_axis_path = vec![rotation_field.clone(), "rotation".to_owned(), rotation_axis.to_owned()];
                propertyOverrideOfPath.entry(rotation_axis_path).or_insert(base_degrees);
            }
        }

        if !propertyOverrideOfPath.is_empty() {
            propertyOverrideOfPathOfObjectId.insert(*object_id, propertyOverrideOfPath);
        }
    }
    propertyOverrideOfPathOfObjectId
}

// -------------------------------------- Rest pose overrides on Skeleton joints --------------------------------------

// The world writes animated rest poses once per step, but the renderer draws between steps, so it applies them to a copy of
// the Skeleton every frame. computePropertyOverridesOfObjectIdAtTime fills in the rotation axes nobody animates from the
// joint's rotation base; with no base, as in the editor, they come from the joint's own rest rotation. Returns None when
// none of this Skeleton's joints are animated.
pub fn computeRenderSkeletonInfoWithRestPoseOverrides(
    render_skeleton_info: &RenderSkeletonInfo,
    propertyOverrideOfPathOfObjectId: &HashMap<ObjectId, HashMap<Vec<String>, f32>>,
) -> Option<RenderSkeletonInfo> {
    let mut animated_render_skeleton_info: Option<RenderSkeletonInfo> = None;
    for (joint_object_id, joint_index) in &render_skeleton_info.jointIndexOfObjectId {
        let Some(propertyOverrideOfPath) = propertyOverrideOfPathOfObjectId.get(joint_object_id) else {
            continue;
        };
        let rest_pose = &mut animated_render_skeleton_info.get_or_insert_with(|| render_skeleton_info.clone()).jointOfIndex[*joint_index].rest_pose;
        let stored_rotation = rest_pose.quaternion;
        let rotation_base_degrees = || canonicalRotationBaseDegrees(quat_to_euler_degrees(stored_rotation));
        let mut rotation_degrees: Option<Vec3> = None;
        for (property_path, property_override) in propertyOverrideOfPath {
            let [field, subfield, axis] = property_path.as_slice() else {
                continue;
            };
            if field != "restPose" {
                continue;
            }
            match (subfield.as_str(), axis.as_str()) {
                ("position", "x") => rest_pose.position.x = *property_override,
                ("position", "y") => rest_pose.position.y = *property_override,
                ("position", "z") => rest_pose.position.z = *property_override,
                ("rotation", "x") => rotation_degrees.get_or_insert_with(rotation_base_degrees).x = *property_override,
                ("rotation", "y") => rotation_degrees.get_or_insert_with(rotation_base_degrees).y = *property_override,
                ("rotation", "z") => rotation_degrees.get_or_insert_with(rotation_base_degrees).z = *property_override,
                _ => {}
            }
        }
        if let Some(rotation_degrees) = rotation_degrees {
            rest_pose.quaternion = euler_degrees_to_quat(rotation_degrees);
        }
    }
    animated_render_skeleton_info
}

// -------------------------------------- Applying property overrides to render objects --------------------------------------

// this is for property animations. it only includes continuous number property fields relevant to drawing an object
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum RenderPropertyTarget {
    Opacity,
    Glow,
    ColorR,
    ColorG,
    ColorB,
    LocationPositionX,
    LocationPositionY,
    LocationPositionZ,
    LocationRotationX,
    LocationRotationY,
    LocationRotationZ,
    MaterialTileSizeU,
    MaterialTileSizeV,
    MaterialTileOffsetU,
    MaterialTileOffsetV,
    Radius,
    Length,
    SizeX,
    SizeY,
    SizeZ,
    ScaleX,
    ScaleY,
    ScaleZ,
    SunOrientationTheta,
    SunOrientationPhi,
    SunColorR,
    SunColorG,
    SunColorB,
    SunIntensity,
    AmbientTopColorR,
    AmbientTopColorG,
    AmbientTopColorB,
    AmbientBottomColorR,
    AmbientBottomColorG,
    AmbientBottomColorB,
    AmbientIntensity,
    SunShadowSoftness,
    AmbientOcclusionIntensity,
    AmbientOcclusionRadius,
    AmbientOcclusionSunlightStrength,
    SkyTopColorR,
    SkyTopColorG,
    SkyTopColorB,
    SkyHorizonColorR,
    SkyHorizonColorG,
    SkyHorizonColorB,
    SkyBottomColorR,
    SkyBottomColorG,
    SkyBottomColorB,
    SkyboxTextureOpacity,
    SkyReflectionStrength,
    Vibrance,
}
impl RenderPropertyTarget {
    fn fromPropertyPath(property_path: &[String]) -> Option<Self> {
        match property_path {
            [field] => match field.as_str() {
                "opacity" => Some(Self::Opacity),
                "glow" => Some(Self::Glow),
                "radius" => Some(Self::Radius),
                "length" => Some(Self::Length),
                "sunIntensity" => Some(Self::SunIntensity),
                "sunShadowSoftness" => Some(Self::SunShadowSoftness),
                "ambientIntensity" => Some(Self::AmbientIntensity),
                "ambientOcclusionIntensity" => Some(Self::AmbientOcclusionIntensity),
                "ambientOcclusionRadius" => Some(Self::AmbientOcclusionRadius),
                "ambientOcclusionSunlightStrength" => Some(Self::AmbientOcclusionSunlightStrength),
                "skyboxTextureOpacity" => Some(Self::SkyboxTextureOpacity),
                "skyReflectionStrength" => Some(Self::SkyReflectionStrength),
                "vibrance" => Some(Self::Vibrance),
                _ => None,
            },
            [field, subfield] => match (field.as_str(), subfield.as_str()) {
                ("color", "r") => Some(Self::ColorR),
                ("color", "g") => Some(Self::ColorG),
                ("color", "b") => Some(Self::ColorB),
                ("material", "tileSizeU") => Some(Self::MaterialTileSizeU),
                ("material", "tileSizeV") => Some(Self::MaterialTileSizeV),
                ("material", "tileOffsetU") => Some(Self::MaterialTileOffsetU),
                ("material", "tileOffsetV") => Some(Self::MaterialTileOffsetV),
                ("size", "x") => Some(Self::SizeX),
                ("size", "y") => Some(Self::SizeY),
                ("size", "z") => Some(Self::SizeZ),
                ("scale", "x") => Some(Self::ScaleX),
                ("scale", "y") => Some(Self::ScaleY),
                ("scale", "z") => Some(Self::ScaleZ),
                ("sunOrientation", "theta") => Some(Self::SunOrientationTheta),
                ("sunOrientation", "phi") => Some(Self::SunOrientationPhi),
                ("sunColor", "r") => Some(Self::SunColorR),
                ("sunColor", "g") => Some(Self::SunColorG),
                ("sunColor", "b") => Some(Self::SunColorB),
                ("ambientTopColor", "r") => Some(Self::AmbientTopColorR),
                ("ambientTopColor", "g") => Some(Self::AmbientTopColorG),
                ("ambientTopColor", "b") => Some(Self::AmbientTopColorB),
                ("ambientBottomColor", "r") => Some(Self::AmbientBottomColorR),
                ("ambientBottomColor", "g") => Some(Self::AmbientBottomColorG),
                ("ambientBottomColor", "b") => Some(Self::AmbientBottomColorB),
                ("skyTopColor", "r") => Some(Self::SkyTopColorR),
                ("skyTopColor", "g") => Some(Self::SkyTopColorG),
                ("skyTopColor", "b") => Some(Self::SkyTopColorB),
                ("skyHorizonColor", "r") => Some(Self::SkyHorizonColorR),
                ("skyHorizonColor", "g") => Some(Self::SkyHorizonColorG),
                ("skyHorizonColor", "b") => Some(Self::SkyHorizonColorB),
                ("skyBottomColor", "r") => Some(Self::SkyBottomColorR),
                ("skyBottomColor", "g") => Some(Self::SkyBottomColorG),
                ("skyBottomColor", "b") => Some(Self::SkyBottomColorB),
                _ => None,
            },
            [field, subfield, component] => match (field.as_str(), subfield.as_str(), component.as_str()) {
                ("location", "position", "x") => Some(Self::LocationPositionX),
                ("location", "position", "y") => Some(Self::LocationPositionY),
                ("location", "position", "z") => Some(Self::LocationPositionZ),
                ("location", "rotation", "x") => Some(Self::LocationRotationX),
                ("location", "rotation", "y") => Some(Self::LocationRotationY),
                ("location", "rotation", "z") => Some(Self::LocationRotationZ),
                _ => None,
            },
            _ => None,
        }
    }
}
use self::RenderPropertyTarget as R;

fn get_override_value(propertyValueOfR: &HashMap<R, f32>, render_property_target: R, base_value: f32) -> f32 {
    let Some(property_override) = propertyValueOfR.get(&render_property_target) else {
        return base_value;
    };
    *property_override
}

fn applyColorPropertyOverrides(propertyValueOfR: &HashMap<R, f32>, color: &mut Color3, red_target: R, green_target: R, blue_target: R) {
    color.r = get_override_value(propertyValueOfR, red_target, color.r);
    color.g = get_override_value(propertyValueOfR, green_target, color.g);
    color.b = get_override_value(propertyValueOfR, blue_target, color.b);
}

fn applyVisualSettingsPropertyOverrides(propertyValueOfR: &HashMap<R, f32>, visualSettings: &mut VisualSettings) {
    visualSettings.opacity = get_override_value(propertyValueOfR, R::Opacity, visualSettings.opacity);
    visualSettings.glow = get_override_value(propertyValueOfR, R::Glow, visualSettings.glow);
    applyColorPropertyOverrides(propertyValueOfR, &mut visualSettings.color, R::ColorR, R::ColorG, R::ColorB);
}

// An object turns around its pivot, which is its own position unless a Pivot points at it. Position animators move the
// pivot, rotation animators turn the object around it, and the object ends up wherever that leaves it. The pivot is read
// off the current pose, which stays correct because turning around a point never moves that point.
fn applyLocationPropertyOverrides(propertyValueOfR: &HashMap<R, f32>, location: &mut Location3D, pivot_offset: Vec3) {
    let pivot = location.position + location.quaternion * pivot_offset;
    let pivot = Vec3::new(
        get_override_value(propertyValueOfR, R::LocationPositionX, pivot.x),
        get_override_value(propertyValueOfR, R::LocationPositionY, pivot.y),
        get_override_value(propertyValueOfR, R::LocationPositionZ, pivot.z),
    );

    let rotation_degrees = quat_to_euler_degrees(location.quaternion);
    location.quaternion = euler_degrees_to_quat(Vec3::new(
        get_override_value(propertyValueOfR, R::LocationRotationX, rotation_degrees.x),
        get_override_value(propertyValueOfR, R::LocationRotationY, rotation_degrees.y),
        get_override_value(propertyValueOfR, R::LocationRotationZ, rotation_degrees.z),
    ));
    location.position = pivot - location.quaternion * pivot_offset;
}

// The world applies animated locations with the same pivot rule as the renderer.
pub fn applyLocationPropertyOverridesOfPath(propertyOverrideOfPath: &HashMap<Vec<String>, f32>, location: &mut Location3D, pivot_offset: Vec3) {
    let propertyValueOfR = propertyOverrideOfPath
        .iter()
        .filter_map(|(property_path, property_override)| {
            R::fromPropertyPath(property_path).map(|render_property_target| (render_property_target, *property_override))
        })
        .collect();
    applyLocationPropertyOverrides(&propertyValueOfR, location, pivot_offset);
}

fn applyOrientationPropertyOverrides(propertyValueOfR: &HashMap<R, f32>, orientation: &mut Orientation3D, theta_target: R, phi_target: R) {
    orientation.theta = get_override_value(propertyValueOfR, theta_target, orientation.theta.to_degrees()).to_radians();
    orientation.phi = -get_override_value(propertyValueOfR, phi_target, -orientation.phi.to_degrees()).to_radians();
}

pub fn applyPropertyOverridesToRenderObjectOfId(
    render_object_of_id: &mut HashMap<ObjectId, RenderObject>,
    propertyOverrideOfPathOfObjectId: &HashMap<ObjectId, HashMap<Vec<String>, f32>>,
    pivot_offset_of_id: &HashMap<ObjectId, Vec3>,
) {
    // Resolve animated bodies before their children, independent of the iteration order of the map.
    // Order references to the overrides so each property's path is parsed just once.
    let (group_overrides, other_overrides): (Vec<_>, Vec<_>) =
        propertyOverrideOfPathOfObjectId
            .iter()
            .partition(|(object_id, _)| match render_object_of_id.get(object_id) {
                Some(RenderObject::RigidGroup(_)) => true,
                _ => false,
            });
    for (object_id, propertyOverrideOfPath) in group_overrides.into_iter().chain(other_overrides) {
        let mut propertyValueOfR = HashMap::new();
        for (property_path, property_override) in propertyOverrideOfPath {
            let Some(render_property_target) = R::fromPropertyPath(property_path) else {
                continue;
            };
            propertyValueOfR.insert(render_property_target, *property_override);
        }
        if propertyValueOfR.is_empty() {
            continue;
        }
        let is_location_overridden = propertyValueOfR.keys().any(|target| match target {
            R::LocationPositionX | R::LocationPositionY | R::LocationPositionZ | R::LocationRotationX | R::LocationRotationY | R::LocationRotationZ => true,
            _ => false,
        });
        let group_location = if is_location_overridden {
            render_object_of_id
                .get(object_id)
                .and_then(RenderObject::getTransform)
                .and_then(|transform| transform.rigid_group_to_follow.as_ref())
                .and_then(|(group_id, _)| render_object_of_id.get(group_id))
                .and_then(|render_object| match render_object {
                    RenderObject::RigidGroup(group) => Some(group.physicsInfo3D.location.clone()),
                    _ => None,
                })
        } else {
            None
        };
        let Some(render_object) = render_object_of_id.get_mut(object_id) else {
            continue;
        };

        // A location animator writes world coordinates. Resolve the inherited pose first, then keep its
        // overridden location for this frame so the group transform is not applied a second time.
        // Store that override as a local offset so the child keeps its group relationship.
        if is_location_overridden && let Some(transform) = render_object.getTransformMut() {
            let pivot_offset = pivot_offset_of_id.get(object_id).copied().unwrap_or(Vec3::ZERO);
            match (&mut transform.rigid_group_to_follow, group_location) {
                (Some((_, local_location)), Some(group_location)) => {
                    let mut location = Location3D {
                        position: group_location.position + group_location.quaternion * local_location.position,
                        quaternion: group_location.quaternion * local_location.quaternion,
                    };
                    applyLocationPropertyOverrides(&propertyValueOfR, &mut location, pivot_offset);
                    let inverse_rotation = group_location.quaternion.inverse();
                    local_location.position = inverse_rotation * (location.position - group_location.position);
                    local_location.quaternion = inverse_rotation * location.quaternion;
                    transform.physicsInfo3D.location = location;
                }
                (None, _) => applyLocationPropertyOverrides(&propertyValueOfR, &mut transform.physicsInfo3D.location, pivot_offset),
                _ => {}
            }
        }

        match render_object {
            RenderObject::Ball(ball) => {
                applyVisualSettingsPropertyOverrides(&propertyValueOfR, &mut ball.visualSettings);
                ball.primitive.radius = get_override_value(&propertyValueOfR, R::Radius, ball.primitive.radius);
                if let Some(material) = &mut ball.material {
                    material.tileSizeU = get_override_value(&propertyValueOfR, R::MaterialTileSizeU, material.tileSizeU);
                    material.tileSizeV = get_override_value(&propertyValueOfR, R::MaterialTileSizeV, material.tileSizeV);
                    material.tileOffsetU = get_override_value(&propertyValueOfR, R::MaterialTileOffsetU, material.tileOffsetU);
                    material.tileOffsetV = get_override_value(&propertyValueOfR, R::MaterialTileOffsetV, material.tileOffsetV);
                }
            }
            RenderObject::Cylinder(cylinder) => {
                applyVisualSettingsPropertyOverrides(&propertyValueOfR, &mut cylinder.visualSettings);
                cylinder.primitive.radius = get_override_value(&propertyValueOfR, R::Radius, cylinder.primitive.radius);
                cylinder.primitive.length = get_override_value(&propertyValueOfR, R::Length, cylinder.primitive.length);
                if let Some(material) = &mut cylinder.material {
                    material.tileSizeU = get_override_value(&propertyValueOfR, R::MaterialTileSizeU, material.tileSizeU);
                    material.tileSizeV = get_override_value(&propertyValueOfR, R::MaterialTileSizeV, material.tileSizeV);
                    material.tileOffsetU = get_override_value(&propertyValueOfR, R::MaterialTileOffsetU, material.tileOffsetU);
                    material.tileOffsetV = get_override_value(&propertyValueOfR, R::MaterialTileOffsetV, material.tileOffsetV);
                }
            }
            RenderObject::Brick(brick) => {
                applyVisualSettingsPropertyOverrides(&propertyValueOfR, &mut brick.visualSettings);
                brick.primitive.size.x = get_override_value(&propertyValueOfR, R::SizeX, brick.primitive.size.x);
                brick.primitive.size.y = get_override_value(&propertyValueOfR, R::SizeY, brick.primitive.size.y);
                brick.primitive.size.z = get_override_value(&propertyValueOfR, R::SizeZ, brick.primitive.size.z);
                if let Some(material) = &mut brick.material {
                    material.tileSizeU = get_override_value(&propertyValueOfR, R::MaterialTileSizeU, material.tileSizeU);
                    material.tileSizeV = get_override_value(&propertyValueOfR, R::MaterialTileSizeV, material.tileSizeV);
                    material.tileOffsetU = get_override_value(&propertyValueOfR, R::MaterialTileOffsetU, material.tileOffsetU);
                    material.tileOffsetV = get_override_value(&propertyValueOfR, R::MaterialTileOffsetV, material.tileOffsetV);
                }
            }
            RenderObject::Mesh(mesh) => {
                applyVisualSettingsPropertyOverrides(&propertyValueOfR, &mut mesh.visualSettings);
                mesh.scale.x = get_override_value(&propertyValueOfR, R::ScaleX, mesh.scale.x);
                mesh.scale.y = get_override_value(&propertyValueOfR, R::ScaleY, mesh.scale.y);
                mesh.scale.z = get_override_value(&propertyValueOfR, R::ScaleZ, mesh.scale.z);
            }
            RenderObject::WorldLighting(world_lighting) => {
                applyOrientationPropertyOverrides(
                    &propertyValueOfR,
                    &mut world_lighting.sunOrientation,
                    R::SunOrientationTheta,
                    R::SunOrientationPhi,
                );
                applyColorPropertyOverrides(&propertyValueOfR, &mut world_lighting.sunColor, R::SunColorR, R::SunColorG, R::SunColorB);
                world_lighting.sunIntensity = get_override_value(&propertyValueOfR, R::SunIntensity, world_lighting.sunIntensity);
                world_lighting.sunShadowSoftness = get_override_value(&propertyValueOfR, R::SunShadowSoftness, world_lighting.sunShadowSoftness);
                applyColorPropertyOverrides(
                    &propertyValueOfR,
                    &mut world_lighting.ambientTopColor,
                    R::AmbientTopColorR,
                    R::AmbientTopColorG,
                    R::AmbientTopColorB,
                );
                applyColorPropertyOverrides(
                    &propertyValueOfR,
                    &mut world_lighting.ambientBottomColor,
                    R::AmbientBottomColorR,
                    R::AmbientBottomColorG,
                    R::AmbientBottomColorB,
                );
                world_lighting.ambientIntensity = get_override_value(&propertyValueOfR, R::AmbientIntensity, world_lighting.ambientIntensity);
                world_lighting.ambientOcclusionIntensity =
                    get_override_value(&propertyValueOfR, R::AmbientOcclusionIntensity, world_lighting.ambientOcclusionIntensity);
                world_lighting.ambientOcclusionRadius = get_override_value(&propertyValueOfR, R::AmbientOcclusionRadius, world_lighting.ambientOcclusionRadius);
                world_lighting.ambientOcclusionSunlightStrength = get_override_value(
                    &propertyValueOfR,
                    R::AmbientOcclusionSunlightStrength,
                    world_lighting.ambientOcclusionSunlightStrength,
                );
                applyColorPropertyOverrides(
                    &propertyValueOfR,
                    &mut world_lighting.skyTopColor,
                    R::SkyTopColorR,
                    R::SkyTopColorG,
                    R::SkyTopColorB,
                );
                applyColorPropertyOverrides(
                    &propertyValueOfR,
                    &mut world_lighting.skyHorizonColor,
                    R::SkyHorizonColorR,
                    R::SkyHorizonColorG,
                    R::SkyHorizonColorB,
                );
                applyColorPropertyOverrides(
                    &propertyValueOfR,
                    &mut world_lighting.skyBottomColor,
                    R::SkyBottomColorR,
                    R::SkyBottomColorG,
                    R::SkyBottomColorB,
                );
                world_lighting.skyboxTextureOpacity = get_override_value(&propertyValueOfR, R::SkyboxTextureOpacity, world_lighting.skyboxTextureOpacity);
                world_lighting.skyReflectionStrength = get_override_value(&propertyValueOfR, R::SkyReflectionStrength, world_lighting.skyReflectionStrength);
                world_lighting.vibrance = get_override_value(&propertyValueOfR, R::Vibrance, world_lighting.vibrance);
            }
            _ => {}
        }
    }

    // Poses sampled after this see the animated rest poses.
    for render_object in render_object_of_id.values_mut() {
        let RenderObject::Skeleton(render_skeleton) = render_object else {
            continue;
        };
        if let Some(render_skeleton_info) =
            computeRenderSkeletonInfoWithRestPoseOverrides(&render_skeleton.renderSkeletonInfoArc, propertyOverrideOfPathOfObjectId)
        {
            render_skeleton.renderSkeletonInfoArc = Arc::new(render_skeleton_info);
        }
    }
}
