use std::env;
use std::path::PathBuf;

fn main() {
    let mut attributes = tauri_build::Attributes::new();

    // tauri-build puts the Windows app manifest (Common Controls v6, needed by
    // the native dialogs) into the resource file, which only the app binary
    // links; test binaries then die with STATUS_ENTRYPOINT_NOT_FOUND. With
    // MSVC, embed the manifest through the linker instead, which applies to
    // every binary of this package, tests included.
    let windows_msvc = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows_msvc {
        let manifest =
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    }

    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
