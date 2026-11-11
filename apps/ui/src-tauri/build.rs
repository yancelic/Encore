fn main() {
    println!("cargo:rerun-if-changed=icons/icon.ico");
    // Tauri embeds the app manifest in the app binary, but its test harness also
    // links TaskDialogIndirect through the mock runtime and needs Common Controls 6.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("test.manifest");
        println!("cargo:rerun-if-changed=test.manifest");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::build()
}
