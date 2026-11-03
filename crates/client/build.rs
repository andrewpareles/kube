//! Puts the Windows icon and version details inside `client.exe`.
//!
//! On macOS the icon lives in the `.app` bundle, so the packager alone is enough. Windows has no
//! bundle: Explorer, the task bar, the Start menu and the installer all read the icon out of the
//! executable itself, and an executable with no icon resource of its own gets the default grey
//! program icon no matter what the installer was told. So the icon is compiled into the binary.
//!
//! The `.ico` is the same file the packager points at, written by
//! `scripts/icon-gen/generateKubeIcons.mjs` and checked in, so it is here for every build rather
//! than only for the ones that ran the packaging scripts first.

fn main() {
    #[cfg(target_os = "windows")]
    embed_windows_resources();
}

#[cfg(target_os = "windows")]
fn embed_windows_resources() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set for build scripts"));
    let icon_path = manifest_dir.join("../../packaging/client/assets/windows/icon.ico");
    println!("cargo:rerun-if-changed={}", icon_path.display());

    let mut resources = winresource::WindowsResource::new();
    resources.set_icon(icon_path.to_str().expect("the repository path is valid UTF-8"));
    // These are what the executable's Properties dialog shows. Without them Windows falls back to
    // the crate name, so the app reads as "client" everywhere it names itself outside our own UI.
    resources.set("ProductName", "Kube.new");
    resources.set("FileDescription", "Kube.new");
    resources.set("CompanyName", "Glass Devtools, Inc.");
    resources.set("LegalCopyright", "Copyright (C) Glass Devtools, Inc.");

    resources.compile().expect("failed to compile the Windows resources for client.exe");
}
