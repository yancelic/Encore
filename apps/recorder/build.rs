fn main() {
    let icon = std::path::Path::new("../ui/src-tauri/icons/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if cfg!(target_os = "windows") && icon.exists() {
        let mut res = tauri_winres::WindowsResource::new();
        res.set_icon(icon.to_str().unwrap());
        res.compile().expect("failed to compile Windows icon resource");
    }
}
