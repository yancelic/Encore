use tauri_plugin_updater::UpdaterExt;

/// Run after bundling: verifies the release against the exact public key shipped in-app.
#[test]
#[ignore = "requires the signed NSIS bundle; build:installer runs this after bundling"]
fn prepared_installer_signature_matches_app_key() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let directory = root.join("target/release/bundle/nsis");
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("latest.json")).unwrap()).unwrap();
    let version = metadata["version"].as_str().unwrap();
    let mut bytes =
        std::fs::read(directory.join(format!("Encore_{version}_x64-setup.exe"))).unwrap();
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
    assert!(
        update.install(&bytes).is_err(),
        "Tampered packages must never launch NSIS"
    );
}
