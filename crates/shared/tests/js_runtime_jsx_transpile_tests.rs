use game_types::object_types::ObjectLocation;
use shared::game::transpileJsScript::transpileRuntimeScriptSource;
use shared::game::world::WorldControlMode;

#[test]
fn runtime_jsx_transpile_uses_the_special_jsx_runtime_import_name() -> anyhow::Result<()> {
    let transpiled_source = transpileRuntimeScriptSource(
        "root/gui_script",
        "export const gui = <div>Hello world</div>;\n",
        &WorldControlMode::Server,
        ObjectLocation::Server,
    )?;

    assert!(
        transpiled_source.contains("kube-ts-api/jsx-runtime"),
        "runtime JSX transpile did not use the special jsx-runtime import"
    );

    Ok(())
}

#[test]
fn runtime_jsx_transpile_uses_the_fragment_helper_import_for_fragments() -> anyhow::Result<()> {
    let transpiled_source = transpileRuntimeScriptSource(
        "root/gui_fragment_script",
        "export const gui = <><div /></>;\n",
        &WorldControlMode::Server,
        ObjectLocation::Server,
    )?;

    assert!(
        transpiled_source.contains("Fragment as"),
        "runtime JSX transpile did not use the fragment helper import"
    );

    Ok(())
}
