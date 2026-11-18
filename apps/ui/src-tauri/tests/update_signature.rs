#![cfg(any(windows, target_os = "macos"))]

use std::path::{Path, PathBuf};

use tauri_plugin_updater::UpdaterExt;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// Windows reads the prepared latest.json. macOS builds in CI before that file exists, so it
/// describes its own archive with the same fields latest.json will publish for darwin-aarch64.
#[cfg(windows)]
fn release() -> (serde_json::Value, Vec<u8>) {
    let directory = root().join("target/release/bundle/nsis");
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("latest.json")).unwrap()).unwrap();
    let version = metadata["version"].as_str().unwrap();
    let bytes = std::fs::read(directory.join(format!("Encore_{version}_x64-setup.exe"))).unwrap();
    (metadata, bytes)
}

#[cfg(target_os = "macos")]
fn release() -> (serde_json::Value, Vec<u8>) {
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root().join("apps/ui/src-tauri/tauri.conf.json")).unwrap(),
    )
    .unwrap();
    let version = config["version"].as_str().unwrap();
    let archive = root().join(format!(
        "target/release/bundle/dmg/Encore_{version}_aarch64.app.tar.gz"
    ));
    let signature = std::fs::read_to_string(archive.with_extension("gz.sig")).unwrap();
    let metadata = serde_json::json!({
        "version": version,
        "platforms": {
            "darwin-aarch64": {
                "signature": signature.trim(),
                "url": format!("https://github.com/yancelic/Encore/releases/download/v{version}/Encore_{version}_aarch64.app.tar.gz"),
            }
        }
    });
    (metadata, std::fs::read(archive).unwrap())
}

/// Run after bundling: verifies the release against the exact public key shipped in-app.
#[test]
#[ignore = "requires the signed release package; the release build runs this after bundling"]
fn prepared_installer_signature_matches_app_key() {
    let (metadata, mut bytes) = release();
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(tauri::generate_context!())
        .unwrap();
    // Bypass version comparison only in this test: bundle and verifier have the same version.
    let updater = app
        .updater_builder()
        .version_comparator(|_, _| true)
        .build()
        .unwrap();
    let update = updater.restore_update(metadata).unwrap().unwrap();
    update
        .verify(&bytes)
        .expect("Release must be signed by the key trusted by the app");
    bytes[0] ^= 1;
    assert!(
        update.verify(&bytes).is_err(),
        "Tampered packages must be rejected"
    );
    // install verifies first, so a tampered package never launches NSIS or replaces the app.
    assert!(
        update.install(&bytes).is_err(),
        "Tampered packages must never be installed"
    );
}
