//! `DehydratedGltf::validate` is the gate between stored asset data and the importer that builds
//! world objects from it. Only its happy path and its Blender id check had coverage; these cover
//! every other way a stored model can be wrong, so a model that would break the importer is refused
//! while it is still data.

use game_types::{
    consts::DEHYDRATED_GLTF_CURRENT_VERSION,
    object_types::{InterpolationStyle, Location3D, MaterialInfo, SkAnimationInfo, material::AlphaMode},
};
use glam::{Quat, Vec3};
use render_core::{
    dehydrated_gltf::{DehydratedAnimation, DehydratedGltf, DehydratedMaterial, DehydratedMesh, DehydratedSkeleton, validate_skeleton_content},
    renderer_types::{RenderSkJoint, RenderSkKeyframe, RenderSkKeyframePose},
};

fn pose() -> Location3D {
    Location3D {
        position: Vec3::ZERO,
        quaternion: Quat::IDENTITY,
    }
}

fn joint(name: &str, parent_joint_index: Option<usize>) -> RenderSkJoint {
    RenderSkJoint {
        name: name.to_string(),
        parent_joint_index,
        rest_pose: pose(),
    }
}

fn mesh(mesh_data_id: i64, skinned_skeleton_index: Option<usize>) -> DehydratedMesh {
    DehydratedMesh {
        name: "mesh".to_string(),
        mesh_data_id,
        skinned_skeleton_index,
        material: None,
    }
}

// One well-formed model every test starts from, so each case below differs from a model that
// validates in exactly the one way it is named for.
fn model() -> DehydratedGltf {
    DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id: None,
        import_scale: 1.0,
        dehydrated_meshes: vec![mesh(11, None)],
        dehydrated_skeletons: Vec::new(),
    }
}

fn skeleton(skeleton_index: usize, joints: Vec<RenderSkJoint>, animations: Vec<DehydratedAnimation>) -> DehydratedSkeleton {
    DehydratedSkeleton {
        skeleton_index,
        jointOfIndex: joints,
        animations,
    }
}

fn animation(duration: f32, sk_keyframes: Vec<RenderSkKeyframe>) -> DehydratedAnimation {
    DehydratedAnimation {
        name: "Walk".to_string(),
        sk_animation_info: SkAnimationInfo {
            duration,
            interpolationStyle: InterpolationStyle::Linear,
        },
        sk_keyframes,
    }
}

fn keyframe(time: f32, joint_names: &[&str]) -> RenderSkKeyframe {
    RenderSkKeyframe {
        time,
        skKeyframePoses: joint_names
            .iter()
            .map(|name| RenderSkKeyframePose {
                jointName: name.to_string(),
                localPose: pose(),
            })
            .collect(),
    }
}

fn assert_rejected(gltf: DehydratedGltf, reason: &str) {
    let error = gltf.validate().unwrap_err().to_string();
    assert!(error.contains(reason), "expected a model refused for {reason}, got {error}");
}

// The model that every case below is a variation of has to validate, or those cases prove nothing.
#[test]
fn a_well_formed_model_validates() -> anyhow::Result<()> {
    model().validate()?;
    let mut skinned = model();
    skinned.dehydrated_meshes = vec![mesh(11, Some(0))];
    skinned.dehydrated_skeletons = vec![skeleton(
        0,
        vec![joint("root", None), joint("hand", Some(0))],
        vec![animation(1.0, vec![keyframe(0.0, &["root"]), keyframe(1.0, &["root", "hand"])])],
    )];
    skinned.validate()?;
    Ok(())
}

// Stored models are brought up to date as they are read, so the only ones that reach validation at another version are
// ones a newer server wrote, and those can hold fields this build would misread.
#[test]
fn a_model_at_any_other_version_is_refused() {
    for version in [DEHYDRATED_GLTF_CURRENT_VERSION - 1, DEHYDRATED_GLTF_CURRENT_VERSION + 1] {
        let mut gltf = model();
        gltf.version = version;
        assert_rejected(gltf, "Unsupported dehydrated glTF version");
    }
}

#[test]
fn a_model_with_nothing_to_draw_is_refused() {
    let mut gltf = model();
    gltf.dehydrated_meshes = Vec::new();
    assert_rejected(gltf, "no mesh parts");
}

// Import scale divides and multiplies positions all through the importer, so zero or a non-finite
// value spreads through every object it builds instead of failing where it came from.
#[test]
fn a_scale_that_cannot_be_imported_is_refused() {
    for import_scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let mut gltf = model();
        gltf.import_scale = import_scale;
        assert_rejected(gltf, "model scale");
    }
}

#[test]
fn mesh_data_ids_and_texture_ids_have_to_point_somewhere() {
    for mesh_data_id in [0, -1, i64::MIN] {
        let mut gltf = model();
        gltf.dehydrated_meshes = vec![mesh(mesh_data_id, None)];
        assert_rejected(gltf, "mesh data id");
    }
    // Every texture slot on the material is checked, not just the first one that happens to be set.
    for slot in 0..4 {
        let mut material_info = MaterialInfo {
            albedoTextureDataId: None,
            metallicTextureDataId: None,
            roughnessTextureDataId: None,
            normalTextureDataId: None,
            alphaMode: AlphaMode::Opaque,
            isDoubleSided: false,
        };
        let slots = [
            &mut material_info.albedoTextureDataId,
            &mut material_info.metallicTextureDataId,
            &mut material_info.roughnessTextureDataId,
            &mut material_info.normalTextureDataId,
        ];
        *slots[slot] = Some(0);
        let mut gltf = model();
        gltf.dehydrated_meshes = vec![DehydratedMesh {
            name: "mesh".to_string(),
            mesh_data_id: 11,
            skinned_skeleton_index: None,
            material: Some(DehydratedMaterial {
                name: "material".to_string(),
                material_info: material_info,
            }),
        }];
        assert_rejected(gltf, "texture data id");
    }
}

// Meshes find their skeleton by index, so two skeletons claiming one index means a mesh silently
// binds to whichever came last.
#[test]
fn two_skeletons_cannot_claim_the_same_index() {
    let mut gltf = model();
    gltf.dehydrated_skeletons = vec![
        skeleton(0, vec![joint("root", None)], Vec::new()),
        skeleton(0, vec![joint("root", None)], Vec::new()),
    ];
    assert_rejected(gltf, "Repeated skeleton index");
}

#[test]
fn a_mesh_cannot_be_skinned_to_a_skeleton_that_is_not_there() {
    let mut gltf = model();
    gltf.dehydrated_meshes = vec![mesh(11, Some(1))];
    gltf.dehydrated_skeletons = vec![skeleton(0, vec![joint("root", None)], Vec::new())];
    assert_rejected(gltf, "missing skeleton");
}

// Poses name their joint, so two joints sharing a name make a keyframe ambiguous.
#[test]
fn joints_need_distinct_names_and_a_parent_that_comes_first() {
    let mut repeated = model();
    repeated.dehydrated_skeletons = vec![skeleton(0, vec![joint("root", None), joint("root", Some(0))], Vec::new())];
    assert_rejected(repeated, "Repeated joint name");

    // The pose of a joint is built from its parent's, so a parent later in the list is not ready yet.
    for parent_joint_index in [Some(1), Some(2)] {
        let mut gltf = model();
        gltf.dehydrated_skeletons = vec![skeleton(0, vec![joint("root", None), joint("hand", parent_joint_index)], Vec::new())];
        assert_rejected(gltf, "parent must precede");
    }
}

#[test]
fn animations_need_a_real_duration_and_keyframes_that_move_forward() {
    let mut duration = model();
    duration.dehydrated_skeletons = vec![skeleton(
        0,
        vec![joint("root", None)],
        vec![animation(f32::NAN, vec![keyframe(0.0, &["root"])])],
    )];
    assert_rejected(duration, "animation duration");

    let mut negative = model();
    negative.dehydrated_skeletons = vec![skeleton(0, vec![joint("root", None)], vec![animation(-1.0, vec![keyframe(0.0, &["root"])])])];
    assert_rejected(negative, "animation duration");

    // Sampling walks the keyframes in order, so one that goes backwards is never reached.
    for times in [[0.0, -1.0], [1.0, 0.5], [0.0, f32::NAN]] {
        let mut gltf = model();
        let keyframes = times.iter().map(|time| keyframe(*time, &["root"])).collect();
        gltf.dehydrated_skeletons = vec![skeleton(0, vec![joint("root", None)], vec![animation(1.0, keyframes)])];
        assert_rejected(gltf, "keyframe time");
    }
}

#[test]
fn a_keyframe_cannot_pose_a_joint_the_skeleton_does_not_have() {
    let mut gltf = model();
    gltf.dehydrated_skeletons = vec![skeleton(0, vec![joint("root", None)], vec![animation(1.0, vec![keyframe(0.0, &["elbow"])])])];
    assert_rejected(gltf, "unknown joint");
}

// Loaded imports go through this directly, without a DehydratedGltf around them, so it has to
// refuse the same things on its own.
#[test]
fn loaded_imports_are_checked_by_the_same_rules_as_stored_ones() {
    let joints = [joint("root", None), joint("root", Some(0))];
    let error = validate_skeleton_content(&joints, std::iter::empty()).unwrap_err().to_string();
    assert!(error.contains("Repeated joint name"), "got {error}");

    let joints = [joint("root", None)];
    let info = SkAnimationInfo {
        duration: 1.0,
        interpolationStyle: InterpolationStyle::Linear,
    };
    let keyframes = [keyframe(0.0, &["elbow"])];
    let error = validate_skeleton_content(&joints, std::iter::once((&info, keyframes.as_slice())))
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown joint"), "got {error}");

    validate_skeleton_content(&joints, std::iter::once((&info, [keyframe(0.0, &["root"])].as_slice()))).unwrap();
}
