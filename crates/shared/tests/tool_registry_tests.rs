use serde_json::json;
use shared::api::protocol_for_chat::{EDITOR_TOOL_REGISTRY, ToolName};

// These lock the schemas and descriptions the model actually sees. The tool bodies are covered by
// browser_editor_tools.rs and game_settings_tools.rs.

#[test]
fn report_issue_tool_schema_requires_description_only() {
    let tool_spec = EDITOR_TOOL_REGISTRY.get_tool(ToolName::ReportIssue).expect("report_issue should be registered");

    assert_eq!(tool_spec.parameters_json["type"], json!("object"));
    assert_eq!(tool_spec.parameters_json["required"], json!(["description"]));
    assert_eq!(tool_spec.parameters_json["additionalProperties"], json!(false));
    assert_eq!(tool_spec.parameters_json["properties"]["description"]["type"], json!("string"));
}

#[test]
fn publish_tool_is_registered_without_arguments() {
    let tool_spec = EDITOR_TOOL_REGISTRY.get_tool(ToolName::Publish).expect("publish should be registered");

    assert_eq!(tool_spec.tool_name.as_ref(), "publish");
    assert_eq!(tool_spec.parameters_json["type"], json!("object"));
    assert_eq!(tool_spec.parameters_json["additionalProperties"], json!(false));
    assert_eq!(tool_spec.parameters_json["properties"], json!({}));
    assert!(tool_spec.description.contains("captures its thumbnail"));
}

#[test]
fn ls_and_grep_tool_schemas_use_clear_filter_names() {
    let ls_tool_spec = EDITOR_TOOL_REGISTRY.get_tool(ToolName::Ls).expect("ls should be registered");
    assert_eq!(ls_tool_spec.parameters_json["required"], json!(["in_object_id"]));
    assert!(ls_tool_spec.parameters_json["properties"].get("in_object_id").is_some());
    assert!(ls_tool_spec.parameters_json["properties"].get("in").is_none());

    let grep_tool_spec = EDITOR_TOOL_REGISTRY.get_tool(ToolName::Grep).expect("grep should be registered");
    assert!(grep_tool_spec.parameters_json["properties"].get("object_type").is_some());
    assert!(grep_tool_spec.parameters_json["properties"].get("type").is_none());
}

// Materials come from assets now, so the built-in material tool is gone and the asset tools cover materials.
#[test]
fn materials_come_from_assets_instead_of_a_built_in_tool() {
    assert!("add_default_material".parse::<ToolName>().is_err());
    let asset_search = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::AssetSearch)
        .expect("asset_search should be registered");
    assert!(asset_search.description.contains("Material"));
    let asset_download = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::AssetDownload)
        .expect("asset_download should be registered");
    assert!(asset_download.description.contains("Material creates a Material"));
}

#[test]
fn read_dirty_scripts_in_workspace_tool_schema_accepts_optional_filters() {
    let tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::ReadDirtyScriptsInWorkspace)
        .expect("read_dirty_scripts_in_workspace should be registered");

    assert_eq!(tool_spec.parameters_json["type"], json!("object"));
    assert_eq!(tool_spec.parameters_json["additionalProperties"], json!(false));
    assert!(tool_spec.parameters_json.get("required").is_none());
    assert_eq!(tool_spec.parameters_json["properties"]["path_substring"]["type"], json!(["string", "null"]));
    assert_eq!(tool_spec.parameters_json["properties"]["include_text"]["type"], json!(["boolean", "null"]));
    assert_eq!(
        tool_spec.parameters_json["properties"]["include_diagnostics"]["type"],
        json!(["boolean", "null"])
    );
    assert_eq!(
        tool_spec.parameters_json["properties"]["only_files_with_diagnostics"]["type"],
        json!(["boolean", "null"])
    );
}

#[test]
fn read_all_diagnostics_tool_schema_accepts_no_args() {
    let tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::ReadAllScriptWarnings)
        .expect("read_all_script_warnings should be registered");

    assert_eq!(tool_spec.parameters_json["type"], json!("object"));
    assert_eq!(tool_spec.parameters_json["additionalProperties"], json!(false));
    assert_eq!(tool_spec.parameters_json["properties"], json!({}));
}

#[test]
fn run_temporary_script_tool_schema_requires_script() {
    let tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::RunTemporaryScript)
        .expect("run_temporary_script should be registered");

    assert_eq!(tool_spec.tool_name.as_ref(), "run_temporary_script");
    assert_eq!(tool_spec.parameters_json["type"], json!("object"));
    assert_eq!(tool_spec.parameters_json["required"], json!(["script"]));
    assert_eq!(tool_spec.parameters_json["additionalProperties"], json!(false));
    assert_eq!(tool_spec.parameters_json["properties"]["script"]["type"], json!("string"));
    assert!(tool_spec.description.contains("temporary server script"));
}

#[test]
fn move_by_recursively_tool_schema_requires_object_id_and_translation() {
    let tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::MoveByRecursively)
        .expect("move_by_recursively should be registered");

    assert_eq!(tool_spec.parameters_json["type"], json!("object"));
    assert_eq!(tool_spec.parameters_json["required"], json!(["object_id", "translation"]));
    assert_eq!(tool_spec.parameters_json["additionalProperties"], json!(false));
    assert_eq!(tool_spec.parameters_json["properties"]["translation"]["$ref"], json!("#/$defs/Vec3Args"));
    assert!(tool_spec.description.contains("world space"));
}

#[test]
fn move_to_recursively_tool_schema_requires_object_id_and_target_point() {
    let tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::MoveToRecursively)
        .expect("move_to_recursively should be registered");

    assert_eq!(tool_spec.parameters_json["type"], json!("object"));
    assert_eq!(tool_spec.parameters_json["required"], json!(["object_id", "target_point"]));
    assert_eq!(tool_spec.parameters_json["additionalProperties"], json!(false));
    assert_eq!(tool_spec.parameters_json["properties"]["target_point"]["$ref"], json!("#/$defs/Vec3Args"));
    assert!(tool_spec.description.contains("root object has no direct physics position"));
}

#[test]
fn physical_primitive_helper_tool_schemas_use_clear_geometry_names() {
    let create_brick_tool_spec = EDITOR_TOOL_REGISTRY.get_tool(ToolName::CreateBrick).expect("create_brick should be registered");
    assert_eq!(create_brick_tool_spec.tool_name.as_ref(), "create_brick");
    assert_eq!(
        create_brick_tool_spec.parameters_json["required"],
        json!(["parent_id", "name", "center", "size"])
    );
    assert_eq!(
        create_brick_tool_spec.parameters_json["properties"]["center"]["$ref"],
        json!("#/$defs/Vec3Args")
    );
    assert_eq!(create_brick_tool_spec.parameters_json["properties"]["size"]["$ref"], json!("#/$defs/Vec3Args"));
    assert!(create_brick_tool_spec.description.contains("rotation in degrees"));

    let create_box_between_corners_tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::CreateBoxBetweenCorners)
        .expect("create_box_between_corners should be registered");
    assert_eq!(create_box_between_corners_tool_spec.tool_name.as_ref(), "create_box_between_corners");
    assert_eq!(
        create_box_between_corners_tool_spec.parameters_json["required"],
        json!(["parent_id", "name", "corner_a", "corner_b"])
    );
    assert!(create_box_between_corners_tool_spec.description.contains("opposite corners"));

    let create_wall_tool_spec = EDITOR_TOOL_REGISTRY.get_tool(ToolName::CreateWall).expect("create_wall should be registered");
    assert_eq!(create_wall_tool_spec.tool_name.as_ref(), "create_wall");
    assert_eq!(
        create_wall_tool_spec.parameters_json["required"],
        json!(["parent_id", "name", "start", "end", "height", "thickness"])
    );
    assert!(create_wall_tool_spec.description.contains("height and thickness"));

    let create_brick_between_points_tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::CreateBrickBetweenPoints)
        .expect("create_brick_between_points should be registered");
    assert_eq!(create_brick_between_points_tool_spec.tool_name.as_ref(), "create_brick_between_points");
    assert_eq!(
        create_brick_between_points_tool_spec.parameters_json["required"],
        json!(["parent_id", "name", "start", "end", "width", "height"])
    );
    assert!(create_brick_between_points_tool_spec.description.contains("local Z"));

    let create_ball_tool_spec = EDITOR_TOOL_REGISTRY.get_tool(ToolName::CreateBall).expect("create_ball should be registered");
    assert_eq!(create_ball_tool_spec.tool_name.as_ref(), "create_ball");
    assert_eq!(
        create_ball_tool_spec.parameters_json["required"],
        json!(["parent_id", "name", "center", "radius"])
    );
    assert!(create_ball_tool_spec.description.contains("center point and radius"));

    let create_cylinder_tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::CreateCylinder)
        .expect("create_cylinder should be registered");
    assert_eq!(create_cylinder_tool_spec.tool_name.as_ref(), "create_cylinder");
    assert_eq!(
        create_cylinder_tool_spec.parameters_json["required"],
        json!(["parent_id", "name", "center", "radius", "length"])
    );
    assert!(create_cylinder_tool_spec.description.contains("local Z"));

    let create_cylinder_between_points_tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::CreateCylinderBetweenPoints)
        .expect("create_cylinder_between_points should be registered");
    assert_eq!(create_cylinder_between_points_tool_spec.tool_name.as_ref(), "create_cylinder_between_points");
    assert_eq!(
        create_cylinder_between_points_tool_spec.parameters_json["required"],
        json!(["parent_id", "name", "start", "end", "radius"])
    );
    assert!(create_cylinder_between_points_tool_spec.description.contains("endpoint centers"));

    for tool_spec in EDITOR_TOOL_REGISTRY.tool_specs() {
        assert_ne!(tool_spec.tool_name.as_ref(), "part2");
        assert_ne!(tool_spec.tool_name.as_ref(), "part3");
    }
}

#[test]
fn ai_headless_playtest_tools_are_registered_with_the_args_the_ai_needs() {
    // starting takes no args: players join one at a time so the ai can also make them leave one at a time
    for tool_name in [
        ToolName::StartAiHeadlessPlaytestServer,
        ToolName::StartAiHeadlessPlaytestClient,
        ToolName::StopAiHeadlessPlaytestAll,
    ] {
        let tool_spec = EDITOR_TOOL_REGISTRY.get_tool(tool_name).expect("playtest tool should be registered");
        assert_eq!(tool_spec.parameters_json["required"], json!(null), "{} should take no args", tool_name.as_ref());
    }

    let stop_client_tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::StopAiHeadlessPlaytestClient)
        .expect("stop_ai_headless_playtest_client should be registered");
    assert_eq!(stop_client_tool_spec.parameters_json["required"], json!(["client_id"]));

    let read_logs_tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::ReadAiHeadlessPlaytestLogs)
        .expect("read_ai_headless_playtest_logs should be registered");
    // every read arg is optional so the ai can just read everything with no cursor
    assert_eq!(read_logs_tool_spec.parameters_json["required"], json!(null));
    for read_arg_name in ["after_seq", "max_bytes", "wait_ms", "source"] {
        assert!(read_logs_tool_spec.parameters_json["properties"][read_arg_name].is_object());
    }
}

#[test]
fn read_logs_tells_the_ai_how_to_write_a_source() {
    // "server" / "client_1" is not guessable, so it has to reach the model. only /// doc comments do.
    let read_logs_tool_spec = EDITOR_TOOL_REGISTRY
        .get_tool(ToolName::ReadAiHeadlessPlaytestLogs)
        .expect("read_ai_headless_playtest_logs should be registered");
    let source_description = read_logs_tool_spec.parameters_json["properties"]["source"]["description"]
        .as_str()
        .expect("source needs a description or the ai cannot guess its format");
    assert!(source_description.contains("server"), "unexpected description: {source_description}");
    assert!(source_description.contains("client_"), "unexpected description: {source_description}");
}
