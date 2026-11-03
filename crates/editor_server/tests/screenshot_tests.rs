use editor_server::{
    playtest_process::{PlaytestProcess, PlaytestRequest},
    screenshot::{ScreenshotRequest, draw_png_in_child_process},
};
use game_types::{
    object_types::{BrickInfo, ObjectBeingAdded, ObjectLocation, PhysicsInfo3D, PhysicsToggles3D, ScriptInfo, VisualSettings, shared::Color3},
    physics_primitive_types::BrickPrimitiveSettings,
};
use glam::{Quat, Vec3};
use indexmap::IndexMap;
use render_core::{camera::DEFAULT_CAMERA_FOV_DEGREES, renderable::Renderable};
use shared::game::{
    data_fetcher::DataFetcherArc,
    gui_types::{GuiElementKind, GuiRenderedRoot, GuiSerializedElementNode, GuiSerializedNode},
    world::{World, WorldControlMode},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::RwLock;

fn red_brick_world() -> anyhow::Result<World> {
    let mut world = World::fromStoredWorldString(include_str!("../../../dev-demo.kube"), WorldControlMode::Editor, None)?;
    let root = world.getRootObjectId();
    let child_ids: Vec<_> = world.getDirectChildrenIds(root)?.iter().copied().collect();
    for child_id in child_ids {
        world.removeObject(child_id)?;
    }
    world.addObject(
        "red brick".into(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings {
                    color: Color3 { r: 1.0, g: 0.0, b: 0.0 },
                    ..VisualSettings::TYPICAL_OPAQUE
                },
            },
            BrickPrimitiveSettings { size: Vec3::splat(2.0) },
            PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        root,
    );
    Ok(world)
}

fn is_red(pixel: &image::Rgba<u8>) -> bool {
    let [r, g, b, _] = pixel.0;
    u16::from(r) > 2 * u16::from(g) && u16::from(r) > 2 * u16::from(b)
}

async fn draw_in_child_process(world_arc: &RwLock<World>, camera_position: Vec3, target: Vec3) -> anyhow::Result<image::RgbaImage> {
    let request = ScreenshotRequest::of_editor_world(world_arc, &DataFetcherArc::new(), camera_position, target, 1000, 500).await?;
    let png = draw_png_in_child_process(Path::new(env!("CARGO_BIN_EXE_editor_server")), &request).await?;
    Ok(image::load_from_memory(&png)?.to_rgba8())
}

// A width of 1000 pixels is not a whole number of 256 byte rows, so the readback has to cut padding.
#[tokio::test(flavor = "multi_thread")]
async fn a_screenshot_draws_the_editor_world_from_the_given_camera() -> anyhow::Result<()> {
    let world_arc = RwLock::new(red_brick_world()?);
    let camera_position = Vec3::new(0.0, 0.0, 6.0);

    let facing = draw_in_child_process(&world_arc, camera_position, Vec3::ZERO).await?;
    assert_eq!(facing.dimensions(), (1000, 500));
    assert!(is_red(facing.get_pixel(500, 250)), "{:?}", facing.get_pixel(500, 250));
    assert!(!is_red(facing.get_pixel(5, 5)), "{:?}", facing.get_pixel(5, 5));
    assert!(!is_red(facing.get_pixel(994, 494)), "{:?}", facing.get_pixel(994, 494));

    let away = draw_in_child_process(&world_arc, camera_position, Vec3::new(0.0, 0.0, 12.0)).await?;
    assert!(!is_red(away.get_pixel(500, 250)), "{:?}", away.get_pixel(500, 250));
    Ok(())
}

// A drawing process that dies returns an error to the tool, and the editor keeps running.
#[tokio::test(flavor = "multi_thread")]
async fn a_screenshot_process_that_dies_fails_the_screenshot() -> anyhow::Result<()> {
    let world_arc = RwLock::new(red_brick_world()?);
    let request = ScreenshotRequest::of_editor_world(&world_arc, &DataFetcherArc::new(), Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, 100, 100).await?;
    assert!(draw_png_in_child_process(Path::new("/usr/bin/false"), &request).await.is_err());
    Ok(())
}

fn is_white(pixel: &image::Rgba<u8>) -> bool {
    pixel.0[..3].iter().all(|channel| *channel > 240)
}

// The GUI is drawn by the play webview's page in headless Chromium and laid over the world, with the world
// showing through wherever the page draws nothing.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Chromium at CHROMIUM_PATH and a graphics adapter; run with --ignored"]
async fn a_player_screenshot_draws_their_gui_over_the_world() -> anyhow::Result<()> {
    let world = red_brick_world()?;
    let camera_position = Vec3::new(0.0, 0.0, 6.0);
    let gui_root_of_script_gui_key = IndexMap::from([(
        (1, "hud".to_string()),
        GuiRenderedRoot {
            children: vec![GuiSerializedNode::Element(GuiSerializedElementNode {
                element_kind: GuiElementKind::Div,
                node_id: "corner".to_string(),
                class_name: "absolute right-0 bottom-0 size-[100px] bg-white".to_string(),
                on_click_handler_id: None,
                on_right_click_handler_id: None,
                on_mouse_down_handler_id: None,
                on_mouse_up_handler_id: None,
                on_hover_handler_id: None,
                children: Vec::new(),
            })],
        },
    )]);
    let request = ScreenshotRequest::new(
        &DataFetcherArc::new(),
        world.getRenderSnapshot().render_object_of_id,
        HashMap::new(),
        Some(gui_root_of_script_gui_key),
        camera_position,
        Vec3::ZERO,
        DEFAULT_CAMERA_FOV_DEGREES,
        800,
        400,
    )
    .await?;
    let png = draw_png_in_child_process(Path::new(env!("CARGO_BIN_EXE_editor_server")), &request).await?;
    let image = image::load_from_memory(&png)?.to_rgba8();
    assert!(is_white(image.get_pixel(760, 360)), "{:?}", image.get_pixel(760, 360));
    assert!(!is_white(image.get_pixel(640, 240)), "{:?}", image.get_pixel(640, 240));
    assert!(is_red(image.get_pixel(400, 200)), "{:?}", image.get_pixel(400, 200));
    Ok(())
}

// All 40 games the AI built on 2026-10-13 reported that the playtest screenshot never showed their client scripts' GUI.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Chromium at CHROMIUM_PATH and a graphics adapter; run with --ignored"]
async fn a_headless_playtest_player_screenshot_shows_the_gui_their_client_script_shows() -> anyhow::Result<()> {
    let mut world = red_brick_world()?;
    let root = world.getRootObjectId();
    let script_id = world.addObject(
        "hud".into(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root,
    );
    world.setScriptRawCode(
        script_id,
        r#"game.guiService.show("corner", <div className="absolute right-0 bottom-0 size-[100px] bg-white" />);"#.into(),
    );
    let logs = Arc::new(Mutex::new(Vec::new()));
    let logs_to_write = logs.clone();
    let (process, _) = PlaytestProcess::start(
        Path::new(env!("CARGO_BIN_EXE_editor_server")),
        world.toStoredWorldString()?,
        None,
        move |source, level, message| logs_to_write.lock().unwrap().push(format!("{} {level}: {message}", source.to_text())),
    )
    .await?;
    let client_id: u8 = process.request(PlaytestRequest::StartClient).await?;
    let mut corner_pixel = None;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let png: Vec<u8> = process
            .request(PlaytestRequest::CaptureScreenshot {
                client_id,
                width: 800,
                height: 400,
            })
            .await?;
        let pixel = *image::load_from_memory(&png)?.to_rgba8().get_pixel(760, 360);
        corner_pixel = Some(pixel);
        if is_white(&pixel) {
            break;
        }
    }
    process.stop().await;
    let corner_pixel = corner_pixel.unwrap();
    assert!(is_white(&corner_pixel), "{corner_pixel:?}\n{}", logs.lock().unwrap().join("\n"));
    Ok(())
}
