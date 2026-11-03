use crate::game::world::WorldControlMode;
use deno_ast::{
    EmitOptions, ImportsNotUsedAsValues, JsxAutomaticOptions, JsxRuntime, MediaType, ParseParams, TranspileModuleOptions, TranspileOptions, parse_module,
};
use game_types::object_types::ObjectLocation;
use regex::Regex;
use std::{sync::Arc, sync::LazyLock};

// just an identifier we use internally to figure out what export we're talking about between client and server
fn FN_ID(script_path: &str, export_name: &str) -> String {
    format!("\"{export_name}#{script_path}\"")
}

// Each tuple is (exported name, local symbol name).
fn get_export_names(script_source: &str) -> Vec<(String, String)> {
    // finds export function x(){ ... }

    let mut export_names = Vec::new();

    // `export function x(){}` and `export async function x(){}`.
    static EXPORTED_REGEX_FUNCTION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?m)^\s*export\s+(?:async\s+)?function\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*\("#).unwrap());
    for captures in EXPORTED_REGEX_FUNCTION.captures_iter(script_source) {
        let export_name = captures[1].to_string();
        export_names.push((export_name.clone(), export_name));
    }

    // `export const x = ...`, `export let x = ...`, and `export var x = ...`.
    static EXPORTED_REGEX_CONST: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?m)^\s*export\s+(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*="#).unwrap());
    for captures in EXPORTED_REGEX_CONST.captures_iter(script_source) {
        let export_name = captures[1].to_string();
        export_names.push((export_name.clone(), export_name));
    }

    // `export { x }`, `export { x as y }`, and `export { x, x as y }`.
    // Finds export { x, y as z } and lets rust skip re-exports like export { x } from './other'
    static EXPORTED_REGEX_LIST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?ms)^\s*export\s*\{([^}]*)\}\s*(from\b)?"#).unwrap());
    for captures in EXPORTED_REGEX_LIST.captures_iter(script_source) {
        // Re-exports like `export { x } from './other'` do not have a local symbol for us to register here.
        if captures.get(2).is_some() {
            continue;
        }

        // `x` and `x as y`.
        for export_list_item_source in captures[1].split(',') {
            static EXPORTED_REGEX_LIST_ITEM: LazyLock<Regex> =
                LazyLock::new(|| Regex::new(r#"^\s*([A-Za-z_$][A-Za-z0-9_$]*)(?:\s+as\s+([A-Za-z_$][A-Za-z0-9_$]*))?\s*$"#).unwrap());
            let Some(export_list_item_captures) = EXPORTED_REGEX_LIST_ITEM.captures(export_list_item_source) else {
                continue;
            };
            let internal_name = export_list_item_captures[1].to_string();
            let export_name = match export_list_item_captures.get(2) {
                Some(alias_capture) => alias_capture.as_str().to_string(),
                None => internal_name.clone(),
            };
            export_names.push((export_name, internal_name));
        }
    }
    export_names
}
/*

The two transpile_to_X functions turn a file with 'use server' into raw JS code. it turns it into 1 of 2 things depending on whether it's server or client.

    // original source code:
    // 'use server'

    // export const fn_1: T = () => { ... }
    // export const fn_2: T = () => { ... }
    // export const fn_3: T = () => { ... }
    // export const fn_4: T = () => { ... }

    // and from a Client file:
    // import { fn_1 } from './sibling_script_name';
    // when editing, the linter sees whole script source code with types (above). dont look below when youre talking about linting because the linter doesnt see it. it only sees source code.

    // -----------
    // server transpile: adds this to the bottom of the 'use server' file
    // __registerJsServerFn__(fn_1)
    // __registerJsServerFn__(fn_2)
    // __registerJsServerFn__(fn_3)
    // __registerJsServerFn__(fn_4)

    // -----------
    // client transpile: literally just clear the whole 'use server' file and leave behind a bunch of export const lines that call __callJsServerFnFromClient__
    // export const fn_1 = (...args) => { await __callJsServerFnFromClient__("fn_1", ...args) }
    // export const fn_2 = (...args) => { await __callJsServerFnFromClient__("fn_2", ...args) }
    // export const fn_3 = (...args) => { await __callJsServerFnFromClient__("fn_3", ...args) }
    // export const fn_4 = (...args) => { await __callJsServerFnFromClient__("fn_4", ...args) }

*/

// --------------------------------- USE SERVER ---------------------------------
// takes the script and simply appends a bunch of `__registerJsServerFn__({fn_id_str}, {export_name});` to the end of the file, for each exported item.
fn transpile_to__registerJsServerFn__(script_path: &str, script_source: &str) -> String {
    let export_names = get_export_names(script_source);

    let mut transpiled_source = format!(
        r#"
import {{ __registerJsServerFn__ }} from 'kube-ts-api';
{script_source}
"#,
    );

    for (export_name, internal_name) in export_names {
        let fn_id_str = FN_ID(script_path, export_name.as_str());
        transpiled_source.push_str(
            format!(
                r#"
__registerJsServerFn__({fn_id_str}, {internal_name});
"#
            )
            .as_str(),
        );
    }

    log::info!("transpiled_to__registerJsServerFn__: {}", transpiled_source);
    transpiled_source
}

// simply returns an empty file with a bunch of `export const name = (...args) => __callJsServerFnFromClient__(FN_ID, ...args)`, one for each original exported item (fn does not depend on the export body, only args)
fn transpile_to__callJsServerFnFromClient__(script_path: &str, script_source: &str) -> String {
    let export_names = get_export_names(script_source);

    let mut transpiled_source = r#"
import { __callJsServerFnFromClient__ } from 'kube-ts-api';
"#
    .to_string();
    for (export_name, _) in export_names {
        let fn_id_str = FN_ID(script_path, export_name.as_str());
        transpiled_source.push_str(
            format!(
                r#"
export const {export_name} = async (...args) => {{
return await __callJsServerFnFromClient__({fn_id_str}, ...args);
}};
"#,
            )
            .as_str(),
        );
    }
    log::info!("transpiled_to__callJsServerFnFromClient__: {}", transpiled_source);
    transpiled_source
}

// --------------------------------- USE CLIENT ---------------------------------

fn transpile_to__registerJsClientFn__(script_path: &str, script_source: &str) -> String {
    let export_names = get_export_names(script_source);

    let mut transpiled_source = format!(
        r#"
import {{ __registerJsClientFn__ }} from 'kube-ts-api';
{script_source}
"#,
    );

    for (export_name, internal_name) in export_names {
        let fn_id_str = FN_ID(script_path, export_name.as_str());
        transpiled_source.push_str(
            format!(
                r#"
__registerJsClientFn__({fn_id_str}, {internal_name});
"#
            )
            .as_str(),
        );
    }

    log::info!("transpiled_to__registerJsClientFn__: {}", transpiled_source);
    transpiled_source
}

fn transpile_to__callJsClientFnFromServer__(script_path: &str, script_source: &str) -> String {
    let export_names = get_export_names(script_source);

    let mut transpiled_source = r#"
import { __callJsClientFnFromServer__ } from 'kube-ts-api';
"#
    .to_string();
    for (export_name, _) in export_names {
        let fn_id_str = FN_ID(script_path, export_name.as_str());
        transpiled_source.push_str(
            format!(
                r#"
export const {export_name} = async (playerObjectId, ...args) => {{
return await __callJsClientFnFromServer__(playerObjectId, {fn_id_str}, ...args);
}};
"#,
            )
            .as_str(),
        );
    }
    log::info!("transpiled_to__callJsClientFnFromServer__: {}", transpiled_source);
    transpiled_source
}

// just treat runtime scripts as TSX. we don't need extension branching here.
fn transpile_jsx_syntax(script_path: &str, script_source: &str) -> anyhow::Result<String> {
    let script_root_specifier = deno_ast::ModuleSpecifier::parse("file:///kube/")?;
    let script_specifier = script_root_specifier.join(script_path).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let parsed_source = parse_module(ParseParams {
        specifier: script_specifier,
        text: Arc::<str>::from(script_source.to_string()),
        media_type: MediaType::Tsx,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    })
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    let emitted_source = parsed_source
        .transpile(
            &TranspileOptions {
                imports_not_used_as_values: ImportsNotUsedAsValues::Remove,
                // `jsx-runtime` is a special automatic JSX entry name, so keep the import source on the
                // package root and let the compiler resolve `kube-ts-api/jsx-runtime`.
                jsx: Some(JsxRuntime::Automatic(JsxAutomaticOptions {
                    development: false,
                    import_source: Some("kube-ts-api".to_string()),
                })),
                ..Default::default()
            },
            &TranspileModuleOptions::default(),
            &EmitOptions::default(),
        )
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .into_source();

    Ok(emitted_source.text)
}

// --------------------------------- TRANSPILE ALL SCRIPTS ---------------------------------

// This rewrites the smallest useful slice of server-action files so we can keep one script source of truth.
fn transpileScriptSource(
    script_path: &str,
    script_source: &str,
    worldControlMode: &WorldControlMode,
    objectLocation: ObjectLocation,
) -> anyhow::Result<String> {
    // detects 'use server' at top of file
    static USE_SERVER_AT_TOP_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\A\s*(?:'use server'|"use server")"#).unwrap());
    static USE_CLIENT_AT_TOP_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\A\s*(?:'use client'|"use client")"#).unwrap());

    // all scripts run everywhere - server, client, editor. it is *crucial* that the filtering below works properly.
    match worldControlMode {
        WorldControlMode::Editor => {
            // do not change the source in edit mode
            return Ok(script_source.to_string());
        }
        WorldControlMode::Player(_) => {
            match objectLocation {
                // client script on client
                ObjectLocation::Client => {
                    if !USE_CLIENT_AT_TOP_REGEX.is_match(script_source) {
                        return Ok(script_source.to_string());
                    }
                    let transpiled_source = transpile_to__registerJsClientFn__(script_path, script_source);
                    return Ok(transpiled_source);
                }
                // server script on client
                ObjectLocation::Server => {
                    // this code should not be run at all on the client, unless it has 'use server' at the top
                    if !USE_SERVER_AT_TOP_REGEX.is_match(script_source) {
                        return Ok("".to_string());
                    }
                    let transpiled_source = transpile_to__callJsServerFnFromClient__(script_path, script_source);
                    return Ok(transpiled_source);
                }
            }
        }
        WorldControlMode::Server => {
            match objectLocation {
                // server script on server
                ObjectLocation::Server => {
                    // normal code if it doesn't have 'use server' at the top
                    if !USE_SERVER_AT_TOP_REGEX.is_match(script_source) {
                        return Ok(script_source.to_string());
                    }
                    let transpiled_source = transpile_to__registerJsServerFn__(script_path, script_source);
                    return Ok(transpiled_source);
                }
                // client script on server - should not be run at all
                ObjectLocation::Client => {
                    if !USE_CLIENT_AT_TOP_REGEX.is_match(script_source) {
                        return Ok("".to_string());
                    }
                    let transpiled_source = transpile_to__callJsClientFnFromServer__(script_path, script_source);
                    return Ok(transpiled_source);
                }
            }
        }
    }
}

// transpileScriptSource does the old use-server/use-client rewrite.
// transpileRuntimeScriptSource just runs that first, then does the real TSX transpile the runtime wants.
pub fn transpileRuntimeScriptSource(
    script_path: &str,
    script_source: &str,
    worldControlMode: &WorldControlMode,
    objectLocation: ObjectLocation,
) -> anyhow::Result<String> {
    let runtime_script_source = transpileScriptSource(script_path, script_source, worldControlMode, objectLocation)?;
    if runtime_script_source.is_empty() {
        return Ok(runtime_script_source);
    }

    transpile_jsx_syntax(script_path, runtime_script_source.as_str())
}

#[cfg(test)]
mod tests {
    use super::{WorldControlMode, transpileScriptSource};
    use game_types::object_types::ObjectLocation;
    use regex::Regex;
    use std::sync::LazyLock;

    struct ExportCase {
        case_name: &'static str,
        script_source: &'static str,
        body_marker: &'static str,
        expected_export_name_and_internal_name_pairs: &'static [(&'static str, &'static str)],
    }

    const SCRIPT_PATH: &str = "root/server_actions";
    const CLIENT_SCRIPT_PATH: &str = "root/client_actions";

    const EXPORT_CASES: &[ExportCase] = &[
        ExportCase {
            case_name: "export function",
            script_source: "'use server'\nexport function function_decl() {\n    return 'function_decl_marker';\n}\n",
            body_marker: "function_decl_marker",
            expected_export_name_and_internal_name_pairs: &[("function_decl", "function_decl")],
        },
        ExportCase {
            case_name: "export async function",
            script_source: "'use server'\nexport async function async_function_decl() {\n    return 'async_function_decl_marker';\n}\n",
            body_marker: "async_function_decl_marker",
            expected_export_name_and_internal_name_pairs: &[("async_function_decl", "async_function_decl")],
        },
        ExportCase {
            case_name: "export const value",
            script_source: "'use server'\nexport const const_value = 'const_value_marker';\n",
            body_marker: "const_value_marker",
            expected_export_name_and_internal_name_pairs: &[("const_value", "const_value")],
        },
        ExportCase {
            case_name: "export let value",
            script_source: "'use server'\nexport let let_value = 'let_value_marker';\n",
            body_marker: "let_value_marker",
            expected_export_name_and_internal_name_pairs: &[("let_value", "let_value")],
        },
        ExportCase {
            case_name: "export var value",
            script_source: "'use server'\nexport var var_value = 'var_value_marker';\n",
            body_marker: "var_value_marker",
            expected_export_name_and_internal_name_pairs: &[("var_value", "var_value")],
        },
        ExportCase {
            case_name: "export list plain",
            script_source: "'use server'\nconst list_plain = 'list_plain_marker';\nexport { list_plain };\n",
            body_marker: "list_plain_marker",
            expected_export_name_and_internal_name_pairs: &[("list_plain", "list_plain")],
        },
        ExportCase {
            case_name: "export list alias",
            script_source: "'use server'\nconst list_alias_local = 'list_alias_marker';\nexport { list_alias_local as list_alias_export };\n",
            body_marker: "list_alias_marker",
            expected_export_name_and_internal_name_pairs: &[("list_alias_export", "list_alias_local")],
        },
        ExportCase {
            case_name: "export list mixed local and alias",
            script_source: "'use server'\nconst list_multi_local = 'list_multi_marker';\nexport { list_multi_local, list_multi_local as list_multi_alias };\n",
            body_marker: "list_multi_marker",
            expected_export_name_and_internal_name_pairs: &[("list_multi_local", "list_multi_local"), ("list_multi_alias", "list_multi_local")],
        },
        ExportCase {
            case_name: "multiline export list",
            script_source: "'use server'\nconst multiline_first = 'multiline_first_marker';\nconst multiline_second = 'multiline_second_marker';\nexport {\n    multiline_first,\n    multiline_second as multiline_second_alias,\n};\n",
            body_marker: "multiline_second_marker",
            expected_export_name_and_internal_name_pairs: &[("multiline_first", "multiline_first"), ("multiline_second_alias", "multiline_second")],
        },
        ExportCase {
            case_name: "re-export from another module is skipped",
            script_source: "'use server'\nconst local_export = 'local_export_marker';\nexport { remote_value } from './remote_module';\nexport { local_export };\n",
            body_marker: "local_export_marker",
            expected_export_name_and_internal_name_pairs: &[("local_export", "local_export")],
        },
    ];

    fn client_stub_pairs(transpiled_source: &str) -> Vec<(String, String)> {
        static CLIENT_STUB_REGEX: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r#"(?m)^export const ([A-Za-z_$][A-Za-z0-9_$]*) = async \(\.\.\.args\) => \{\nreturn await __callJsServerFnFromClient__\(("([^"]+)"), \.\.\.args\);\n\};$"#,
            )
            .unwrap()
        });

        let mut pairs = Vec::new();
        for captures in CLIENT_STUB_REGEX.captures_iter(transpiled_source) {
            pairs.push((captures[1].to_string(), captures[2].to_string()));
        }
        pairs
    }

    fn server_registration_pairs(transpiled_source: &str) -> Vec<(String, String)> {
        static SERVER_REGISTRATION_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r#"(?m)^__registerJsServerFn__\(("([^"]+)"), ([A-Za-z_$][A-Za-z0-9_$]*)\);$"#).unwrap());

        let mut pairs = Vec::new();
        for captures in SERVER_REGISTRATION_REGEX.captures_iter(transpiled_source) {
            pairs.push((captures[1].to_string(), captures[3].to_string()));
        }
        pairs
    }

    fn expected_client_stub_pairs(export_name_and_internal_name_pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        for (export_name, _) in export_name_and_internal_name_pairs {
            pairs.push(((*export_name).to_string(), format!(r#""{export_name}#{SCRIPT_PATH}""#)));
        }
        pairs
    }

    fn expected_server_registration_pairs(export_name_and_internal_name_pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        for (export_name, internal_name) in export_name_and_internal_name_pairs {
            pairs.push((format!(r#""{export_name}#{SCRIPT_PATH}""#), (*internal_name).to_string()));
        }
        pairs
    }

    fn client_function_registration_pairs(transpiled_source: &str) -> Vec<(String, String)> {
        static CLIENT_FUNCTION_REGISTRATION_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r#"(?m)^__registerJsClientFn__\(("([^"]+)"), ([A-Za-z_$][A-Za-z0-9_$]*)\);$"#).unwrap());

        let mut pairs = Vec::new();
        for captures in CLIENT_FUNCTION_REGISTRATION_REGEX.captures_iter(transpiled_source) {
            pairs.push((captures[1].to_string(), captures[3].to_string()));
        }
        pairs
    }

    #[test]
    fn client_target_rewrites_all_supported_named_export_forms_into_stubs() -> anyhow::Result<()> {
        for export_case in EXPORT_CASES {
            let transpiled_source = transpileScriptSource(SCRIPT_PATH, export_case.script_source, &WorldControlMode::Player(1), ObjectLocation::Server)?;

            assert!(
                transpiled_source.contains("import { __callJsServerFnFromClient__ } from 'kube-ts-api';"),
                "missing client helper import for case: {}",
                export_case.case_name
            );
            assert!(
                !transpiled_source.contains("__registerJsServerFn__("),
                "client rewrite leaked server registration for case: {}",
                export_case.case_name
            );
            assert!(
                !transpiled_source.contains(export_case.body_marker),
                "client rewrite kept server body marker for case: {}",
                export_case.case_name
            );
            assert_eq!(
                client_stub_pairs(&transpiled_source),
                expected_client_stub_pairs(export_case.expected_export_name_and_internal_name_pairs),
                "client stub pairs did not match for case: {}",
                export_case.case_name
            );
        }

        Ok(())
    }

    #[test]
    fn server_target_registers_all_supported_named_export_forms() -> anyhow::Result<()> {
        for export_case in EXPORT_CASES {
            let transpiled_source = transpileScriptSource(SCRIPT_PATH, export_case.script_source, &WorldControlMode::Server, ObjectLocation::Server)?;

            assert!(
                !transpiled_source.contains("__callJsServerFnFromClient__"),
                "server rewrite leaked client helper for case: {}",
                export_case.case_name
            );
            assert!(
                transpiled_source.contains(export_case.body_marker),
                "server rewrite lost original body marker for case: {}",
                export_case.case_name
            );
            assert_eq!(
                server_registration_pairs(&transpiled_source),
                expected_server_registration_pairs(export_case.expected_export_name_and_internal_name_pairs),
                "server registration pairs did not match for case: {}",
                export_case.case_name
            );
        }

        Ok(())
    }

    #[test]
    fn nested_use_server_directives_do_not_trigger_transpile() -> anyhow::Result<()> {
        let script_source = "export function inner_fn() {\n    'use server'\n    return 20;\n}\n";
        let transpiled_source = transpileScriptSource("root/server_actions", script_source, &WorldControlMode::Server, ObjectLocation::Server)?;

        assert_eq!(transpiled_source, script_source);
        assert!(!transpiled_source.contains("__callJsServerFnFromClient__"));

        Ok(())
    }

    #[test]
    fn server_target_rewrites_use_client_modules_to_player_object_id_stubs() -> anyhow::Result<()> {
        let script_source = "'use client'\nexport const client_fn = () => 'client_fn_marker';\n";
        let transpiled_source = transpileScriptSource(CLIENT_SCRIPT_PATH, script_source, &WorldControlMode::Server, ObjectLocation::Client)?;

        assert!(
            transpiled_source.contains("import { __callJsClientFnFromServer__ } from 'kube-ts-api';"),
            "missing use client helper import"
        );
        assert!(
            transpiled_source.contains("export const client_fn = async (playerObjectId, ...args) => {"),
            "missing playerObjectId stub arg"
        );
        assert!(
            transpiled_source.contains(r#"return await __callJsClientFnFromServer__(playerObjectId, "client_fn#root/client_actions", ...args);"#),
            "missing use client helper call"
        );
        assert!(!transpiled_source.contains("userId"), "use client stub still references userId");
        assert!(!transpiled_source.contains("client_fn_marker"), "server rewrite kept use client body");

        Ok(())
    }

    #[test]
    fn client_target_registers_use_client_exports() -> anyhow::Result<()> {
        let script_source = "'use client'\nexport const client_fn = () => 'client_fn_marker';\n";
        let transpiled_source = transpileScriptSource(CLIENT_SCRIPT_PATH, script_source, &WorldControlMode::Player(1), ObjectLocation::Client)?;

        assert!(transpiled_source.contains("client_fn_marker"), "client rewrite lost use client body");
        assert_eq!(
            client_function_registration_pairs(&transpiled_source),
            vec![(r#""client_fn#root/client_actions""#.to_string(), "client_fn".to_string())]
        );

        Ok(())
    }
}
