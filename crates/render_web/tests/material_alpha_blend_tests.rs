#[path = "../../render_core/tests/helpers/render_capture.rs"]
mod render_capture;

// Exercise the production static and skinned pipelines, including full-opacity Blend materials.
use game_types::{
    consts::DEHYDRATED_GLTF_CURRENT_VERSION,
    data_types::{MeshData, MeshVertexInternal, SkinningInfoPerVertex},
    object_types::{Location3D, MaterialInfo, ObjectInfo, material::AlphaMode},
};
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
    computeSkeletonPose::computeSkeletonPoseOfIdAtTime,
    dehydrated_gltf::{DehydratedGltf, DehydratedMaterial, DehydratedMesh, DehydratedSkeleton},
    dehydrated_gltf_to_stored_world::dehydrated_gltf_to_stored_world,
    render_scene::{RenderGeometry, RenderInstance, RenderScene, build_render_scene},
    renderable::Renderable,
    renderer::Renderer,
    renderer_types::RenderSkJoint,
};
use render_web::stored_world_renderable_impl::StoredWorldRenderable;
use std::{collections::HashMap, sync::Arc};

async fn capture(renderer: &mut Renderer, scene: &RenderScene) -> Vec<u8> {
    let camera_position = Vec3::new(0.0, 0.0, 4.0);
    let (_, camera) = renderer.create_camera_matrix_buffer_pair(camera_position, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(renderer, 128, 128);
    renderer.render_scene(
        &mut frame,
        scene,
        None,
        &camera,
        Vec3::ZERO,
        camera_position,
        build_camera_view_proj_matrix(camera_position, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES),
    );
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Alpha blend readback"),
        size: 128 * 128 * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    frame.encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(512),
                rows_per_image: Some(128),
            },
        },
        texture.size(),
    );
    renderer.finish_frame(frame);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).unwrap();
    });
    renderer.device().poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.await.unwrap().unwrap();
    let pixels = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();
    pixels
}

#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn blend_preserves_texture_alpha_opacity_and_selection_silhouettes() -> anyhow::Result<()> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, 128, 128)?;
    for is_skinned in [false, true] {
        let positions = [
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(-1.0, 1.0, 0.0),
        ];
        let indices = vec![0, 1, 2, 0, 2, 3];
        let mesh = if is_skinned {
            MeshData::SkinnedMeshData {
                vertices: positions
                    .into_iter()
                    .map(|position| MeshVertexInternal {
                        position,
                        normal: Vec3::Z,
                        uv: Vec2::splat(0.5),
                        tangent: Vec4::X,
                        skinning: SkinningInfoPerVertex {
                            jointIndices: [0; 4],
                            jointWeights: [1.0, 0.0, 0.0, 0.0],
                        },
                    })
                    .collect(),
                indices,
                jointNameOfIdx: vec!["joint".into()],
                inverseBindMatrixOfJointName: HashMap::from([("joint".into(), Mat4::IDENTITY.to_cols_array())]),
            }
        } else {
            MeshData::StaticMeshData {
                vertices: positions
                    .into_iter()
                    .map(|position| MeshVertexInternal {
                        position,
                        normal: Vec3::Z,
                        uv: Vec2::splat(0.5),
                        tangent: Vec4::X,
                        skinning: (),
                    })
                    .collect(),
                indices,
            }
        };
        renderer.load_mesh_into_gpu(1, &mesh);
        let mesh_data_of_id = HashMap::from([(1, Arc::new(mesh))]);
        for is_always_on_top in [false, true] {
            let mut opaque_red = 0.0;
            renderer.load_texture_into_gpu(3, &[vec![0, 0, 255, 255]], 1, 1, wgpu::TextureFormat::Rgba8Unorm, true)?;
            for (alpha_mode, texture_alpha, opacity, expected_alpha) in [
                (AlphaMode::Opaque, 0, 1.0, 255_u8),
                (AlphaMode::Mask { cutoff: 0.5 }, 64, 1.0, 0),
                (AlphaMode::Blend, 0, 1.0, 0),
                (AlphaMode::Blend, 64, 1.0, 64),
                (AlphaMode::Blend, 128, 1.0, 128),
                (AlphaMode::Blend, 255, 1.0, 255),
                (AlphaMode::Blend, 128, 0.5, 64),
            ] {
                renderer.load_texture_into_gpu(2, &[vec![255, 80, 30, texture_alpha]], 1, 1, wgpu::TextureFormat::Rgba8Unorm, true)?;
                let mut stored = dehydrated_gltf_to_stored_world(
                    "Alpha blend",
                    DehydratedGltf {
                        version: DEHYDRATED_GLTF_CURRENT_VERSION,
                        blender_id: None,
                        import_scale: 1.0,
                        dehydrated_meshes: vec![DehydratedMesh {
                            name: "mesh".to_string(),
                            mesh_data_id: 1,
                            skinned_skeleton_index: if is_skinned { Some(0) } else { None },
                            material: Some(DehydratedMaterial {
                                name: "material".to_string(),
                                material_info: MaterialInfo {
                                    albedoTextureDataId: Some(2),
                                    metallicTextureDataId: None,
                                    roughnessTextureDataId: None,
                                    normalTextureDataId: None,
                                    alphaMode: alpha_mode,
                                    isDoubleSided: false,
                                },
                            }),
                        }],
                        dehydrated_skeletons: if is_skinned {
                            vec![DehydratedSkeleton {
                                skeleton_index: 0,
                                animations: Vec::new(),
                                jointOfIndex: vec![RenderSkJoint {
                                    name: "joint".into(),
                                    parent_joint_index: None,
                                    rest_pose: Location3D {
                                        position: Vec3::ZERO,
                                        quaternion: Quat::IDENTITY,
                                    },
                                }],
                            }]
                        } else {
                            Vec::new()
                        },
                    },
                )?;
                for object in stored.objectOfId.values_mut() {
                    if let ObjectInfo::Mesh(mesh) = &mut object.info {
                        mesh.visualSettings.opacity = opacity;
                        mesh.visualSettings.isAlwaysOnTop = is_always_on_top;
                    }
                }
                let root_id = stored.rootObjectId;
                let source = StoredWorldRenderable::new(stored)?;
                let objects = source.getRenderSnapshot().render_object_of_id;
                let poses = computeSkeletonPoseOfIdAtTime(&objects, 0.0, Some(0.0));
                let mut scene = build_render_scene(&objects, &mesh_data_of_id, Some(&poses));
                scene.should_draw_sky = false;
                scene.lighting.sky_horizon_color_and_padding = [0.0; 4];
                scene.lighting.sun_color_and_intensity = [0.0; 4];
                scene.lighting.ambient_top_color_and_intensity = [1.0, 1.0, 1.0, 0.5];
                scene.lighting.ambient_bottom_color_and_padding = [1.0, 1.0, 1.0, 0.0];
                let pixels = capture(&mut renderer, &scene).await;
                // A selected import outlines its visible mesh descendants, including skinned and front-layer meshes.
                // Fully transparent/cutout texels must stay absent, and clearing selection must leave no stale mask.
                scene.selected_object_ids = source.object_and_descendant_ids(root_id);
                let selected_pixels = capture(&mut renderer, &scene).await;
                let blue_outline_pixels = selected_pixels
                    .chunks_exact(4)
                    .zip(pixels.chunks_exact(4))
                    .filter(|(selected, plain)| selected != plain && selected[2] > selected[0] && selected[2] > selected[1])
                    .count();
                if expected_alpha == 0 {
                    assert_eq!(selected_pixels, pixels, "Invisible texels acquired an outline");
                } else {
                    assert!(
                        blue_outline_pixels > 0,
                        "Missing outline: {alpha_mode:?}, skinned {is_skinned}, front layer {is_always_on_top}"
                    );
                    let center = (64 * 128 + 64) * 4;
                    assert_eq!(
                        &selected_pixels[center..center + 4],
                        &pixels[center..center + 4],
                        "Selection filled the object instead of outlining it"
                    );
                }
                scene.selected_object_ids.clear();
                assert_eq!(capture(&mut renderer, &scene).await, pixels, "Cleared selection left an outline");
                // With no sky the picture is see-through, so its colors are premultiplied in sRGB, the way the page blends it.
                let red = f32::from(pixels[(64 * 128 + 64) * 4]) / 255.0;
                if let AlphaMode::Opaque = alpha_mode {
                    opaque_red = red;
                    assert!(opaque_red > 0.05);
                }
                let expected_red = opaque_red * f32::from(expected_alpha) / 255.0;
                assert!(
                    (red - expected_red).abs() <= 0.02,
                    "{alpha_mode:?}: texture alpha {texture_alpha}, opacity {opacity}, skinned {is_skinned}, always-on-top {is_always_on_top}: expected sRGB red {expected_red}, got {red}"
                );
                // A differently colored rear wing must never leak through fully opaque texels.
                // Partial texels must still reveal it, for both instance layers and mesh pipelines.
                // A see-through rear wing is depth tested against the opaque depth replay, so it checks the replay too.
                assert_eq!(scene.instances.len(), 1);
                let pixel = (64 * 128 + 64) * 4;
                for rear_opacity in [1.0, 0.5] {
                    let front = &scene.instances[0];
                    let RenderGeometry::Mesh { data_id, skeleton_id } = front.geometry else {
                        unreachable!()
                    };
                    let mut rear_content = front.content;
                    rear_content.model = (Mat4::from_translation(Vec3::new(0.0, 0.0, -0.5)) * Mat4::from_cols_array_2d(&rear_content.model)).to_cols_array_2d();
                    rear_content.color = [1.0, 1.0, 1.0, rear_opacity];
                    let mut rear_material = front.material;
                    rear_material.basecolor_texture_data_id = Some(3);
                    scene.instances.push(RenderInstance {
                        object_id: usize::MAX,
                        geometry: RenderGeometry::Mesh { data_id, skeleton_id },
                        content: rear_content,
                        material: rear_material,
                        is_always_on_top,
                    });
                    let layered_pixels = capture(&mut renderer, &scene).await;
                    if expected_alpha == 255 {
                        for channel in 0..3 {
                            assert!(
                                pixels[pixel + channel].abs_diff(layered_pixels[pixel + channel]) <= 1,
                                "Rear wing leaked through {alpha_mode:?}, skinned {is_skinned}, always-on-top {is_always_on_top}, rear opacity {rear_opacity}"
                            );
                        }
                    } else {
                        assert!(
                            layered_pixels[pixel + 2] > pixels[pixel + 2] + 20,
                            "Partial/empty texel blocked the rear wing: {alpha_mode:?}, alpha {texture_alpha}, opacity {opacity}, rear opacity {rear_opacity}"
                        );
                    }
                    // Texels that let the rear wing show through must let sunlight through to it too.
                    if expected_alpha == 0 && rear_opacity == 1.0 && !is_always_on_top {
                        scene.lighting.sun_direction = [0.0, 0.0, 1.0, 0.0];
                        scene.lighting.sun_color_and_intensity = [1.0; 4];
                        let sunlit_through_front = capture(&mut renderer, &scene).await;
                        let front = scene.instances.remove(0);
                        let sunlit_alone = capture(&mut renderer, &scene).await;
                        scene.instances.insert(0, front);
                        scene.lighting.sun_color_and_intensity = [0.0; 4];
                        for channel in 0..3 {
                            assert!(
                                sunlit_through_front[pixel + channel].abs_diff(sunlit_alone[pixel + channel]) <= 1,
                                "Empty texels shaded the rear wing: {alpha_mode:?}, skinned {is_skinned}: {:?} vs {:?}",
                                &sunlit_through_front[pixel..pixel + 3],
                                &sunlit_alone[pixel..pixel + 3]
                            );
                        }
                    }
                    scene.instances.pop();
                }

                if let (AlphaMode::Blend, 255, 1.0) = (alpha_mode, texture_alpha, opacity) {
                    // Back faces are culled by default. Double-sided materials draw them with flipped
                    // normals, so the reversed wing receives the same front-directed light as before.
                    scene.lighting.sun_direction = [0.0, 0.0, 1.0, 0.0];
                    scene.lighting.sun_color_and_intensity = [1.0; 4];
                    let lit_front = capture(&mut renderer, &scene).await;
                    let front = &mut scene.instances[0];
                    front.content.model = (Mat4::from_rotation_y(std::f32::consts::PI) * Mat4::from_cols_array_2d(&front.content.model)).to_cols_array_2d();
                    let culled_back = capture(&mut renderer, &scene).await;
                    assert_eq!(&culled_back[pixel..pixel + 3], &[0, 0, 0]);
                    scene.instances[0].material.is_double_sided = true;
                    let lit_back = capture(&mut renderer, &scene).await;
                    for channel in 0..3 {
                        assert!(
                            lit_front[pixel + channel].abs_diff(lit_back[pixel + channel]) <= 3,
                            "Double-sided lighting changed, skinned {is_skinned}, always-on-top {is_always_on_top}: {:?} vs {:?}",
                            &lit_front[pixel..pixel + 3],
                            &lit_back[pixel..pixel + 3]
                        );
                    }
                }
            }
        }
    }
    assert!(renderer.device().pop_error_scope().await.is_none());
    Ok(())
}

#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn see_through_edges_are_no_brighter_than_they_are_covered() -> anyhow::Result<()> {
    // A bright diamond with no sky, so its slanted edges are partly covered. The page blends the picture premultiplied in sRGB, and an
    // edge pixel brighter than it is covered draws a light outline over whatever is behind the canvas.
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, 128, 128)?;
    let mesh = MeshData::StaticMeshData {
        vertices: [
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
        ]
        .into_iter()
        .map(|position| MeshVertexInternal {
            position,
            normal: Vec3::Z,
            uv: Vec2::splat(0.5),
            tangent: Vec4::X,
            skinning: (),
        })
        .collect(),
        indices: vec![0, 1, 2, 0, 2, 3],
    };
    renderer.load_mesh_into_gpu(1, &mesh);
    renderer.load_texture_into_gpu(2, &[vec![255, 255, 255, 255]], 1, 1, wgpu::TextureFormat::Rgba8Unorm, true)?;
    let stored = dehydrated_gltf_to_stored_world(
        "Diamond",
        DehydratedGltf {
            version: DEHYDRATED_GLTF_CURRENT_VERSION,
            blender_id: None,
            import_scale: 1.0,
            dehydrated_meshes: vec![DehydratedMesh {
                name: "mesh".to_string(),
                mesh_data_id: 1,
                skinned_skeleton_index: None,
                material: Some(DehydratedMaterial {
                    name: "material".to_string(),
                    material_info: MaterialInfo {
                        albedoTextureDataId: Some(2),
                        metallicTextureDataId: None,
                        roughnessTextureDataId: None,
                        normalTextureDataId: None,
                        alphaMode: AlphaMode::Opaque,
                        isDoubleSided: false,
                    },
                }),
            }],
            dehydrated_skeletons: Vec::new(),
        },
    )?;
    let objects = StoredWorldRenderable::new(stored)?.getRenderSnapshot().render_object_of_id;
    let mut scene = build_render_scene(&objects, &HashMap::from([(1, Arc::new(mesh))]), None);
    scene.should_draw_sky = false;
    scene.lighting.sky_horizon_color_and_padding = [0.0; 4];
    scene.lighting.sun_color_and_intensity = [0.0; 4];
    scene.lighting.ambient_top_color_and_intensity = [1.0, 1.0, 1.0, 4.0];
    scene.lighting.ambient_bottom_color_and_padding = [1.0, 1.0, 1.0, 0.0];
    let pixels = capture(&mut renderer, &scene).await;
    let edge_pixels: Vec<&[u8]> = pixels.chunks_exact(4).filter(|pixel| pixel[3] > 0 && pixel[3] < 255).collect();
    assert!(edge_pixels.len() > 20, "The diamond's edges are not antialiased");
    let center = (64 * 128 + 64) * 4;
    assert!(
        pixels[center..center + 3].iter().all(|&channel| channel > 200),
        "The diamond is not bright: {:?}",
        &pixels[center..center + 4]
    );
    for pixel in edge_pixels {
        assert!(
            pixel[..3].iter().all(|&channel| channel <= pixel[3].saturating_add(1)),
            "An edge pixel is brighter than it is covered: {pixel:?}"
        );
    }
    assert!(renderer.device().pop_error_scope().await.is_none());
    Ok(())
}
