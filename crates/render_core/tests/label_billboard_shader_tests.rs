// Run the production billboard shader on a graphics adapter, including the old steep-angle snap:
// cargo test -p render_core --test label_billboard_shader_tests -- --ignored

use bytemuck::Zeroable;
use game_types::object_types::label::CONSTANT_SIZE_REFERENCE_DISTANCE;
use game_types::object_types::shared::Orientation3D;
use glam::{Mat4, Quat, Vec3, camera::rh};
use render_core::render_scene::{CameraMatrixBufferContent, InstanceContent, instance_model_for_view};
use wgpu::util::DeviceExt;

#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn billboards_stay_upright_across_steep_angles_and_shadow_passes() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();

    // Read back actual model matrices for every combination of billboard and constant-size flags.
    let shader_source = format!(
        "{}\n{}",
        include_str!("../src/scene.wgsl"),
        r#"
        @group(0) @binding(1) var<uniform> test_model: mat4x4<f32>;
        @group(0) @binding(2) var<storage, read_write> results: array<mat4x4<f32>, 4>;
        @compute @workgroup_size(1)
        fn test_billboard(@builtin(global_invocation_id) id: vec3<u32>) {
            var i: InstanceContent;
            i.m0 = test_model[0];
            i.m1 = test_model[1];
            i.m2 = test_model[2];
            i.m3 = test_model[3];
            i.instance_flags = id.x;
            results[id.x] = build_model_matrix(i);
        }
        "#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Billboard orientation regression"),
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("test_billboard"),
        compilation_options: Default::default(),
        cache: None,
    });
    let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: size_of::<CameraMatrixBufferContent>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let model_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&Mat4::IDENTITY.to_cols_array()),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let result_size = (4 * size_of::<[[f32; 4]; 4]>()) as u64;
    let result_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: result_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: result_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: model_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: result_buffer.as_entire_binding(),
            },
        ],
    });

    // Cross the former abs(forward.y) > 0.999 boundary and reach both limits using the real camera clamp.
    let projection = rh::proj::directx::perspective_infinite_reverse(60.0_f32.to_radians(), 1.7, 0.1);
    let scale = Vec3::new(2.0, 3.0, 0.5);
    let authored_rotation = Quat::from_rotation_z(0.4);
    for yaw_degrees in [0.0_f32, 45.0, 90.0, 180.0, 270.0] {
        for pitch_degrees in [-90.0_f32, -87.5, -87.4, -45.0, 0.0, 45.0, 87.4, 87.5, 90.0] {
            let mut camera_orientation = Orientation3D::Z();
            camera_orientation.rotate_as_camera(yaw_degrees.to_radians(), pitch_degrees.to_radians());
            let camera_rotation = Quat::from_rotation_y(camera_orientation.theta) * Quat::from_rotation_x(camera_orientation.phi);
            let camera_position = camera_rotation * Vec3::new(0.0, 0.0, 15.0);
            let camera_world = Mat4::from_rotation_translation(camera_rotation, camera_position);
            let view_proj = projection * camera_world.inverse();
            let view_clip_w_row = view_proj.transpose().w_axis;

            // Offset labels can cross directly above or below the camera independently of its aim.
            // A label at the camera position must also produce a finite matrix.
            for position in [Vec3::ZERO, Vec3::new(2.0, 3.0, -1.0), camera_position] {
                let model = Mat4::from_scale_rotation_translation(scale, authored_rotation, position);
                queue.write_buffer(&model_buffer, 0, bytemuck::cast_slice(&model.to_cols_array()));
                for is_shadow_pass in [false, true] {
                    let pass_view_proj = if is_shadow_pass {
                        rh::proj::directx::orthographic(-20.0, 20.0, -20.0, 20.0, 0.1, 100.0)
                            * rh::view::look_at_mat4(Vec3::new(8.0, 12.0, 6.0), Vec3::ZERO, Vec3::Y)
                    } else {
                        view_proj
                    };
                    let camera_content = CameraMatrixBufferContent {
                        view_proj: pass_view_proj.to_cols_array_2d(),
                        inverse_view_proj: pass_view_proj.inverse().to_cols_array_2d(),
                        camera_position: camera_position.extend(1.0).to_array(),
                        view_clip_w_row: view_clip_w_row.to_array(),
                    };
                    queue.write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera_content));
                    let mut encoder = device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&pipeline);
                        pass.set_bind_group(0, &bind_group, &[]);
                        pass.dispatch_workgroups(4, 1, 1);
                    }
                    encoder.copy_buffer_to_buffer(&result_buffer, 0, &readback, 0, result_size);
                    queue.submit([encoder.finish()]);
                    let (result_tx, result_rx) = tokio::sync::oneshot::channel();
                    readback.slice(..).map_async(wgpu::MapMode::Read, move |result| {
                        result_tx.send(result).unwrap();
                    });
                    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    result_rx.await.unwrap().unwrap();

                    // Billboard axes must stay aligned with the view; surface labels keep their rotation.
                    // This also checks scale, translation, handedness, and constant-size depth in both passes.
                    {
                        let mapped = readback.slice(..).get_mapped_range();
                        let matrices: &[[[f32; 4]; 4]] = bytemuck::cast_slice(&mapped);
                        for (flags, columns) in matrices.iter().enumerate() {
                            let actual = Mat4::from_cols_array_2d(columns);
                            let cpu_instance = InstanceContent {
                                model: model.to_cols_array_2d(),
                                instance_flags: flags as u32,
                                ..InstanceContent::zeroed()
                            };
                            assert!(
                                actual.abs_diff_eq(instance_model_for_view(&cpu_instance, view_proj), 1e-4),
                                "CPU picking transform diverged from the displayed shader geometry"
                            );
                            assert!(actual.is_finite(), "invalid matrix at yaw={yaw_degrees}, pitch={pitch_degrees}, flags={flags}");
                            let rotation = if flags & 1 != 0 { camera_rotation } else { authored_rotation };
                            let scale_multiplier = if flags & 2 != 0 {
                                view_clip_w_row.dot(position.extend(1.0)).abs().max(1e-4) / CONSTANT_SIZE_REFERENCE_DISTANCE
                            } else {
                                1.0
                            };
                            let expected = Mat4::from_scale_rotation_translation(scale * scale_multiplier, rotation, position);
                            assert!(
                                actual.abs_diff_eq(expected, 1e-4),
                                "billboard snapped or changed scale: yaw={yaw_degrees}, pitch={pitch_degrees}, position={position:?}, flags={flags}, shadow={is_shadow_pass}\nactual={actual:?}\nexpected={expected:?}"
                            );
                        }
                    }
                    readback.unmap();
                }
            }
        }
    }
}
