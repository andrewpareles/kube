// mod helpers;

// use game_types::physics_primitive_types::{BrickPrimitiveSettings, PrimitiveSettings};
// use glam::{Quat, Vec3};
// use helpers::world_test_helpers::{add_ball, add_brick, add_cylinder, add_rigid_group, make_world};
// use serde_json::{Value, json};
// use shared::api::protocol_for_chat::{ToolCall, apply_editor_tool_call};
// use shared::game::data_fetcher::DataFetcherArc;
// use shared::game::physics::{SetPhysicsInfo3D, SetPhysicsToggles3D};
// use shared::game::world::World;
// use std::sync::Arc;
// use tokio::sync::RwLock;

// fn set_walkthrough(world: &mut World, object_id: usize) {
//     world.setPhysicsToggles3D(
//         object_id,
//         SetPhysicsToggles3D {
//             isWalkthrough: Some(true),
//             ..Default::default()
//         },
//         false,
//     );
// }

// // Exercise the public editor tool against real colliders, including curved surfaces and welded assemblies.
// async fn placement_warnings(world: World) -> anyhow::Result<(Vec<usize>, Vec<Value>)> {
//     let result = apply_editor_tool_call(
//         Arc::new(RwLock::new(world)),
//         DataFetcherArc::new(),
//         None,
//         ToolCall {
//             name: "get_placement_warnings".to_string(),
//             arguments: json!({}),
//         },
//     )
//     .await;
//     assert!(!result.is_error, "{}", result.text);
//     let mut result_json: Value = serde_json::from_str(&result.text)?;
//     let floating_ids = result_json["floating_warnings"]
//         .as_array()
//         .expect("floating warnings")
//         .iter()
//         .map(|warning| warning["object_id"].as_u64().expect("object id") as usize)
//         .collect();
//     let collisions = serde_json::from_value(result_json["collision_warnings"].take())?;
//     Ok((floating_ids, collisions))
// }

// #[tokio::test]
// async fn touching_primitives_have_a_warning_free_contact_band() -> anyhow::Result<()> {
//     for (support_index, add_support) in [add_brick, add_ball, add_cylinder].into_iter().enumerate() {
//         for (primitive_index, add_primitive) in [add_brick, add_ball, add_cylinder].into_iter().enumerate() {
//             for floor_y in [0.0, 100.0, 1000.0] {
//                 for gap in [-0.0005, 0.0, 0.0005] {
//                     for (is_support_walkthrough, is_primitive_walkthrough) in [(false, false), (true, false), (false, true), (true, true)] {
//                         let mut world = make_world()?;
//                         let root_id = world.getRootObjectId();
//                         add_support(&mut world, 10, root_id, Vec3::new(0.0, floor_y, 0.0), true)?;
//                         add_primitive(&mut world, 20, root_id, Vec3::new(0.0, floor_y + 2.0 + gap, 0.0), true)?;
//                         if is_support_walkthrough {
//                             set_walkthrough(&mut world, 10);
//                         }
//                         if is_primitive_walkthrough {
//                             set_walkthrough(&mut world, 20);
//                         }
//                         let (floating_ids, collisions) = placement_warnings(world).await?;
//                         assert!(collisions.is_empty(), "floor_y={floor_y}, gap={gap}: {collisions:?}");
//                         assert_eq!(
//                             floating_ids,
//                             vec![10],
//                             "support={support_index}, primitive={primitive_index}, floor_y={floor_y}, gap={gap}"
//                         );
//                     }
//                 }
//             }
//         }
//     }
//     Ok(())
// }

// #[tokio::test]
// async fn thin_objects_keep_support_within_the_contact_tolerance() -> anyhow::Result<()> {
//     for gap in [0.0, 0.0005] {
//         let mut world = make_world()?;
//         let root_id = world.getRootObjectId();
//         add_brick(&mut world, 10, root_id, Vec3::ZERO, false)?;
//         add_brick(&mut world, 20, root_id, Vec3::new(0.0, 0.0001 + gap, 0.0), false)?;
//         for object_id in [10, 20] {
//             world.setPrimitiveSettings(
//                 object_id,
//                 PrimitiveSettings::Brick(BrickPrimitiveSettings {
//                     size: Vec3::new(2.0, 0.0001, 2.0),
//                 }),
//                 false,
//             )?;
//         }
//         let (floating_ids, collisions) = placement_warnings(world).await?;
//         assert_eq!(floating_ids, vec![10]);
//         assert!(collisions.is_empty());
//     }
//     Ok(())
// }

// #[tokio::test]
// async fn real_gaps_and_penetrations_still_warn() -> anyhow::Result<()> {
//     for gap in [-0.02, 0.02] {
//         for is_walkthrough in [false, true] {
//             let mut world = make_world()?;
//             let root_id = world.getRootObjectId();
//             add_brick(&mut world, 10, root_id, Vec3::ZERO, false)?;
//             add_brick(&mut world, 20, root_id, Vec3::new(0.0, 2.0 + gap, 0.0), false)?;
//             if is_walkthrough {
//                 set_walkthrough(&mut world, 10);
//                 set_walkthrough(&mut world, 20);
//             }
//             let (floating_ids, collisions) = placement_warnings(world).await?;
//             if gap > 0.0 {
//                 assert_eq!(floating_ids, vec![10, 20]);
//                 assert!(collisions.is_empty());
//             } else {
//                 assert_eq!(floating_ids, vec![10]);
//                 assert_eq!(collisions.len(), 1);
//                 assert_eq!(collisions[0]["object"]["object_id"], json!(10));
//                 assert_eq!(collisions[0]["colliding_with_object"]["object_id"], json!(20));
//                 assert!((collisions[0]["collision_depth"].as_f64().expect("depth") - 0.02).abs() < 0.0001);
//             }
//         }
//     }
//     Ok(())
// }

// #[tokio::test]
// async fn walkthrough_objects_report_overlaps_and_provide_geometric_support() -> anyhow::Result<()> {
//     let mut world = make_world()?;
//     let root_id = world.getRootObjectId();
//     add_brick(&mut world, 10, root_id, Vec3::ZERO, false)?;
//     add_brick(&mut world, 20, root_id, Vec3::new(0.0, 2.0, 0.0), false)?;
//     add_brick(&mut world, 30, root_id, Vec3::new(0.0, 2.0, 0.0), false)?;
//     for object_id in [10, 30] {
//         set_walkthrough(&mut world, object_id);
//     }
//     let (floating_ids, collisions) = placement_warnings(world).await?;
//     assert_eq!(floating_ids, vec![10]);
//     assert_eq!(collisions.len(), 1);
//     assert_eq!(collisions[0]["object"]["object_id"], json!(20));
//     assert_eq!(collisions[0]["colliding_with_object"]["object_id"], json!(30));
//     assert_eq!(collisions[0]["collision_depth"], json!(2.0));
//     Ok(())
// }

// #[tokio::test]
// async fn rigid_groups_share_support_and_report_unsupported_assemblies_once() -> anyhow::Result<()> {
//     for is_supported in [false, true] {
//         let mut world = make_world()?;
//         let root_id = world.getRootObjectId();
//         add_rigid_group(&mut world, 10, root_id, Vec3::ZERO, None)?;
//         add_brick(&mut world, 20, 10, Vec3::ZERO, false)?;
//         add_brick(&mut world, 30, 10, Vec3::new(0.0, 2.0, 0.0), false)?;
//         // A welded piece does not need its own surface below it, even inside a nested group.
//         add_rigid_group(&mut world, 40, 10, Vec3::ZERO, None)?;
//         add_brick(&mut world, 50, 40, Vec3::new(4.0, 2.0, 0.0), false)?;
//         if is_supported {
//             add_brick(&mut world, 60, root_id, Vec3::new(0.0, -2.0, 0.0), false)?;
//         }
//         let (floating_ids, collisions) = placement_warnings(world).await?;
//         assert_eq!(floating_ids, if is_supported { vec![60] } else { vec![10] });
//         assert!(collisions.is_empty());
//     }
//     Ok(())
// }

// #[tokio::test]
// async fn empty_groups_do_not_warn_but_unsupported_walkthrough_groups_do() -> anyhow::Result<()> {
//     let mut world = make_world()?;
//     let root_id = world.getRootObjectId();
//     add_rigid_group(&mut world, 10, root_id, Vec3::ZERO, None)?;
//     add_rigid_group(&mut world, 20, root_id, Vec3::ZERO, None)?;
//     add_ball(&mut world, 30, 20, Vec3::ZERO, false)?;
//     set_walkthrough(&mut world, 30);
//     let (floating_ids, collisions) = placement_warnings(world).await?;
//     assert_eq!(floating_ids, vec![20]);
//     assert!(collisions.is_empty());
//     Ok(())
// }

// #[tokio::test]
// async fn walkthrough_group_members_share_geometric_support_with_the_assembly() -> anyhow::Result<()> {
//     let mut world = make_world()?;
//     let root_id = world.getRootObjectId();
//     add_brick(&mut world, 10, root_id, Vec3::ZERO, false)?;
//     add_rigid_group(&mut world, 20, root_id, Vec3::ZERO, None)?;
//     add_brick(&mut world, 30, 20, Vec3::new(0.0, 2.0, 0.0), false)?;
//     add_brick(&mut world, 40, 20, Vec3::new(0.0, 4.0, 0.0), false)?;
//     set_walkthrough(&mut world, 30);
//     let (floating_ids, collisions) = placement_warnings(world).await?;
//     assert_eq!(floating_ids, vec![10]);
//     assert!(collisions.is_empty());
//     Ok(())
// }

// #[tokio::test]
// async fn walkthrough_siblings_neither_self_support_nor_report_internal_overlaps() -> anyhow::Result<()> {
//     let mut world = make_world()?;
//     let root_id = world.getRootObjectId();
//     add_rigid_group(&mut world, 10, root_id, Vec3::ZERO, None)?;
//     add_brick(&mut world, 20, 10, Vec3::ZERO, false)?;
//     add_brick(&mut world, 30, 10, Vec3::ZERO, false)?;
//     set_walkthrough(&mut world, 20);
//     set_walkthrough(&mut world, 30);
//     let (floating_ids, collisions) = placement_warnings(world).await?;
//     assert_eq!(floating_ids, vec![10]);
//     assert!(collisions.is_empty());
//     Ok(())
// }

// #[tokio::test]
// async fn walls_and_ceilings_do_not_count_as_support() -> anyhow::Result<()> {
//     for (offset, expected_floating_ids) in [(Vec3::X * 2.0, vec![10, 20]), (Vec3::Y * 2.0, vec![10])] {
//         let mut world = make_world()?;
//         let root_id = world.getRootObjectId();
//         add_brick(&mut world, 10, root_id, Vec3::ZERO, false)?;
//         add_brick(&mut world, 20, root_id, offset, false)?;
//         let (floating_ids, collisions) = placement_warnings(world).await?;
//         assert_eq!(floating_ids, expected_floating_ids);
//         assert!(collisions.is_empty());
//     }
//     Ok(())
// }

// #[tokio::test]
// async fn touching_walls_do_not_hide_support_below() -> anyhow::Result<()> {
//     for is_walkthrough in [false, true] {
//         let mut world = make_world()?;
//         let root_id = world.getRootObjectId();
//         add_brick(&mut world, 10, root_id, Vec3::ZERO, false)?;
//         world.setPrimitiveSettings(
//             10,
//             PrimitiveSettings::Brick(BrickPrimitiveSettings {
//                 size: Vec3::new(10.0, 2.0, 10.0),
//             }),
//             false,
//         )?;
//         add_brick(&mut world, 20, root_id, Vec3::new(0.0, 2.0, 0.0), false)?;
//         add_brick(&mut world, 30, root_id, Vec3::new(2.0, 2.0, 0.0), false)?;
//         if is_walkthrough {
//             for object_id in [10, 20, 30] {
//                 set_walkthrough(&mut world, object_id);
//             }
//         }
//         let (floating_ids, collisions) = placement_warnings(world).await?;
//         assert_eq!(floating_ids, vec![10]);
//         assert!(collisions.is_empty());
//     }
//     Ok(())
// }

// #[tokio::test]
// async fn rotated_surface_supports_a_touching_ball() -> anyhow::Result<()> {
//     let mut world = make_world()?;
//     let root_id = world.getRootObjectId();
//     let rotation = Quat::from_rotation_z(30_f32.to_radians());
//     add_brick(&mut world, 10, root_id, Vec3::ZERO, false)?;
//     world.forcePhysicsInfo3D(
//         10,
//         SetPhysicsInfo3D {
//             rotation: Some(rotation),
//             ..Default::default()
//         },
//         false,
//     );
//     add_ball(&mut world, 20, root_id, rotation * (Vec3::Y * 2.0), false)?;
//     let (floating_ids, collisions) = placement_warnings(world).await?;
//     assert_eq!(floating_ids, vec![10]);
//     assert!(collisions.is_empty(), "{collisions:?}");
//     Ok(())
// }
