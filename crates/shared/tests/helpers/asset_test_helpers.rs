#![allow(non_snake_case, dead_code)]
use game_types::{
    object_types::{shared::Color3, *},
    stored_world_type::StoredWorld,
};

use glam::{Quat, Vec3};

use super::world_test_helpers::*;

// -------------------------- Asset object fixture --------------------------
// One reusable fixture covers every object variant and keeps all references inside its brick.
// Asset IDs are supplied by offline tests or by the real uploads in the live test.
pub fn asset_object(mesh_data_id: i64, texture_data_id: i64, sound_data_id: i64) -> anyhow::Result<StoredWorld> {
    let mut world = make_world()?;
    add_brick(&mut world, 100, 1, Vec3::new(0.0, 2.0, 0.0), false)?;
    add_ball(&mut world, 101, 100, Vec3::new(-3.0, 4.0, 0.0), false)?;
    add_cylinder(&mut world, 102, 100, Vec3::new(3.0, 4.0, 0.0), false)?;
    add_clock(&mut world, 103, 100)?;
    add_material(&mut world, 104, 100)?;
    add_skeleton(&mut world, 105, 100)?;
    world.addObjectWithId(
        106,
        "joint".into(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::Y,
                quaternion: Quat::IDENTITY,
            },
        }),
        105,
    );
    add_local_group(&mut world, 107, 100)?;
    add_rigid_group(&mut world, 108, 100, Vec3::new(0.0, 5.0, 0.0), Some(105))?;
    add_script(&mut world, 109, 100)?;
    add_label(&mut world, 110, 100)?;
    add_mesh(&mut world, 111, 100)?;
    add_player(&mut world, 112, 100, Some(101))?;
    add_sound_player(&mut world, 113, 100, Some(103))?;
    add_sk_animation(&mut world, 114, 105)?;
    add_sk_keyframe(&mut world, 115, 114)?;
    add_sk_keyframe_pose(&mut world, 116, 115)?;
    add_sk_attachment(&mut world, 117, 105, Some(102))?;
    add_sk_animator(&mut world, 118, 105)?;
    add_property_animation(&mut world, 119, 100)?;
    add_property_keyframe(&mut world, 120, 119)?;
    add_property_animator(&mut world, 121, 100)?;
    add_world_lighting(&mut world, 122, 100)?;
    add_pivot(&mut world, 123, 100, Some(102), Vec3::new(0.0, 0.0, 1.0))?;
    add_tween(&mut world, 124, 101)?;
    add_label_image(&mut world, 125, 100)?;
    // Adjust the stored scene used by browser and native rendering tests.
    let mut stored: StoredWorld = serde_json::from_str(&world.toStoredWorldString()?)?;
    stored.objectOfId.get_mut(&stored.rootObjectId).unwrap().name = "All-Object-Types".into();
    stored.scriptRawCodeOfId.insert(109, "console.log('Asset object script loaded');".into());

    // Give pointer fields and optional data real values so losing them cannot pass a round trip.
    for object in stored.objectOfId.values_mut() {
        match &mut object.info {
            ObjectInfo::Brick(info) => {
                info.material = Some(BrickMaterialSettings {
                    materialInfoId: ObjectIdPointer(104),
                    tileSizeU: 2.0,
                    tileSizeV: 3.0,
                    tileOffsetU: 0.25,
                    tileOffsetV: 0.5,
                })
            }
            ObjectInfo::Ball(info) => {
                info.material = Some(BallMaterialSettings {
                    materialInfoId: ObjectIdPointer(104),
                    tileSizeU: 3.0,
                    tileSizeV: 2.0,
                    tileOffsetU: 0.5,
                    tileOffsetV: 0.25,
                })
            }
            ObjectInfo::Cylinder(info) => {
                info.material = Some(CylinderMaterialSettings {
                    materialInfoId: ObjectIdPointer(104),
                    tileSizeU: 4.0,
                    tileSizeV: 2.0,
                    tileOffsetU: 0.75,
                    tileOffsetV: 0.25,
                })
            }
            ObjectInfo::Clock(info) => {
                *info = ClockInfo {
                    shouldLoop: true,
                    speed: 0.75,
                    clockState: ClockState::Paused { animationTimeSecs: 0.25 },
                }
            }
            ObjectInfo::Material(info) => {
                *info = MaterialInfo {
                    albedoTextureDataId: Some(texture_data_id),
                    metallicTextureDataId: Some(texture_data_id),
                    roughnessTextureDataId: Some(texture_data_id),
                    normalTextureDataId: Some(texture_data_id),
                    alphaMode: material::AlphaMode::Mask { cutoff: 0.25 },
                    isDoubleSided: false,
                }
            }
            ObjectInfo::Label(info) => {
                info.text = "Every object type".into();
                info.lineHeight = 0.75;
                info.maxWidth = Some(8.0);
                info.horizontalAlign = LabelHorizontalAlign::Left;
                info.backgroundColor = Some(Color3::BLACK);
                info.backgroundOpacity = 0.5;
                info.offsetFromObject = Vec3::new(0.0, 2.0, 0.0);
                info.rotationFromObject = Quat::from_rotation_y(0.5);
                info.physicalObjectId = Some(ObjectIdPointer(100));
                info.billboardMode = BillboardMode::None;
                info.isConstantSize = true;
                info.visualSettings = VisualSettings {
                    opacity: 0.75,
                    color: Color3::WHITE,
                    glow: 0.0,
                    isAlwaysOnTop: true,
                };
            }
            ObjectInfo::LabelImage(info) => {
                info.textureDataId = texture_data_id;
                info.width = 1.5;
                info.height = 0.75;
                info.offsetFromObject = Vec3::new(0.0, 0.0, 0.51);
                info.rotationFromObject = Quat::from_rotation_y(0.25);
                info.physicalObjectId = Some(ObjectIdPointer(100));
                info.billboardMode = BillboardMode::None;
                info.visualSettings.opacity = 0.75;
            }
            ObjectInfo::Mesh(info) => {
                info.meshDataId = mesh_data_id;
                info.scale = Vec3::new(0.5, 0.75, 1.0);
                info.followSkeletonId = Some(ObjectIdPointer(105));
                info.physicalObjectId = Some(ObjectIdPointer(108));
                info.material = Some(MeshMaterialSettings {
                    materialInfoId: ObjectIdPointer(104),
                });
            }
            ObjectInfo::Player(info) => {
                info.headObjectId = Some(ObjectIdPointer(102));
                info.username = "Asset Test Player".into();
                info.userid = None;
            }
            ObjectInfo::SoundPlayer(info) => {
                info.soundDataId = sound_data_id;
                info.volume = 0.35;
            }
            ObjectInfo::SkAnimation(info) => {
                *info = SkAnimationInfo {
                    duration: 2.5,
                    interpolationStyle: InterpolationStyle::Linear,
                }
            }
            ObjectInfo::SkKeyframe(info) => info.time = 1.25,
            ObjectInfo::SkKeyframePose(info) => {
                info.localPose = Location3D {
                    position: Vec3::new(0.0, 1.5, 0.0),
                    quaternion: Quat::from_rotation_z(0.25),
                }
            }
            ObjectInfo::SkAttachment(info) => info.offsetFromJoint.position = Vec3::new(0.0, 0.5, 0.0),
            ObjectInfo::Pivot(info) => info.offsetFromObject = Vec3::new(0.0, 0.0, 1.5),
            ObjectInfo::SkAnimator(info) => {
                info.skeletonId = Some(ObjectIdPointer(105));
                info.skAnimationId = Some(ObjectIdPointer(114));
                info.clockId = Some(ObjectIdPointer(103));
                info.priority = 2;
                info.weight = 75.0;
                info.weightTransition = Some(WeightTransition {
                    startedAtMs: 0.0,
                    timeLocation: ObjectLocation::Server,
                    initValue: 25.0,
                    finalValue: 75.0,
                    durationMs: 500.0,
                    easing: InterpolationStyle::EaseIn,
                });
            }
            ObjectInfo::PropertyAnimation(info) => {
                *info = PropertyAnimationInfo {
                    duration: 3.5,
                    interpolationStyle: InterpolationStyle::EaseOut,
                }
            }
            ObjectInfo::PropertyKeyframe(info) => *info = PropertyKeyframeInfo { time: 1.5, value: 0.5 },
            ObjectInfo::PropertyAnimator(info) => {
                info.targetObjectId = Some(ObjectIdPointer(101));
                info.propertyPath = vec!["opacity".into()];
                info.propertyAnimationId = Some(ObjectIdPointer(119));
                info.clockId = Some(ObjectIdPointer(103));
                info.priority = 3;
                info.weight = 50.0;
                info.weightTransition = Some(WeightTransition {
                    startedAtMs: 0.0,
                    timeLocation: ObjectLocation::Server,
                    initValue: 0.0,
                    finalValue: 50.0,
                    durationMs: 750.0,
                    easing: InterpolationStyle::EaseOut,
                });
            }
            ObjectInfo::Tween(info) => {
                *info = TweenInfo {
                    targetObjectId: Some(ObjectIdPointer(101)),
                    propertyPath: vec!["density".into()],
                    fromValue: 1.5,
                    toValue: 2.5,
                    duration: 1.25,
                    interpolationStyle: InterpolationStyle::EaseIn,
                    clockState: ClockState::Paused { animationTimeSecs: 0.25 },
                }
            }
            ObjectInfo::WorldLighting(info) => {
                info.sunIntensity = 2.5;
                info.skyboxTextureDataId = Some(texture_data_id);
                info.skyboxTextureOpacity = 0.5;
            }
            ObjectInfo::SkJoint(_) | ObjectInfo::Skeleton(_) | ObjectInfo::LocalGroup(_) | ObjectInfo::RigidGroup(_) | ObjectInfo::Script(_) => {}
        }
    }
    Ok(stored)
}
