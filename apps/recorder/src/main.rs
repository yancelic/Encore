#![cfg_attr(
    all(windows, not(debug_assertions), not(test)),
    windows_subsystem = "windows"
)]

#[cfg(windows)]
#[path = "windows.rs"]
mod windows_runtime;
#[cfg(windows)]
pub(crate) use windows_runtime::*;

#[cfg(windows)]
fn main() {
    windows_runtime::main();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Encore recording is not available on this platform yet. The library and editor can be opened with encore-ui.");
    std::process::exit(1);
}
