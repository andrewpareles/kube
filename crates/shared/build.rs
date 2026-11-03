use std::{env, fs, path::Path};

// Lists deno_web's and deno_webidl's JS files for the binary to include. See embedDenoJsFiles in jsRuntime.rs for why.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../Cargo.lock");

    let lockfile = fs::read_to_string("../../Cargo.lock").unwrap();
    let registry_source_directory = home::cargo_home().unwrap().join("registry/src");

    let mut entries = Vec::new();
    for extension_name in ["deno_web", "deno_webidl"] {
        let mut lockfile_lines = lockfile.lines();
        lockfile_lines.find(|line| *line == format!("name = \"{extension_name}\"")).unwrap();
        let version = lockfile_lines.next().unwrap().trim_start_matches("version = \"").trim_end_matches('"');

        let package_directory = fs::read_dir(&registry_source_directory)
            .unwrap()
            .map(|registry| registry.unwrap().path().join(format!("{extension_name}-{version}")))
            .find(|path| path.is_dir())
            .unwrap();
        let mut file_paths: Vec<_> = fs::read_dir(package_directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "js"))
            .collect();
        file_paths.sort();
        for file_path in file_paths {
            let specifier = format!("ext:{extension_name}/{}", file_path.file_name().unwrap().to_str().unwrap());
            entries.push(format!(
                "    ({specifier:?}, deno_core::ascii_str_include!({:?})),\n",
                file_path.to_str().unwrap()
            ));
        }
    }

    let code = format!(
        "const CODE_OF_DENO_JS_FILE: &[(&str, deno_core::FastStaticString)] = &[\n{}];\n",
        entries.concat()
    );
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("code_of_deno_js_file.rs"), code).unwrap();
}
