fn main() {
    let icon = std::path::Path::new("../ui/src-tauri/icons/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if cfg!(target_os = "windows") && icon.exists() {
        let mut res = tauri_winres::WindowsResource::new();
        res.set_icon(icon.to_str().unwrap());
        res.set("FileDescription", "Encore Background Game & Audio Replay Recorder");
        res.set("ProductName", "Encore");
        res.set("CompanyName", "Encore");
        res.set("LegalCopyright", "Copyright (C) 2026 Encore Contributors (GPL-3.0)");
        res.set("OriginalFilename", "encore-recorder.exe");
        res.set("InternalName", "encore-recorder");
        res.compile().expect("failed to compile Windows icon and version resource");
    }
}
